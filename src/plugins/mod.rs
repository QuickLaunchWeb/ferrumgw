use std::collections::HashMap;
use std::sync::Arc;
use anyhow::{Result, Context};
use async_trait::async_trait;
use hyper::{Body, Request, Response};
use tracing::{debug, info, error, warn};

use crate::proxy::handler::RequestContext;

// Import plugin implementations
mod stdout_logging;
mod http_logging;
mod transaction_debugger;
mod oauth2_auth;
mod jwt_auth;
mod key_auth;
mod basic_auth;
mod access_control;
mod request_transformer;
mod response_transformer;
mod rate_limiting;

/// A trait that defines the interface for all plugins
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Returns the name of the plugin
    fn name(&self) -> &'static str;
    
    /// Called when a request is first received, before any other processing
    /// Return Ok(true) to continue processing, Ok(false) to stop
    async fn on_request_received(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        Ok(true) // Default is to continue
    }
    
    /// Called during the authentication phase
    /// Return Ok(true) to continue processing, Ok(false) to stop
    async fn authenticate(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        Ok(true) // Default is to continue
    }
    
    /// Called during the authorization phase (after authentication)
    /// Return Ok(true) to continue processing, Ok(false) to stop
    async fn authorize(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        Ok(true) // Default is to continue
    }
    
    /// Called just before the request is proxied to the backend
    /// Return Ok(true) to continue processing, Ok(false) to stop
    async fn before_proxy(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        Ok(true) // Default is to continue
    }
    
    /// Called after receiving the response from the backend
    async fn after_proxy(&self, resp: &mut Response<Body>, ctx: &mut RequestContext) -> Result<()> {
        Ok(()) // Default is to do nothing
    }
    
    /// Called to log the transaction (after the response is sent)
    async fn log(&self, req: &Request<Body>, resp: &Response<Body>, ctx: &RequestContext) -> Result<()> {
        Ok(()) // Default is to do nothing
    }
}

/// Registry of available plugin factories
pub struct PluginRegistry {
    factories: HashMap<String, Box<dyn Fn(serde_json::Value) -> Result<Box<dyn Plugin>> + Send + Sync>>,
}

impl PluginRegistry {
    /// Creates a new plugin registry with all built-in plugins registered
    pub fn new() -> Self {
        let mut factories = HashMap::new();
        
        // Register all standard plugins
        factories.insert(
            "stdout_logging".to_string(),
            Box::new(|config| Ok(Box::new(stdout_logging::StdoutLoggingPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "http_logging".to_string(),
            Box::new(|config| Ok(Box::new(http_logging::HttpLoggingPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "transaction_debugger".to_string(),
            Box::new(|config| Ok(Box::new(transaction_debugger::TransactionDebuggerPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "jwt_auth".to_string(),
            Box::new(|config| Ok(Box::new(jwt_auth::JwtAuthPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "key_auth".to_string(),
            Box::new(|config| Ok(Box::new(key_auth::KeyAuthPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "basic_auth".to_string(),
            Box::new(|config| Ok(Box::new(basic_auth::BasicAuthPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "oauth2_auth".to_string(),
            Box::new(|config| Ok(Box::new(oauth2_auth::OAuth2Plugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "access_control".to_string(),
            Box::new(|config| Ok(Box::new(access_control::AccessControlPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "request_transformer".to_string(),
            Box::new(|config| Ok(Box::new(request_transformer::RequestTransformerPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "response_transformer".to_string(),
            Box::new(|config| Ok(Box::new(response_transformer::ResponseTransformerPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        factories.insert(
            "rate_limiting".to_string(),
            Box::new(|config| Ok(Box::new(rate_limiting::RateLimitingPlugin::new(config)?) as Box<dyn Plugin>))
        );
        
        Self { factories }
    }
    
    /// Creates a plugin instance from a plugin name and configuration
    pub fn create_plugin(&self, name: &str, config: serde_json::Value) -> Result<Box<dyn Plugin>> {
        match self.factories.get(name) {
            Some(factory) => factory(config).context(format!("Failed to create plugin: {}", name)),
            None => Err(anyhow::anyhow!("Unknown plugin: {}", name)),
        }
    }
    
    /// Returns a list of all available plugin names
    pub fn available_plugins(&self) -> Vec<String> {
        self.factories.keys().cloned().collect()
    }
}

/// Manager for plugin instances and execution
pub struct PluginManager {
    registry: PluginRegistry,
    // In a complete implementation, this would manage plugin instances
    // and organize them into execution pipelines
}

impl PluginManager {
    /// Creates a new plugin manager
    pub fn new() -> Self {
        Self {
            registry: PluginRegistry::new(),
        }
    }
    
    /// Lists all available plugin types
    pub fn available_plugins(&self) -> Vec<String> {
        self.registry.available_plugins()
    }
    
    /// Runs the pre-proxy plugin pipeline on a request
    /// Returns the (possibly modified) request and a boolean indicating whether to continue
    pub async fn run_pre_proxy_plugins(
        &self,
        mut req: Request<Body>,
        ctx: &mut RequestContext,
    ) -> Result<(Request<Body>, bool)> {
        // In a complete implementation, this would:
        // 1. Determine which plugins are active for this proxy
        // 2. Run them in the correct order with appropriate hooks
        
        // For now, we'll just return the request unchanged
        debug!("Pre-proxy plugins would run here (actual implementation pending)");
        Ok((req, true))
    }
    
    /// Runs the post-proxy plugin pipeline on a response
    /// Returns the (possibly modified) response
    pub async fn run_post_proxy_plugins(
        &self,
        mut resp: Response<Body>,
        ctx: &mut RequestContext,
    ) -> Result<Response<Body>> {
        // In a complete implementation, this would:
        // 1. Determine which plugins are active for this proxy
        // 2. Run them in the correct order with appropriate hooks
        
        // For now, we'll just return the response unchanged
        debug!("Post-proxy plugins would run here (actual implementation pending)");
        Ok(resp)
    }
}
