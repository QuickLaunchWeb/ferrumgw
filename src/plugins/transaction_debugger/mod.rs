use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, header};
use serde::{Serialize, Deserialize};
use tracing::{debug, info};
use chrono::Utc;
use futures::StreamExt;

use crate::plugins::Plugin;
use crate::proxy::handler::RequestContext;

/// Configuration for the transaction debugger plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionDebuggerConfig {
    /// Whether to log request bodies
    #[serde(default = "default_false")]
    pub log_request_body: bool,
    
    /// Whether to log response bodies
    #[serde(default = "default_false")]
    pub log_response_body: bool,
    
    /// Maximum body size to log (in bytes)
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,
}

fn default_false() -> bool {
    false
}

fn default_max_body_size() -> usize {
    1024 * 10 // 10 KB
}

impl Default for TransactionDebuggerConfig {
    fn default() -> Self {
        Self {
            log_request_body: false,
            log_response_body: false,
            max_body_size: default_max_body_size(),
        }
    }
}

/// Plugin that logs detailed request and response information for debugging
pub struct TransactionDebuggerPlugin {
    config: TransactionDebuggerConfig,
}

impl TransactionDebuggerPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| TransactionDebuggerConfig::default());
        
        Ok(Self { config })
    }
    
    /// Format headers for logging
    fn format_headers(headers: &header::HeaderMap) -> String {
        let mut result = String::new();
        for (name, value) in headers.iter() {
            if let Ok(value_str) = value.to_str() {
                result.push_str(&format!("    {}: {}\n", name, value_str));
            } else {
                result.push_str(&format!("    {}: <binary>\n", name));
            }
        }
        result
    }
}

#[async_trait]
impl Plugin for TransactionDebuggerPlugin {
    fn name(&self) -> &'static str {
        "transaction_debugger"
    }
    
    async fn on_request_received(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        info!(
            "[TRANSACTION_DEBUGGER] Request received: {} {}",
            req.method(),
            req.uri()
        );
        
        debug!("[TRANSACTION_DEBUGGER] Request headers:\n{}", Self::format_headers(req.headers()));
        
        // Log request body if configured
        if self.config.log_request_body {
            let (parts, body) = req.clone().into_parts();
            let body_bytes = hyper::body::to_bytes(body).await?;
            
            if body_bytes.len() > 0 {
                // Check if the body is too large
                let body_to_log = if body_bytes.len() > self.config.max_body_size {
                    format!(
                        "<first {} bytes of {} total>: {}",
                        self.config.max_body_size,
                        body_bytes.len(),
                        String::from_utf8_lossy(&body_bytes[..self.config.max_body_size])
                    )
                } else {
                    String::from_utf8_lossy(&body_bytes).to_string()
                };
                
                debug!("[TRANSACTION_DEBUGGER] Request body: {}", body_to_log);
            } else {
                debug!("[TRANSACTION_DEBUGGER] Request body: <empty>");
            }
            
            // Reconstruct the request with the original body
            *req = Request::from_parts(parts, Body::from(body_bytes));
        }
        
        // Continue processing the request
        Ok(true)
    }
    
    async fn after_proxy(&self, resp: &mut Response<Body>, ctx: &mut RequestContext) -> Result<()> {
        info!(
            "[TRANSACTION_DEBUGGER] Response received: {} for {} {}",
            resp.status(),
            ctx.proxy.name.as_deref().unwrap_or("unnamed"),
            ctx.proxy.listen_path
        );
        
        debug!("[TRANSACTION_DEBUGGER] Response headers:\n{}", Self::format_headers(resp.headers()));
        
        // Log response body if configured
        if self.config.log_response_body {
            let (parts, body) = resp.clone().into_parts();
            let body_bytes = hyper::body::to_bytes(body).await?;
            
            if body_bytes.len() > 0 {
                // Check if the body is too large
                let body_to_log = if body_bytes.len() > self.config.max_body_size {
                    format!(
                        "<first {} bytes of {} total>: {}",
                        self.config.max_body_size,
                        body_bytes.len(),
                        String::from_utf8_lossy(&body_bytes[..self.config.max_body_size])
                    )
                } else {
                    String::from_utf8_lossy(&body_bytes).to_string()
                };
                
                debug!("[TRANSACTION_DEBUGGER] Response body: {}", body_to_log);
            } else {
                debug!("[TRANSACTION_DEBUGGER] Response body: <empty>");
            }
            
            // Reconstruct the response with the original body
            *resp = Response::from_parts(parts, Body::from(body_bytes));
        }
        
        Ok(())
    }
}
