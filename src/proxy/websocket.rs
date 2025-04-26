use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use hyper::{Body, Client, Request, Response, StatusCode};
use hyper_tls::HttpsConnector;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async, tungstenite::Message, WebSocketStream, MaybeTlsStream
};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use std::sync::Arc;
use std::net::SocketAddr;
use http::Uri;

use crate::proxy::handler::RequestContext;
use crate::config::data_model::Protocol;

/// Handles WebSocket proxying for the gateway
pub async fn handle_websocket(
    req: Request<Body>, 
    ctx: RequestContext,
    original_uri: Uri,
) -> Result<Response<Body>> {
    let backend_uri = build_ws_backend_uri(&ctx, &original_uri)?;
    
    // Check if the backend protocol is correctly set to ws or wss
    match ctx.proxy.backend_protocol {
        Protocol::Ws | Protocol::Wss => {
            // Good, continue with WebSocket handling
        },
        _ => {
            return Err(anyhow!("Backend protocol must be ws or wss for WebSocket proxying"));
        }
    }
    
    debug!("WebSocket backend URI: {}", backend_uri);
    
    // Create a response that will be upgraded
    let res = Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("Connection", "upgrade")
        .header("Upgrade", "websocket")
        .body(Body::empty())?;
    
    // Get the peer address for logging
    let client_addr = ctx.client_addr;
    
    // Spawn a task to handle the WebSocket upgrade and proxying
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                debug!("WebSocket connection upgraded for client: {}", client_addr);
                
                // Connect to the backend WebSocket
                if let Err(e) = proxy_websocket(upgraded, backend_uri, client_addr).await {
                    error!("WebSocket proxy error: {}", e);
                }
            },
            Err(e) => {
                error!("WebSocket upgrade error: {}", e);
            }
        }
    });
    
    Ok(res)
}

/// Create the backend WebSocket URI from the request context
fn build_ws_backend_uri(ctx: &RequestContext, original_uri: &Uri) -> Result<String> {
    let scheme = match ctx.proxy.backend_protocol {
        Protocol::Ws => "ws",
        Protocol::Wss => "wss",
        _ => return Err(anyhow!("Unsupported protocol for WebSocket")),
    };
    
    // Extract the path to forward
    let path = if ctx.proxy.strip_listen_path {
        // Strip the listen_path prefix
        let path_str = original_uri.path();
        let stripped = path_str.strip_prefix(&ctx.proxy.listen_path)
            .unwrap_or(path_str)
            .trim_start_matches('/');
        
        stripped.to_string()
    } else {
        // Keep the original path
        original_uri.path().to_string()
    };
    
    // Add the backend_path prefix if it exists
    let full_path = if let Some(backend_path) = &ctx.proxy.backend_path {
        // Ensure proper path joining
        let mut result = backend_path.trim_end_matches('/').to_string();
        if !path.is_empty() {
            result.push('/');
            result.push_str(&path);
        }
        result
    } else {
        path
    };
    
    // Add query parameters if they exist
    let full_path_with_query = if let Some(query) = original_uri.query() {
        format!("{}?{}", full_path, query)
    } else {
        full_path
    };
    
    // Build the final URI
    let uri = format!("{}://{}:{}/{}",
        scheme,
        ctx.proxy.backend_host,
        ctx.proxy.backend_port,
        full_path_with_query.trim_start_matches('/')
    );
    
    Ok(uri)
}

/// Proxies WebSocket connections between client and backend
async fn proxy_websocket(
    client_ws: Upgraded,
    backend_uri: String,
    client_addr: SocketAddr,
) -> Result<()> {
    // Create a WebSocketStream from the upgraded connection
    let client_ws_stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
        client_ws,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    ).await;
    
    debug!("Connecting to backend WebSocket at {}", backend_uri);
    
    // Connect to the backend WebSocket
    let (backend_ws_stream, _) = connect_async(&backend_uri).await
        .map_err(|e| anyhow!("Failed to connect to backend WebSocket: {}", e))?;
    
    debug!("Connected to backend WebSocket, setting up bidirectional proxy");
    
    // Split the WebSocket streams for concurrent reading and writing
    let (client_write, client_read) = client_ws_stream.split();
    let (backend_write, backend_read) = backend_ws_stream.split();
    
    // Create a channel for communicating between the two directions
    let (client_to_backend_tx, client_to_backend_rx) = mpsc::channel(32);
    let (backend_to_client_tx, backend_to_client_rx) = mpsc::channel(32);
    
    // Set up the client-to-backend direction
    tokio::spawn(proxy_ws_messages(
        client_read,
        client_to_backend_tx,
        format!("client-{}", client_addr),
        "backend".to_string(),
    ));
    
    tokio::spawn(forward_ws_messages(
        client_to_backend_rx,
        backend_write,
        format!("client-{}", client_addr),
        "backend".to_string(),
    ));
    
    // Set up the backend-to-client direction
    tokio::spawn(proxy_ws_messages(
        backend_read,
        backend_to_client_tx,
        "backend".to_string(),
        format!("client-{}", client_addr),
    ));
    
    tokio::spawn(forward_ws_messages(
        backend_to_client_rx,
        client_write,
        "backend".to_string(),
        format!("client-{}", client_addr),
    ));
    
    debug!("WebSocket proxy established between client {} and backend {}", client_addr, backend_uri);
    
    Ok(())
}

/// Reads messages from a WebSocket stream and sends them to a channel
async fn proxy_ws_messages<S>(
    mut read: futures_util::stream::SplitStream<WebSocketStream<S>>,
    tx: mpsc::Sender<Message>,
    from: String,
    to: String,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    debug!("Starting WebSocket message proxy from {} to {}", from, to);
    
    while let Some(message_result) = read.next().await {
        match message_result {
            Ok(message) => {
                // Check if it's a close message
                if message.is_close() {
                    debug!("Received close message from {}", from);
                    // Forward the close message
                    if tx.send(message).await.is_err() {
                        debug!("Channel closed, stopping message proxy from {} to {}", from, to);
                        break;
                    }
                    break;
                }
                
                // Log message type for debugging (except for pings/pongs)
                if !message.is_ping() && !message.is_pong() {
                    let msg_type = if message.is_binary() {
                        "binary"
                    } else if message.is_text() {
                        "text"
                    } else {
                        "other"
                    };
                    
                    debug!("Proxying {} message from {} to {}", msg_type, from, to);
                }
                
                // Forward the message
                if tx.send(message).await.is_err() {
                    debug!("Channel closed, stopping message proxy from {} to {}", from, to);
                    break;
                }
            },
            Err(e) => {
                warn!("Error reading WebSocket message from {}: {}", from, e);
                break;
            }
        }
    }
    
    debug!("WebSocket message proxy from {} to {} ended", from, to);
    Ok(())
}

/// Forwards messages from a channel to a WebSocket stream
async fn forward_ws_messages<S>(
    mut rx: mpsc::Receiver<Message>,
    mut write: futures_util::stream::SplitSink<WebSocketStream<S>, Message>,
    from: String,
    to: String,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    debug!("Starting WebSocket message forwarding from {} to {}", from, to);
    
    while let Some(message) = rx.recv().await {
        if let Err(e) = write.send(message).await {
            warn!("Error writing WebSocket message to {}: {}", to, e);
            break;
        }
    }
    
    debug!("WebSocket message forwarding from {} to {} ended", from, to);
    
    // Try to close the WebSocket gracefully
    let _ = write.close().await;
    
    Ok(())
}
