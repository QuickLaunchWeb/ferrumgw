#[cfg(test)]
mod websocket_tests {
    use std::collections::HashMap;
    use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    use std::sync::Arc;
    use std::time::Duration;
    use chrono::Utc;
    use futures_util::{SinkExt, StreamExt};
    use http::{HeaderMap, Method, Request, Response, StatusCode};
    use hyper::{Body, Client, Server};
    use hyper::service::{make_service_fn, service_fn};
    use tokio::net::TcpListener;
    use tokio::sync::RwLock;
    use tokio::time::sleep;
    use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};
    use serde_json::json;
    
    use ferrumgw::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode};
    use ferrumgw::proxy::router::Router;
    use ferrumgw::proxy::websocket::handle_websocket;
    use ferrumgw::proxy::handler::RequestContext;
    
    // Helper to create a test proxy for WebSocket
    fn create_ws_test_proxy(id: &str, listen_path: &str, backend_host: &str, backend_port: u16) -> Proxy {
        Proxy {
            id: id.to_string(),
            name: Some(format!("Test Proxy {}", id)),
            listen_path: listen_path.to_string(),
            backend_protocol: Protocol::Ws,
            backend_host: backend_host.to_string(),
            backend_port,
            backend_path: None,
            strip_listen_path: true,
            preserve_host_header: false,
            backend_connect_timeout_ms: 5000,
            backend_read_timeout_ms: 30000,
            backend_write_timeout_ms: 30000,
            backend_tls_client_cert_path: None,
            backend_tls_client_key_path: None,
            backend_tls_verify_server_cert: true,
            backend_tls_server_ca_cert_path: None,
            dns_override: None,
            dns_cache_ttl_seconds: None,
            auth_mode: AuthMode::Single,
            plugins: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
    
    // Start a mock WebSocket echo server
    async fn start_mock_ws_server() -> u16 {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
        let listener = TcpListener::bind(&addr).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        
        // Spawn a task to handle incoming connections
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                // For each incoming connection, upgrade to WebSocket and echo messages
                tokio::spawn(async move {
                    let ws_stream = tokio_tungstenite::accept_async(stream).await.unwrap();
                    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
                    
                    // Echo all received messages back to the client
                    while let Some(msg_result) = ws_receiver.next().await {
                        match msg_result {
                            Ok(msg) => {
                                if msg.is_close() {
                                    break;
                                }
                                
                                // Echo the message back
                                if let Err(e) = ws_sender.send(msg).await {
                                    eprintln!("Error sending WebSocket message: {}", e);
                                    break;
                                }
                            },
                            Err(e) => {
                                eprintln!("Error receiving WebSocket message: {}", e);
                                break;
                            }
                        }
                    }
                });
            }
        });
        
        port
    }
    
    #[tokio::test]
    async fn test_websocket_proxy_build_uri() {
        // Test that the WebSocket backend URI is built correctly with various proxy configurations
        
        // Mock WebSocket server port
        let backend_port = 12345;
        
        // Test with strip_listen_path = true, no backend_path
        let proxy = create_ws_test_proxy("test1", "/ws", "127.0.0.1", backend_port);
        let ctx = RequestContext {
            proxy: proxy.clone(),
            original_uri: "http://example.com/ws/echo?param=value".parse().unwrap(),
            client_addr: "127.0.0.1:54321".parse().unwrap(),
            consumer: None,
            auth_status: None,
            request_id: "req123".to_string(),
            start_time: Utc::now(),
            metrics: HashMap::new(),
        };
        
        let backend_uri = ferrumgw::proxy::websocket::build_ws_backend_uri(&ctx).unwrap();
        assert_eq!(backend_uri, "ws://127.0.0.1:12345/echo?param=value");
        
        // Test with strip_listen_path = false, no backend_path
        let mut proxy2 = proxy.clone();
        proxy2.strip_listen_path = false;
        let ctx2 = RequestContext {
            proxy: proxy2,
            original_uri: "http://example.com/ws/echo?param=value".parse().unwrap(),
            client_addr: "127.0.0.1:54321".parse().unwrap(),
            consumer: None,
            auth_status: None,
            request_id: "req123".to_string(),
            start_time: Utc::now(),
            metrics: HashMap::new(),
        };
        
        let backend_uri = ferrumgw::proxy::websocket::build_ws_backend_uri(&ctx2).unwrap();
        assert_eq!(backend_uri, "ws://127.0.0.1:12345/ws/echo?param=value");
        
        // Test with strip_listen_path = true, with backend_path
        let mut proxy3 = proxy.clone();
        proxy3.backend_path = Some("/api".to_string());
        let ctx3 = RequestContext {
            proxy: proxy3,
            original_uri: "http://example.com/ws/echo?param=value".parse().unwrap(),
            client_addr: "127.0.0.1:54321".parse().unwrap(),
            consumer: None,
            auth_status: None,
            request_id: "req123".to_string(),
            start_time: Utc::now(),
            metrics: HashMap::new(),
        };
        
        let backend_uri = ferrumgw::proxy::websocket::build_ws_backend_uri(&ctx3).unwrap();
        assert_eq!(backend_uri, "ws://127.0.0.1:12345/api/echo?param=value");
    }
    
    #[tokio::test]
    async fn test_websocket_proxy_echo() {
        // Start a mock WebSocket echo server
        let backend_port = start_mock_ws_server().await;
        
        // Create a WebSocket proxy configuration
        let proxy = create_ws_test_proxy("test", "/ws", "127.0.0.1", backend_port);
        
        let config = Configuration {
            proxies: vec![proxy.clone()],
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        
        // Set up a simple HTTP server that will handle the upgrade and proxy the WebSocket
        let config_store = Arc::new(RwLock::new(config));
        let router = Router::new(config_store.clone());
        
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
        let listener = TcpListener::bind(&server_addr).await.unwrap();
        let server_port = listener.local_addr().unwrap().port();
        
        // Spawn the proxy server
        tokio::spawn(async move {
            while let Ok((stream, socket_addr)) = listener.accept().await {
                let proxy_clone = proxy.clone();
                let router_clone = router.clone();
                
                tokio::spawn(async move {
                    let mut http = hyper::server::conn::Http::new();
                    http.http1_only(true);
                    
                    let service = make_service_fn(move |_| {
                        let proxy_clone = proxy_clone.clone();
                        let router_clone = router_clone.clone();
                        let socket_addr = socket_addr;
                        
                        async move {
                            Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
                                let proxy_clone = proxy_clone.clone();
                                let ctx = RequestContext {
                                    proxy: proxy_clone,
                                    original_uri: req.uri().clone(),
                                    client_addr: socket_addr,
                                    consumer: None,
                                    auth_status: None,
                                    request_id: "test-req-id".to_string(),
                                    start_time: Utc::now(),
                                    metrics: HashMap::new(),
                                };
                                
                                async move {
                                    // Handle WebSocket upgrade
                                    handle_websocket(req, ctx).await.map_err(|e| {
                                        eprintln!("WebSocket handler error: {}", e);
                                        hyper::Error::from_static("WebSocket error")
                                    })
                                }
                            }))
                        }
                    });
                    
                    if let Err(e) = http.serve_connection(stream, service).await {
                        eprintln!("Error serving HTTP connection: {}", e);
                    }
                });
            }
        });
        
        // Allow the server to start
        sleep(Duration::from_millis(100)).await;
        
        // Connect to the proxy as a WebSocket client
        let ws_url = format!("ws://127.0.0.1:{}/ws/echo", server_port);
        let (ws_stream, _) = connect_async(&ws_url).await.unwrap();
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        
        // Send a test message
        let test_message = "Hello, WebSocket!";
        ws_sender.send(Message::Text(test_message.to_string())).await.unwrap();
        
        // Receive the echo response
        if let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Text(text) = msg {
                assert_eq!(text, test_message);
            } else {
                panic!("Expected text message");
            }
        } else {
            panic!("Did not receive echo response");
        }
        
        // Send a binary message
        let binary_data = vec![1, 2, 3, 4, 5];
        ws_sender.send(Message::Binary(binary_data.clone())).await.unwrap();
        
        // Receive the echo response
        if let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Binary(data) = msg {
                assert_eq!(data, binary_data);
            } else {
                panic!("Expected binary message");
            }
        } else {
            panic!("Did not receive echo response");
        }
        
        // Close the connection
        ws_sender.send(Message::Close(None)).await.unwrap();
    }
}
