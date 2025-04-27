use std::sync::Arc;
use std::net::SocketAddr;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::net::TcpListener;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use hyper::server::conn::Http;
use hyper::service::{service_fn, make_service_fn};
use hyper::{Body, Request, Response, StatusCode};
use quinn::{ServerConfig as QuinnServerConfig, Endpoint, TransportConfig};
use h3_quinn::quinn::{self, ClientConfig};
use h3::server::{Connection as H3Connection, Builder as H3ServerBuilder};
use h3_quinn::server::Connection as H3QuinnConnection;
use h3::quic::SendStream;
use futures::{StreamExt, TryStreamExt};
use bytes::Bytes;
use tokio::io::AsyncWriteExt;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use crate::config::env_config::EnvConfig;
use crate::config::data_model::{Configuration, Proxy, BackendProtocol};
use crate::proxy::router::Router;
use crate::proxy::handler::ProxyHandler;
use crate::plugins::PluginManager;
use crate::proxy::update_manager::UpdateManager;
use crate::dns::cache::DnsCache; // Add correct import for DNS cache

mod router;
mod handler;
mod tls;
mod websocket;
mod update_manager;

pub struct ProxyServer {
    env_config: EnvConfig,
    shared_config: Arc<RwLock<Configuration>>,
    plugin_manager: Arc<PluginManager>,
    dns_cache: Arc<DnsCache>,
    update_manager: Arc<UpdateManager>,
}

impl ProxyServer {
    pub fn new(
        env_config: EnvConfig,
        shared_config: Arc<RwLock<Configuration>>,
        dns_cache: Arc<DnsCache>,
    ) -> Result<Self> {
        // Initialize the plugin manager
        let plugin_manager = Arc::new(PluginManager::new());
        
        // Initialize the router and update manager
        let router = Arc::new(Router::new(Arc::clone(&shared_config)));
        let update_manager = Arc::new(UpdateManager::new(Arc::clone(&router)));
        
        Ok(Self {
            env_config,
            shared_config,
            plugin_manager,
            dns_cache,
            update_manager,
        })
    }
    
    /// Get a reference to the update manager
    pub fn get_update_manager(&self) -> Arc<UpdateManager> {
        Arc::clone(&self.update_manager)
    }
    
    pub async fn start(self) -> Result<()> {
        // Warn if neither HTTP nor HTTPS nor HTTP/3 is enabled
        if self.env_config.proxy_http_port.is_none() 
           && self.env_config.proxy_https_port.is_none()
           && self.env_config.proxy_http3_port.is_none() {
            warn!("No proxy ports are enabled. Gateway will not accept any traffic.");
            return Ok(());
        }
        
        // Start HTTP server if enabled
        if let Some(http_port) = self.env_config.proxy_http_port {
            let addr = format!("0.0.0.0:{}", http_port).parse::<SocketAddr>()?;
            let shared_config = Arc::clone(&self.shared_config);
            let plugin_manager = Arc::clone(&self.plugin_manager);
            let dns_cache = Arc::clone(&self.dns_cache);
            let max_header_size = self.env_config.max_header_size_bytes;
            let max_body_size = self.env_config.max_body_size_bytes;
            
            info!("Starting HTTP/1.1 and HTTP/2 server on {}", addr);
            
            tokio::spawn(async move {
                if let Err(e) = Self::run_http_server(
                    addr, 
                    shared_config, 
                    plugin_manager,
                    dns_cache,
                    max_header_size,
                    max_body_size,
                ).await {
                    error!("HTTP server error: {}", e);
                }
            });
        }
        
        // Start HTTPS server if enabled
        if let Some(https_port) = self.env_config.proxy_https_port {
            if let (Some(cert_path), Some(key_path)) = (
                &self.env_config.proxy_tls_cert_path,
                &self.env_config.proxy_tls_key_path,
            ) {
                let addr = format!("0.0.0.0:{}", https_port).parse::<SocketAddr>()?;
                let shared_config = Arc::clone(&self.shared_config);
                let plugin_manager = Arc::clone(&self.plugin_manager);
                let dns_cache = Arc::clone(&self.dns_cache);
                let max_header_size = self.env_config.max_header_size_bytes;
                let max_body_size = self.env_config.max_body_size_bytes;
                let cert_path = cert_path.clone();
                let key_path = key_path.clone();
                
                info!("Starting HTTPS (HTTP/1.1 and HTTP/2 over TLS) server on {}", addr);
                
                tokio::spawn(async move {
                    if let Err(e) = Self::run_https_server(
                        addr,
                        cert_path,
                        key_path,
                        shared_config,
                        plugin_manager,
                        dns_cache,
                        max_header_size,
                        max_body_size,
                    ).await {
                        error!("HTTPS server error: {}", e);
                    }
                });
            } else {
                warn!("HTTPS port is enabled but TLS certificate and/or key path is not provided. HTTPS server will not start.");
            }
        }
        
        // Start HTTP/3 server if enabled
        if let Some(http3_port) = self.env_config.proxy_http3_port {
            if let (Some(cert_path), Some(key_path)) = (
                &self.env_config.proxy_tls_cert_path,
                &self.env_config.proxy_tls_key_path,
            ) {
                let addr = format!("0.0.0.0:{}", http3_port).parse::<SocketAddr>()?;
                let shared_config = Arc::clone(&self.shared_config);
                let plugin_manager = Arc::clone(&self.plugin_manager);
                let dns_cache = Arc::clone(&self.dns_cache);
                let max_body_size = self.env_config.max_body_size_bytes;
                let cert_path = cert_path.clone();
                let key_path = key_path.clone();
                
                info!("Starting HTTP/3 server on {}", addr);
                
                tokio::spawn(async move {
                    if let Err(e) = Self::run_http3_server(
                        addr,
                        cert_path,
                        key_path,
                        shared_config,
                        plugin_manager,
                        dns_cache,
                        max_body_size,
                    ).await {
                        error!("HTTP/3 server error: {}", e);
                    }
                });
            } else {
                warn!("HTTP/3 port is enabled but TLS certificate and/or key path is not provided. HTTP/3 server will not start.");
            }
        }
        
        // Perform DNS warmup for all backend hostnames
        self.warmup_dns_cache().await;
        
        // Prevent the function from returning (server runs in background tasks)
        loop {
            tokio::time::sleep(Duration::from_secs(60 * 60)).await; // Sleep for an hour
        }
    }
    
    async fn run_http_server(
        addr: SocketAddr,
        shared_config: Arc<RwLock<Configuration>>,
        plugin_manager: Arc<PluginManager>,
        dns_cache: Arc<DnsCache>,
        max_header_size: usize,
        max_body_size: usize,
    ) -> Result<()> {
        // Create TCP listener
        let listener = TcpListener::bind(addr).await?;
        
        // Create the router
        let router = Arc::new(Router::new(Arc::clone(&shared_config)));
        
        // Create the handler
        let handler = Arc::new(ProxyHandler::new(
            Arc::clone(&shared_config),
            Arc::clone(&plugin_manager),
            Arc::clone(&dns_cache),
        ));
        
        // Accept and serve connections
        loop {
            let (stream, remote_addr) = match listener.accept().await {
                Ok((stream, addr)) => (stream, addr),
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };
            
            // Clone the necessary components for the connection handler
            let router_clone = Arc::clone(&router);
            let handler_clone = Arc::clone(&handler);
            
            // Configure HTTP server with appropriate limits
            let http = Http::new()
                .with_executor(tokio::runtime::Handle::current())
                .max_buf_size(max_header_size)
                .http1_only(false)
                .http2_only(false)
                .http1_keep_alive(true)
                .http2_keep_alive_interval(Some(Duration::from_secs(30)));
            
            // Spawn a task to serve the connection
            tokio::spawn(async move {
                if let Err(e) = http
                    .serve_connection(
                        stream,
                        service_fn(move |req| {
                            let router = Arc::clone(&router_clone);
                            let handler = Arc::clone(&handler_clone);
                            let remote_addr = remote_addr;
                            
                            async move {
                                Self::handle_request(
                                    req, 
                                    router, 
                                    handler, 
                                    remote_addr,
                                    max_body_size,
                                ).await
                            }
                        }),
                    )
                    .await
                {
                    error!("Error serving connection: {}", e);
                }
            });
        }
    }
    
    async fn run_https_server(
        addr: SocketAddr,
        cert_path: String,
        key_path: String,
        shared_config: Arc<RwLock<Configuration>>,
        plugin_manager: Arc<PluginManager>,
        dns_cache: Arc<DnsCache>,
        max_header_size: usize,
        max_body_size: usize,
    ) -> Result<()> {
        // Load TLS configuration
        let tls_config = tls::load_server_config(&cert_path, &key_path)
            .context("Failed to load TLS configuration")?;
        
        // Create TCP listener
        let listener = TcpListener::bind(addr).await?;
        
        // Create the router
        let router = Arc::new(Router::new(Arc::clone(&shared_config)));
        
        // Create the handler
        let handler = Arc::new(ProxyHandler::new(
            Arc::clone(&shared_config),
            Arc::clone(&plugin_manager),
            Arc::clone(&dns_cache),
        ));
        
        // Accept and serve connections
        loop {
            let (stream, remote_addr) = match listener.accept().await {
                Ok((stream, addr)) => (stream, addr),
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };
            
            // Clone the necessary components for the connection handler
            let router_clone = Arc::clone(&router);
            let handler_clone = Arc::clone(&handler);
            let tls_config = tls_config.clone();
            
            // Perform TLS handshake
            let tls_stream = match tls::accept_connection(stream, tls_config).await {
                Ok(tls_stream) => tls_stream,
                Err(e) => {
                    error!("TLS handshake failed: {}", e);
                    continue;
                }
            };
            
            // Configure HTTP server with appropriate limits
            let http = Http::new()
                .with_executor(tokio::runtime::Handle::current())
                .max_buf_size(max_header_size)
                .http1_only(false)
                .http2_only(false)
                .http1_keep_alive(true)
                .http2_keep_alive_interval(Some(Duration::from_secs(30)));
            
            // Spawn a task to serve the connection
            tokio::spawn(async move {
                if let Err(e) = http
                    .serve_connection(
                        tls_stream,
                        service_fn(move |req| {
                            let router = Arc::clone(&router_clone);
                            let handler = Arc::clone(&handler_clone);
                            let remote_addr = remote_addr;
                            
                            async move {
                                Self::handle_request(
                                    req, 
                                    router, 
                                    handler, 
                                    remote_addr,
                                    max_body_size,
                                ).await
                            }
                        }),
                    )
                    .await
                {
                    error!("Error serving TLS connection: {}", e);
                }
            });
        }
    }
    
    async fn run_http3_server(
        addr: SocketAddr,
        cert_path: String,
        key_path: String,
        shared_config: Arc<RwLock<Configuration>>,
        plugin_manager: Arc<PluginManager>,
        dns_cache: Arc<DnsCache>,
        max_body_size: usize,
    ) -> Result<()> {
        // Load TLS configuration for QUIC
        let tls_config = tls::load_server_config(&cert_path, &key_path)
            .context("Failed to load TLS configuration for HTTP/3")?;
        
        // Create the router
        let router = Arc::new(Router::new(Arc::clone(&shared_config)));
        
        // Create the handler
        let handler = Arc::new(ProxyHandler::new(
            Arc::clone(&shared_config),
            Arc::clone(&plugin_manager),
            Arc::clone(&dns_cache),
        ));
        
        // Configure and build the QUIC server
        let mut server_config = QuinnServerConfig::default();
        
        // Set up the TLS configuration for QUIC
        let mut client_config = ClientConfig::default();
        client_config.crypto = tls_config.clone();
        client_config.crypto.alpn_protocols = vec![b"h3".to_vec()]; // HTTP/3 protocol identifier
        
        // Configure the transport
        let mut transport_config = TransportConfig::default();
        transport_config.keep_alive_interval(Some(Duration::from_secs(10)));
        transport_config.max_concurrent_uni_streams(100u32.into());
        
        server_config.crypto = tls_config.clone();
        server_config.transport = Arc::new(transport_config);
        
        // Bind to UDP socket
        let socket = UdpSocket::bind(addr)?;
        let endpoint = Endpoint::new(
            Default::default(),
            Some(server_config),
            socket,
        )?;
        
        info!("HTTP/3 server listening on {}", addr);
        
        // Build the H3 server
        let h3_server = H3ServerBuilder::new().build();
        
        // Accept connections
        while let Some(conn) = endpoint.accept().await {
            match conn.await {
                Ok(new_conn) => {
                    // Clone references for this connection
                    let router_clone = Arc::clone(&router);
                    let handler_clone = Arc::clone(&handler);
                    let h3_server = h3_server.clone();
                    
                    // Get the remote address
                    let remote_addr = new_conn.remote_address();
                    
                    tokio::spawn(async move {
                        // Create an H3 connection from the QUIC connection
                        match H3QuinnConnection::new(h3_server, new_conn).await {
                            Ok(h3_conn) => {
                                debug!("New HTTP/3 connection from {}", remote_addr);
                                
                                // Process requests on this connection
                                if let Err(e) = Self::process_h3_requests(
                                    h3_conn,
                                    router_clone,
                                    handler_clone,
                                    remote_addr,
                                    max_body_size,
                                ).await {
                                    error!("Error processing HTTP/3 requests: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to establish HTTP/3 connection: {}", e);
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("Connection failed: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    async fn process_h3_requests<T: h3::quic::Connection>(
        mut h3_conn: H3Connection<T>,
        router: Arc<Router>,
        handler: Arc<ProxyHandler>,
        remote_addr: SocketAddr,
        max_body_size: usize,
    ) -> Result<()> {
        // Process each request in the connection
        while let Some(request) = h3_conn.accept().await? {
            // Clone references for this request
            let router_clone = Arc::clone(&router);
            let handler_clone = Arc::clone(&handler);
            
            // Process the request in a separate task
            tokio::spawn(async move {
                // Convert HTTP/3 request to hyper Request
                let (parts, body) = request.into_parts();
                let hyper_req = Request::from_parts(parts, Body::wrap_stream(body));
                
                // Handle the request using our existing handler
                match Self::handle_request(
                    hyper_req,
                    router_clone,
                    handler_clone,
                    remote_addr,
                    max_body_size,
                ).await {
                    Ok(hyper_resp) => {
                        // Convert hyper Response back to HTTP/3 response
                        let (parts, body) = hyper_resp.into_parts();
                        
                        // Create h3 response
                        let mut resp = h3::server::Response::from_parts(parts, ());
                        
                        // Send the response headers
                        match request.respond(resp).await {
                            Ok(mut send_stream) => {
                                // Create a buffer to collect chunks
                                let mut buffer = Vec::new();
                                
                                // Process the hyper body stream
                                let mut body_stream = body.into_data_stream();
                                
                                // Read and forward chunks from hyper body to h3 send_stream
                                while let Some(chunk_result) = body_stream.next().await {
                                    match chunk_result {
                                        Ok(chunk) => {
                                            // Write chunk to send stream
                                            if let Err(e) = send_stream.write_chunk(chunk).await {
                                                error!("Failed to write HTTP/3 body chunk: {}", e);
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            error!("Error reading response body chunk: {}", e);
                                            break;
                                        }
                                    }
                                }
                                
                                // Finish the stream
                                if let Err(e) = send_stream.finish().await {
                                    error!("Failed to finish HTTP/3 response: {}", e);
                                } else {
                                    debug!("HTTP/3 response sent to {}", remote_addr);
                                }
                            }
                            Err(e) => {
                                error!("Failed to send HTTP/3 response: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error handling HTTP/3 request: {}", e);
                        // Send error response
                        let error_resp = h3::server::Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(())
                            .unwrap();
                        
                        if let Err(e) = request.respond(error_resp).await {
                            error!("Failed to send HTTP/3 error response: {}", e);
                        }
                    }
                }
            });
        }
        
        Ok(())
    }
    
    async fn handle_request(
        req: Request<Body>,
        router: Arc<Router>,
        handler: Arc<ProxyHandler>,
        remote_addr: SocketAddr,
        max_body_size: usize,
    ) -> Result<Response<Body>, hyper::Error> {
        // Check request body size (if Content-Length is provided)
        if let Some(length) = req.headers().get(hyper::header::CONTENT_LENGTH) {
            if let Ok(size) = length.to_str().unwrap_or("0").parse::<usize>() {
                if max_body_size > 0 && size > max_body_size {
                    return Ok(Response::builder()
                        .status(StatusCode::PAYLOAD_TOO_LARGE)
                        .body(Body::from("Request body too large"))
                        .unwrap());
                }
            }
        }
        
        // Match the request to a proxy configuration
        match router.route(&req).await {
            Some(proxy_config) => {
                // Handle the request with the matched proxy
                match handler.handle(req, proxy_config, remote_addr).await {
                    Ok(response) => Ok(response),
                    Err(e) => {
                        error!("Proxy handler error: {}", e);
                        
                        // Return an internal server error
                        Ok(Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::from("Internal Server Error"))
                            .unwrap())
                    }
                }
            },
            None => {
                // No matching proxy found
                debug!("No matching proxy for path: {}", req.uri().path());
                
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("Not Found"))
                    .unwrap())
            }
        }
    }
    
    async fn warmup_dns_cache(&self) {
        info!("Warming up DNS cache with backend hostnames");
        
        // Get unique backend hostnames from all proxies
        let hostnames = {
            let config = self.shared_config.read().await;
            
            let mut unique_hosts = std::collections::HashSet::new();
            for proxy in &config.proxies {
                // Skip if DNS override is configured for this proxy
                if proxy.dns_override.is_some() {
                    continue;
                }
                
                unique_hosts.insert(proxy.backend_host.clone());
            }
            
            unique_hosts.into_iter().collect::<Vec<_>>()
        };
        
        // Start parallel DNS lookups
        for hostname in hostnames {
            let dns_cache = Arc::clone(&self.dns_cache);
            
            tokio::spawn(async move {
                if let Err(e) = dns_cache.resolve(&hostname).await {
                    warn!("DNS warmup failed for host {}: {}", hostname, e);
                } else {
                    debug!("DNS warmup successful for host {}", hostname);
                }
            });
        }
    }
}
