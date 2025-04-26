use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, header, Uri};
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use std::collections::HashMap;

use crate::plugins::Plugin;
use crate::proxy::handler::RequestContext;

/// Configuration for the request transformer plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestTransformerConfig {
    /// Headers to add to the request
    #[serde(default)]
    pub add_headers: HashMap<String, String>,
    
    /// Headers to remove from the request
    #[serde(default)]
    pub remove_headers: Vec<String>,
    
    /// Headers to replace in the request
    #[serde(default)]
    pub replace_headers: HashMap<String, String>,
    
    /// Query parameters to add to the request
    #[serde(default)]
    pub add_query_params: HashMap<String, String>,
    
    /// Query parameters to remove from the request
    #[serde(default)]
    pub remove_query_params: Vec<String>,
    
    /// Query parameters to replace in the request
    #[serde(default)]
    pub replace_query_params: HashMap<String, String>,
}

impl Default for RequestTransformerConfig {
    fn default() -> Self {
        Self {
            add_headers: HashMap::new(),
            remove_headers: Vec::new(),
            replace_headers: HashMap::new(),
            add_query_params: HashMap::new(),
            remove_query_params: Vec::new(),
            replace_query_params: HashMap::new(),
        }
    }
}

/// Plugin that transforms requests before they are sent to the backend
pub struct RequestTransformerPlugin {
    config: RequestTransformerConfig,
}

impl RequestTransformerPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| RequestTransformerConfig::default());
        
        Ok(Self { config })
    }
    
    /// Transform the request headers according to the configuration
    fn transform_headers(&self, req: &mut Request<Body>) {
        // Remove headers
        for header_name in &self.config.remove_headers {
            req.headers_mut().remove(header_name);
            debug!("Removed header: {}", header_name);
        }
        
        // Replace headers
        for (name, value) in &self.config.replace_headers {
            if req.headers().contains_key(name) {
                req.headers_mut().insert(
                    header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                    header::HeaderValue::from_str(value).unwrap()
                );
                debug!("Replaced header: {} = {}", name, value);
            }
        }
        
        // Add headers
        for (name, value) in &self.config.add_headers {
            if !req.headers().contains_key(name) {
                req.headers_mut().insert(
                    header::HeaderName::from_bytes(name.as_bytes()).unwrap_or_else(|_| {
                        warn!("Invalid header name: {}", name);
                        header::HeaderName::from_static("x-invalid-header")
                    }),
                    header::HeaderValue::from_str(value).unwrap_or_else(|_| {
                        warn!("Invalid header value for {}: {}", name, value);
                        header::HeaderValue::from_static("")
                    })
                );
                debug!("Added header: {} = {}", name, value);
            }
        }
    }
    
    /// Transform the query parameters according to the configuration
    fn transform_query_params(&self, req: &mut Request<Body>) -> Result<()> {
        let uri = req.uri();
        let mut query_parts = HashMap::new();
        
        // Parse existing query parameters
        if let Some(query) = uri.query() {
            for pair in query.split('&') {
                let mut parts = pair.splitn(2, '=');
                if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                    query_parts.insert(name.to_string(), value.to_string());
                } else if let Some(name) = parts.next() {
                    query_parts.insert(name.to_string(), String::new());
                }
            }
        }
        
        // Remove query parameters
        for param_name in &self.config.remove_query_params {
            if query_parts.remove(param_name).is_some() {
                debug!("Removed query parameter: {}", param_name);
            }
        }
        
        // Replace query parameters
        for (name, value) in &self.config.replace_query_params {
            if query_parts.contains_key(name) {
                query_parts.insert(name.clone(), value.clone());
                debug!("Replaced query parameter: {} = {}", name, value);
            }
        }
        
        // Add query parameters
        for (name, value) in &self.config.add_query_params {
            if !query_parts.contains_key(name) {
                query_parts.insert(name.clone(), value.clone());
                debug!("Added query parameter: {} = {}", name, value);
            }
        }
        
        // Rebuild the query string
        let query_string = query_parts.iter()
            .map(|(name, value)| {
                if value.is_empty() {
                    name.clone()
                } else {
                    format!("{}={}", name, value)
                }
            })
            .collect::<Vec<String>>()
            .join("&");
        
        // Rebuild the URI with the new query string
        let mut parts = uri.clone().into_parts();
        
        if query_string.is_empty() {
            parts.path_and_query = Some(
                uri.path().parse().map_err(|e| anyhow::anyhow!("Invalid path: {}", e))?
            );
        } else {
            parts.path_and_query = Some(
                format!("{}?{}", uri.path(), query_string)
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Invalid path and query: {}", e))?
            );
        }
        
        let new_uri = Uri::from_parts(parts)
            .map_err(|e| anyhow::anyhow!("Failed to rebuild URI: {}", e))?;
        
        *req.uri_mut() = new_uri;
        
        Ok(())
    }
}

#[async_trait]
impl Plugin for RequestTransformerPlugin {
    fn name(&self) -> &'static str {
        "request_transformer"
    }
    
    async fn before_proxy(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        debug!(
            "Transforming request for proxy: {}",
            ctx.proxy.name.as_deref().unwrap_or("unnamed")
        );
        
        // Transform headers
        self.transform_headers(req);
        
        // Transform query parameters
        if !self.config.add_query_params.is_empty() || 
           !self.config.remove_query_params.is_empty() ||
           !self.config.replace_query_params.is_empty() {
            self.transform_query_params(req)?;
        }
        
        Ok(true)
    }
}
