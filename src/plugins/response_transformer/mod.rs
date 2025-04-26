use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, header};
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use std::collections::HashMap;

use crate::plugins::Plugin;
use crate::proxy::handler::RequestContext;

/// Configuration for the response transformer plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseTransformerConfig {
    /// Headers to add to the response
    #[serde(default)]
    pub add_headers: HashMap<String, String>,
    
    /// Headers to remove from the response
    #[serde(default)]
    pub remove_headers: Vec<String>,
    
    /// Headers to replace in the response
    #[serde(default)]
    pub replace_headers: HashMap<String, String>,
    
    /// Whether to hide the Server header
    #[serde(default = "default_false")]
    pub hide_server_header: bool,
    
    /// Whether to add a Via header
    #[serde(default = "default_false")]
    pub add_via_header: bool,
    
    /// Value for the Via header
    #[serde(default = "default_via_value")]
    pub via_value: String,
}

fn default_false() -> bool {
    false
}

fn default_via_value() -> String {
    "Ferrum Gateway".to_string()
}

impl Default for ResponseTransformerConfig {
    fn default() -> Self {
        Self {
            add_headers: HashMap::new(),
            remove_headers: Vec::new(),
            replace_headers: HashMap::new(),
            hide_server_header: false,
            add_via_header: false,
            via_value: default_via_value(),
        }
    }
}

/// Plugin that transforms responses before they are sent back to the client
pub struct ResponseTransformerPlugin {
    config: ResponseTransformerConfig,
}

impl ResponseTransformerPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| ResponseTransformerConfig::default());
        
        Ok(Self { config })
    }
    
    /// Transform the response headers according to the configuration
    fn transform_headers(&self, resp: &mut Response<Body>) {
        // Remove headers
        for header_name in &self.config.remove_headers {
            resp.headers_mut().remove(header_name);
            debug!("Removed response header: {}", header_name);
        }
        
        // Handle special server header logic
        if self.config.hide_server_header {
            resp.headers_mut().remove(header::SERVER);
            debug!("Removed Server header");
        }
        
        // Handle Via header
        if self.config.add_via_header {
            let via_value = format!("1.1 {}", self.config.via_value);
            resp.headers_mut().insert(
                header::VIA,
                header::HeaderValue::from_str(&via_value).unwrap_or_else(|_| {
                    warn!("Invalid Via header value: {}", via_value);
                    header::HeaderValue::from_static("1.1 Ferrum")
                })
            );
            debug!("Added Via header: {}", via_value);
        }
        
        // Replace headers
        for (name, value) in &self.config.replace_headers {
            if resp.headers().contains_key(name) {
                resp.headers_mut().insert(
                    header::HeaderName::from_bytes(name.as_bytes()).unwrap_or_else(|_| {
                        warn!("Invalid header name: {}", name);
                        header::HeaderName::from_static("x-invalid-header")
                    }),
                    header::HeaderValue::from_str(value).unwrap_or_else(|_| {
                        warn!("Invalid header value for {}: {}", name, value);
                        header::HeaderValue::from_static("")
                    })
                );
                debug!("Replaced response header: {} = {}", name, value);
            }
        }
        
        // Add headers
        for (name, value) in &self.config.add_headers {
            if !resp.headers().contains_key(name) {
                resp.headers_mut().insert(
                    header::HeaderName::from_bytes(name.as_bytes()).unwrap_or_else(|_| {
                        warn!("Invalid header name: {}", name);
                        header::HeaderName::from_static("x-invalid-header")
                    }),
                    header::HeaderValue::from_str(value).unwrap_or_else(|_| {
                        warn!("Invalid header value for {}: {}", name, value);
                        header::HeaderValue::from_static("")
                    })
                );
                debug!("Added response header: {} = {}", name, value);
            }
        }
    }
}

#[async_trait]
impl Plugin for ResponseTransformerPlugin {
    fn name(&self) -> &'static str {
        "response_transformer"
    }
    
    async fn after_proxy(&self, resp: &mut Response<Body>, ctx: &mut RequestContext) -> Result<()> {
        debug!(
            "Transforming response for proxy: {}",
            ctx.proxy.name.as_deref().unwrap_or("unnamed")
        );
        
        // Transform headers
        self.transform_headers(resp);
        
        Ok(())
    }
}
