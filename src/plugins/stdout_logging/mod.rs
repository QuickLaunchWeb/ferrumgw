use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, header};
use serde::{Serialize, Deserialize};
use tracing::info;
use chrono::{DateTime, Utc};

use crate::plugins::Plugin;
use crate::proxy::handler::RequestContext;

/// Configuration for the stdout logging plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdoutLoggingConfig {
    /// Whether to enable JSON format (default: true)
    #[serde(default = "default_true")]
    pub json_format: bool,
}

fn default_true() -> bool {
    true
}

impl Default for StdoutLoggingConfig {
    fn default() -> Self {
        Self {
            json_format: true,
        }
    }
}

/// Plugin that logs transaction summaries to standard output
pub struct StdoutLoggingPlugin {
    config: StdoutLoggingConfig,
}

impl StdoutLoggingPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| StdoutLoggingConfig::default());
        
        Ok(Self { config })
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
impl Plugin for StdoutLoggingPlugin {
    fn name(&self) -> &'static str {
        "stdout_logging"
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
        
        // Log the summary
        if self.config.json_format {
            let json = serde_json::to_string(&summary)?;
            info!("{}", json);
        } else {
            info!(
                "[{}] {} {} -> {} - {} - consumer: {} - latency: {}ms (gateway: {}ms, backend: {}ms, ttfb: {}ms)",
                summary.timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
                summary.http_method,
                summary.request_path,
                summary.backend_target_url,
                summary.status_code,
                summary.consumer_username.as_deref().unwrap_or("-"),
                summary.latency_total_ms,
                summary.latency_gateway_processing_ms,
                summary.latency_backend_total_ms,
                summary.latency_backend_ttfb_ms,
            );
        }
        
        Ok(())
    }
}
