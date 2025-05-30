use std::sync::Arc;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug, trace};
use hyper::{Body, Request, Response, StatusCode, Uri, header};
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;
use http::uri::Scheme;

use crate::config::data_model::{Configuration, Proxy, BackendProtocol};
use crate::proxy::router::Router;
use crate::dns::DnsCache; // Updated import from the dns module
use crate::plugins::PluginManager;
use crate::proxy::websocket::handle_websocket;

type HttpClient = hyper::Client<HttpsConnector<HttpConnector>>;

/// The ProxyHandler is responsible for forwarding requests to the appropriate
/// backend service and processing the response.
pub struct ProxyHandler {
    shared_config: Arc<RwLock<Configuration>>,
    plugin_manager: Arc<PluginManager>,
    dns_cache: Arc<DnsCache>,
    http_client: HttpClient,
}

impl ProxyHandler {
    pub fn new(
        shared_config: Arc<RwLock<Configuration>>,
        plugin_manager: Arc<PluginManager>,
        dns_cache: Arc<DnsCache>,
    ) -> Self {
        // Create a custom DNS resolver that will use our cache
        let mut http = hyper::client::HttpConnector::new();
        http.set_nodelay(true);
        http.enforce_http(false); // Allow HTTPS and other schemes
        http.set_connect_timeout(Some(Duration::from_secs(10)));
        
        // Create a HTTPS connector with our custom DNS and TLS config
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_only()
            .enable_http1()
            .enable_http2()
            .wrap_connector(http);
        
        // Create a hyper client with the HTTPS connector
        let http_client = hyper::Client::builder()
            .pool_idle_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(32)
            .build(https);
        
        Self {
            shared_config,
            plugin_manager,
            dns_cache,
            http_client,
        }
    }
    
    /// Handles a request by forwarding it to the appropriate backend service
    /// and processing the response through the plugin pipeline.
    pub async fn handle(
        &self,
        req: Request<Body>,
        proxy: Proxy,
        client_addr: SocketAddr,
    ) -> Result<Response<Body>> {
        let start_time = Instant::now();
        
        // Create a context for this request
        let mut context = RequestContext {
            proxy: proxy.clone(),
            client_addr,
            consumer: None,
            latency: Default::default(),
        };
        
        // Check for WebSocket upgrade request
        if Self::is_websocket_request(&req) && (proxy.backend_protocol == BackendProtocol::Ws || proxy.backend_protocol == BackendProtocol::Wss) {
            debug!("Handling WebSocket upgrade request for path: {}", req.uri().path());
            return handle_websocket(req, context, req.uri().clone()).await;
        }

        // Run pre-proxy plugins (authentication, access control, etc.)
        let (modified_req, should_continue) = match self.plugin_manager.run_pre_proxy_plugins(req, &mut context).await {
            Ok((modified_req, true)) => (modified_req, true),
            Ok((modified_req, false)) => {
                // Plugin indicated that we should not continue with the proxy
                let rejection_response = Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Body::from("Request rejected by plugin"))
                    .unwrap();
                
                // Run post-proxy plugins with the rejection response
                let response = self.plugin_manager.run_post_proxy_plugins(rejection_response, &mut context).await
                    .unwrap_or_else(|e| {
                        error!("Error in post-proxy plugins: {}", e);
                        rejection_response
                    });
                
                // Always run logging phase
                if let Err(e) = self.plugin_manager.run_log_plugins(&modified_req, &response, &context).await {
                    error!("Error in logging plugins: {}", e);
                }
                
                return Ok(response);
            },
            Err(e) => {
                // Plugin error
                error!("Error in pre-proxy plugins: {}", e);
                
                let error_response = Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Internal server error in request processing"))
                    .unwrap();
                
                // Try to run logging phase even for errors
                if let Err(log_err) = self.plugin_manager.run_log_plugins(&req, &error_response, &context).await {
                    error!("Error in logging plugins: {}", log_err);
                }
                
                return Ok(error_response);
            }
        };
        
        if !should_continue {
            // This shouldn't happen based on the match above, but just in case
            let error_response = Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal server error: plugin chain inconsistency"))
                .unwrap();
            
            // Run logging phase
            if let Err(e) = self.plugin_manager.run_log_plugins(&modified_req, &error_response, &context).await {
                error!("Error in logging plugins: {}", e);
            }
            
            return Ok(error_response);
        }
        
        // Resolve the backend host to an IP address
        let backend_ip = match self.resolve_backend_host(&proxy).await {
            Ok(ip) => ip,
            Err(e) => {
                error!("Failed to resolve backend host {}: {}", proxy.backend_host, e);
                
                let response = Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from("Failed to resolve backend host"))
                    .unwrap();
                
                // Run logging phase
                if let Err(log_err) = self.plugin_manager.run_log_plugins(&modified_req, &response, &context).await {
                    error!("Error in logging plugins: {}", log_err);
                }
                
                return Ok(response);
            }
        };
        
        // Create a router instance for path construction
        let router = Router::new(Arc::clone(&self.shared_config));
        
        // Build the backend URI
        let backend_path = router.construct_backend_path(&modified_req, &proxy);
        let backend_uri = match self.build_backend_uri(&proxy, &backend_ip, &backend_path, &modified_req) {
            Ok(uri) => uri,
            Err(e) => {
                error!("Failed to build backend URI: {}", e);
                
                let response = Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Failed to build backend URI"))
                    .unwrap();
                
                // Run logging phase
                if let Err(log_err) = self.plugin_manager.run_log_plugins(&modified_req, &response, &context).await {
                    error!("Error in logging plugins: {}", log_err);
                }
                
                return Ok(response);
            }
        };
        
        // Prepare the outgoing request to the backend
        let (backend_req, outgoing_body) = match self.prepare_backend_request(modified_req.clone(), &proxy, backend_uri) {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to prepare backend request: {}", e);
                
                let response = Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Failed to prepare backend request"))
                    .unwrap();
                
                // Run logging phase
                if let Err(log_err) = self.plugin_manager.run_log_plugins(&modified_req, &response, &context).await {
                    error!("Error in logging plugins: {}", log_err);
                }
                
                return Ok(response);
            }
        };
        
        // Record time before making backend request
        let backend_start = Instant::now();
        
        // Send the request to the backend
        let resp = match self.http_client.request(backend_req).await {
            Ok(resp) => {
                // Record backend response time
                context.latency.backend_ttfb = backend_start.elapsed().as_millis() as u64;
                context.latency.backend_total = backend_start.elapsed().as_millis() as u64;
                resp
            },
            Err(e) => {
                error!("Error sending request to backend: {}", e);
                
                let response = Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from(format!("Error sending request to backend: {}", e)))
                    .unwrap();
                
                // Record backend failure
                context.latency.backend_ttfb = 0;
                context.latency.backend_total = backend_start.elapsed().as_millis() as u64;
                
                // Run logging phase
                if let Err(log_err) = self.plugin_manager.run_log_plugins(&modified_req, &response, &context).await {
                    error!("Error in logging plugins: {}", log_err);
                }
                
                return Ok(response);
            }
        };
        
        // Process the backend response through post-proxy plugins
        let processed_resp = match self.plugin_manager.run_post_proxy_plugins(resp, &mut context).await {
            Ok(resp) => resp,
            Err(e) => {
                error!("Error in post-proxy plugins: {}", e);
                
                // If post-proxy plugins fail, return a server error
                let error_response = Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Error processing backend response"))
                    .unwrap();
                
                error_response
            }
        };
        
        // Record total request latency
        context.latency.total = start_time.elapsed().as_millis() as u64;
        context.latency.gateway_processing = context.latency.total - context.latency.backend_total;
        
        // Log request summary
        self.log_request_summary(&context, &modified_req, &processed_resp);
        
        // Run logging phase plugins
        if let Err(e) = self.plugin_manager.run_log_plugins(&modified_req, &processed_resp, &context).await {
            error!("Error in logging plugins: {}", e);
        }
        
        // Return the processed response
        Ok(processed_resp)
    }
    
    /// Resolves a backend hostname to an IP address using the DNS cache
    async fn resolve_backend_host(&self, proxy: &Proxy) -> Result<String> {
        // Check if there's a DNS override for this proxy
        if let Some(ref ip) = proxy.dns_override {
            return Ok(ip.clone());
        }
        
        // Otherwise resolve the hostname using the DNS cache
        let ttl = proxy.dns_cache_ttl_seconds
            .map(Duration::from_secs)
            .unwrap_or_else(|| self.dns_cache.default_ttl());
        
        self.dns_cache.lookup_with_ttl(&proxy.backend_host, ttl).await // Using lookup_with_ttl instead of resolve_with_ttl
    }
    
    /// Builds the backend URI for the request
    fn build_backend_uri(&self, proxy: &Proxy, backend_ip: &str, backend_path: &str, original_req: &Request<Body>) -> Result<Uri> {
        // Determine the scheme based on the backend protocol
        let scheme = match proxy.backend_protocol {
            BackendProtocol::Http => Scheme::HTTP,
            BackendProtocol::Https => Scheme::HTTPS,
            BackendProtocol::Ws => Scheme::HTTP,
            BackendProtocol::Wss => Scheme::HTTPS,
            BackendProtocol::Grpc => Scheme::HTTP,
        };
        
        // Preserve the query string from the original request
        let query = original_req.uri().query().map(|q| format!("?{}", q)).unwrap_or_default();
        
        // Construct the backend URI
        let uri_str = format!(
            "{}://{}:{}{}{}",
            scheme,
            backend_ip,
            proxy.backend_port,
            backend_path,
            query
        );
        
        uri_str.parse::<Uri>().context("Failed to parse backend URI")
    }
    
    /// Prepares the outgoing request to the backend
    fn prepare_backend_request(
        &self,
        original_req: Request<Body>,
        proxy: &Proxy,
        backend_uri: Uri,
    ) -> Result<(Request<Body>, Body)> {
        let (parts, body) = original_req.into_parts();
        
        // Create a new request with the backend URI
        let mut req_builder = Request::builder()
            .uri(backend_uri)
            .method(parts.method);
        
        // Copy all headers from the original request
        for (name, value) in parts.headers.iter() {
            // Skip the Host header - it will be set based on the backend URI
            if name.as_str().to_lowercase() != "host" {
                req_builder = req_builder.header(name, value);
            }
        }
        
        // Set Host header to the backend host
        let host = format!("{}:{}", 
            proxy.backend_host,
            proxy.backend_port
        );
        req_builder = req_builder.header("Host", host);
        
        // Set X-Forwarded headers
        let forwarded_for = match parts.headers.get("X-Forwarded-For") {
            Some(forwarded_for) => {
                let mut forwarded = forwarded_for.to_str()?.to_string();
                forwarded.push_str(", ");
                forwarded.push_str(&parts.extensions.get::<SocketAddr>().map(|addr| addr.ip().to_string()).unwrap_or_else(|| "unknown".to_string()));
                forwarded
            },
            None => parts.extensions.get::<SocketAddr>().map(|addr| addr.ip().to_string()).unwrap_or_else(|| "unknown".to_string()),
        };
        
        req_builder = req_builder.header("X-Forwarded-For", forwarded_for);
        req_builder = req_builder.header("X-Forwarded-Proto", parts.uri.scheme_str().unwrap_or("http"));
        req_builder = req_builder.header("X-Forwarded-Host", parts.uri.host().unwrap_or("unknown"));
        
        // Create the final request with an empty body for now
        // We'll return the original body separately
        let backend_req = req_builder
            .body(Body::empty())?;
        
        Ok((backend_req, body))
    }
    
    /// Processes the backend response before returning it to the client
    async fn process_backend_response(&self, mut response: Response<Body>) -> Result<Response<Body>> {
        // Process response headers
        // (future: modify headers as needed here)
        
        // Return the processed response
        Ok(response)
    }
    
    /// Logs a summary of the request and response
    fn log_request_summary(&self, context: &RequestContext, req: &Request<Body>, resp: &Response<Body>) {
        info!(
            "Request processed: method={}, path={}, status={}, backend={:?}:{}, latency_ms={}",
            req.method(),
            req.uri().path(),
            resp.status().as_u16(),
            context.proxy.backend_protocol,
            context.proxy.backend_port,
            context.latency.backend_ttfb,
        );
    }
    
    /// Helper method to detect WebSocket upgrade requests
    fn is_websocket_request(req: &Request<Body>) -> bool {
        // Check for the upgrade header with value "websocket"
        if let Some(upgrade_header) = req.headers().get("upgrade") {
            if let Ok(value) = upgrade_header.to_str() {
                if value.to_lowercase() == "websocket" {
                    // Also check for the connection header containing "upgrade"
                    if let Some(connection_header) = req.headers().get("connection") {
                        if let Ok(value) = connection_header.to_str() {
                            return value.to_lowercase().contains("upgrade");
                        }
                    }
                }
            }
        }
        false
    }
}

/// A struct to represent a consumer context for a request
#[derive(Debug, Clone)]
pub struct Consumer {
    pub id: String,
    pub username: String,
    pub custom_id: Option<String>,
}

/// A struct to track latency metrics for a request
#[derive(Debug, Default)]
pub struct LatencyMetrics {
    /// Total request processing time
    pub total: u64,
    /// Time spent in gateway processing before sending to backend
    pub gateway_processing: u64,
    /// Time to first byte from backend
    pub backend_ttfb: u64,
    /// Total time spent interacting with backend
    pub backend_total: u64,
}

/// A context object for a single request through the gateway
pub struct RequestContext {
    /// The proxy configuration that matched this request
    pub proxy: Proxy,
    /// The client's IP address
    pub client_addr: SocketAddr,
    /// The authenticated consumer (if any)
    pub consumer: Option<Consumer>,
    /// Latency metrics for the request
    pub latency: LatencyMetrics,
}
