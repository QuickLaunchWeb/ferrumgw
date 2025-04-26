use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, header, Client, Method, Uri};
use serde::{Serialize, Deserialize};
use tracing::{info, error};
use chrono::{DateTime, Utc};

use crate::plugins::Plugin;
use crate::proxy::handler::RequestContext;

/// Configuration for the HTTP logging plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpLoggingConfig {
    /// The URL to send logs to
    pub endpoint_url: String,
    
    /// Optional authorization header value
    pub authorization_header: Option<String>,
    
    /// Optional custom headers to include in the request
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    
    /// Timeout in milliseconds for the HTTP request
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    
    /// Whether to use a batch/queue system for logs (reduces overhead)
    #[serde(default = "default_false")]
    pub use_batch: bool,
    
    /// Maximum batch size if batching is enabled
    #[serde(default = "default_batch_size")]
    pub max_batch_size: usize,
    
    /// Maximum wait time in milliseconds before sending a batch
    #[serde(default = "default_flush_interval")]
    pub flush_interval_ms: u64,
}

fn default_timeout() -> u64 {
    5000
}

fn default_batch_size() -> usize {
    100
}

fn default_flush_interval() -> u64 {
    1000
}

fn default_false() -> bool {
    false
}

impl Default for HttpLoggingConfig {
    fn default() -> Self {
        Self {
            endpoint_url: "http://localhost:8080/logs".to_string(),
            authorization_header: None,
            headers: std::collections::HashMap::new(),
            timeout_ms: default_timeout(),
            use_batch: default_false(),
            max_batch_size: default_batch_size(),
            flush_interval_ms: default_flush_interval(),
        }
    }
}

/// Plugin that logs transaction summaries to an HTTP endpoint
pub struct HttpLoggingPlugin {
    config: HttpLoggingConfig,
    client: Client<hyper::client::HttpConnector>,
}

impl HttpLoggingPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| HttpLoggingConfig::default());
        
        // Create an HTTP client
        let client = Client::new();
        
        Ok(Self { config, client })
    }
    
    /// Sends a log entry to the configured HTTP endpoint
    async fn send_log(&self, summary: &TransactionSummary) -> Result<()> {
        // Create request body
        let json = serde_json::to_string(summary)?;
        let body = Body::from(json);
        
        // Parse the endpoint URL
        let uri: Uri = self.config.endpoint_url.parse()?;
        
        // Build the request
        let mut req_builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        
        // Add authorization header if configured
        if let Some(auth) = &self.config.authorization_header {
            req_builder = req_builder.header(header::AUTHORIZATION, auth);
        }
        
        // Add custom headers
        for (name, value) in &self.config.headers {
            req_builder = req_builder.header(name, value);
        }
        
        // Build the request
        let request = req_builder
            .body(body)
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP request: {}", e))?;
        
        // Send the request with a timeout
        let timeout_duration = std::time::Duration::from_millis(self.config.timeout_ms);
        let response = tokio::time::timeout(
            timeout_duration,
            self.client.request(request)
        ).await;
        
        // Check for timeout
        match response {
            Ok(Ok(resp)) => {
                if !resp.status().is_success() {
                    error!(
                        "HTTP logging request failed: endpoint={}, status={}",
                        self.config.endpoint_url,
                        resp.status()
                    );
                }
            },
            Ok(Err(e)) => {
                error!(
                    "HTTP logging request error: endpoint={}, error={}",
                    self.config.endpoint_url,
                    e
                );
            },
            Err(_) => {
                error!(
                    "HTTP logging request timed out after {}ms: endpoint={}",
                    self.config.timeout_ms,
                    self.config.endpoint_url
                );
            }
        }
        
        Ok(())
    }
}

/// Transaction summary for logging
#[derive(Debug, Serialize)]
struct TransactionSummary {
    timestamp: DateTime<Utc>,
    client_ip: String,
    consumer_id: Option<String>,
    consumer_username: Option<String>,
    http_method: String,
    request_path: String,
    proxy_id: String,
    proxy_name: Option<String>,
    backend_target_url: String,
    status_code: u16,
    latency_total_ms: u64,
    latency_gateway_processing_ms: u64,
    latency_backend_ttfb_ms: u64,
    latency_backend_total_ms: u64,
    user_agent: Option<String>,
}

#[async_trait]
impl Plugin for HttpLoggingPlugin {
    fn name(&self) -> &'static str {
        "http_logging"
    }
    
    async fn log(&self, req: &Request<Body>, resp: &Response<Body>, ctx: &RequestContext) -> Result<()> {
        // Extract the user agent
        let user_agent = req.headers()
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        
        // Build the backend target URL
        let backend_proto = match ctx.proxy.backend_protocol {
            crate::config::data_model::BackendProtocol::Http => "http",
            crate::config::data_model::BackendProtocol::Https => "https",
            crate::config::data_model::BackendProtocol::Ws => "ws",
            crate::config::data_model::BackendProtocol::Wss => "wss",
            crate::config::data_model::BackendProtocol::Grpc => "grpc",
        };
        
        let backend_path = ctx.proxy.backend_path.as_deref().unwrap_or("");
        let backend_target_url = format!(
            "{}://{}:{}{}",
            backend_proto,
            ctx.proxy.backend_host,
            ctx.proxy.backend_port,
            backend_path
        );
        
        // Create transaction summary
        let summary = TransactionSummary {
            timestamp: Utc::now(),
            client_ip: ctx.client_addr.to_string(),
            consumer_id: ctx.consumer.as_ref().map(|c| c.id.clone()),
            consumer_username: ctx.consumer.as_ref().map(|c| c.username.clone()),
            http_method: req.method().to_string(),
            request_path: req.uri().path().to_string(),
            proxy_id: ctx.proxy.id.clone(),
            proxy_name: ctx.proxy.name.clone(),
            backend_target_url,
            status_code: resp.status().as_u16(),
            latency_total_ms: ctx.latency.total.as_millis() as u64,
            latency_gateway_processing_ms: ctx.latency.gateway_processing.as_millis() as u64,
            latency_backend_ttfb_ms: ctx.latency.backend_ttfb.as_millis() as u64,
            latency_backend_total_ms: ctx.latency.backend_total.as_millis() as u64,
            user_agent,
        };
        
        // If batching is not enabled, send the log immediately
        if !self.config.use_batch {
            return self.send_log(&summary).await;
        }
        
        // In a complete implementation, we would add the log to a batch queue
        // and have a background task that periodically flushes the queue.
        // For simplicity, we'll just send it immediately for now.
        info!("Batching would be used here to send logs in bulk");
        self.send_log(&summary).await
    }
}
