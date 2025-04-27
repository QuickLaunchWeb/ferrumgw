use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use anyhow::{Result, Context};
use async_trait::async_trait;
use hyper::{Body, Request, Response};
use tracing::{debug, info, error, warn};
use tokio::spawn;
use tokio::time::sleep;
use tokio::select;

use crate::proxy::handler::RequestContext;
use crate::config::data_model::{PluginConfig, Proxy, Configuration};

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
    // Cached plugins for better performance
    global_plugins: Arc<RwLock<Vec<Box<dyn Plugin>>>>,
    // Shared configuration for looking up plugin configs
    shared_config: Arc<RwLock<Configuration>>,
}

impl PluginManager {
    /// Creates a new plugin manager
    pub fn new(shared_config: Arc<RwLock<Configuration>>) -> Self {
        Self {
            registry: PluginRegistry::new(),
            global_plugins: Arc::new(RwLock::new(Vec::new())),
            shared_config,
        }
    }
    
    /// Lists all available plugin types
    pub fn available_plugins(&self) -> Vec<String> {
        self.registry.available_plugins()
    }
    
    /// Create a plugin instance from configuration
    pub fn create_plugin(&self, plugin_name: &str, config: serde_json::Value) -> Result<Box<dyn Plugin>> {
        self.registry.create_plugin(plugin_name, config)
    }
    
    /// Runs the pre-proxy plugin pipeline on a request
    /// Returns the (possibly modified) request and a boolean indicating whether to continue
    pub async fn run_pre_proxy_plugins(
        &self,
        mut req: Request<Body>,
        ctx: &mut RequestContext,
    ) -> Result<(Request<Body>, bool)> {
        let proxy = &ctx.proxy;
        
        // Get all relevant plugins for this proxy
        let active_plugins = self.get_active_plugins_for_proxy(proxy).await?;
        
        // Execute on_request_received phase
        debug!("Executing on_request_received phase for {} plugins", active_plugins.len());
        for plugin in &active_plugins {
            match plugin.on_request_received(&mut req, ctx).await {
                Ok(true) => continue, // Continue to next plugin
                Ok(false) => {
                    debug!("Plugin {} rejected request in on_request_received phase", plugin.name());
                    return Ok((req, false)); // Stop processing
                },
                Err(e) => {
                    error!("Error in plugin {} during on_request_received: {}", plugin.name(), e);
                    return Err(e);
                }
            }
        }
        
        // Execute authenticate phase
        debug!("Executing authenticate phase for {} plugins", active_plugins.len());
        for plugin in &active_plugins {
            match plugin.authenticate(&mut req, ctx).await {
                Ok(true) => continue, // Continue to next plugin
                Ok(false) => {
                    debug!("Plugin {} rejected request in authenticate phase", plugin.name());
                    return Ok((req, false)); // Stop processing
                },
                Err(e) => {
                    error!("Error in plugin {} during authenticate: {}", plugin.name(), e);
                    return Err(e);
                }
            }
        }
        
        // Execute authorize phase
        debug!("Executing authorize phase for {} plugins", active_plugins.len());
        for plugin in &active_plugins {
            match plugin.authorize(&mut req, ctx).await {
                Ok(true) => continue, // Continue to next plugin
                Ok(false) => {
                    debug!("Plugin {} rejected request in authorize phase", plugin.name());
                    return Ok((req, false)); // Stop processing
                },
                Err(e) => {
                    error!("Error in plugin {} during authorize: {}", plugin.name(), e);
                    return Err(e);
                }
            }
        }
        
        // Execute before_proxy phase
        debug!("Executing before_proxy phase for {} plugins", active_plugins.len());
        for plugin in &active_plugins {
            match plugin.before_proxy(&mut req, ctx).await {
                Ok(true) => continue, // Continue to next plugin
                Ok(false) => {
                    debug!("Plugin {} rejected request in before_proxy phase", plugin.name());
                    return Ok((req, false)); // Stop processing
                },
                Err(e) => {
                    error!("Error in plugin {} during before_proxy: {}", plugin.name(), e);
                    return Err(e);
                }
            }
        }
        
        // All plugins executed successfully
        Ok((req, true))
    }
    
    /// Runs the post-proxy plugin pipeline on a response
    /// Returns the (possibly modified) response
    pub async fn run_post_proxy_plugins(
        &self,
        mut resp: Response<Body>,
        ctx: &mut RequestContext,
    ) -> Result<Response<Body>> {
        let proxy = &ctx.proxy;
        
        // Get all relevant plugins for this proxy
        let active_plugins = self.get_active_plugins_for_proxy(proxy).await?;
        
        // Execute after_proxy phase
        debug!("Executing after_proxy phase for {} plugins", active_plugins.len());
        for plugin in &active_plugins {
            match plugin.after_proxy(&mut resp, ctx).await {
                Ok(()) => continue, // Continue to next plugin
                Err(e) => {
                    error!("Error in plugin {} during after_proxy: {}", plugin.name(), e);
                    return Err(e);
                }
            }
        }
        
        // All plugins executed successfully
        Ok(resp)
    }
    
    /// Execute the logging phase for all plugins
    pub async fn run_log_plugins(
        &self,
        req: &Request<Body>,
        resp: &Response<Body>,
        ctx: &RequestContext,
    ) -> Result<()> {
        let proxy = &ctx.proxy;
        
        // Get all relevant plugins for this proxy
        let active_plugins = self.get_active_plugins_for_proxy(proxy).await?;
        
        // Execute log phase (non-blocking, parallel execution)
        debug!("Executing log phase for {} plugins", active_plugins.len());
        let mut log_tasks = Vec::new();
        
        for plugin in active_plugins {
            let p = plugin;
            let req = req.clone();
            let resp = resp.clone();
            let ctx = ctx.clone();
            
            let task = spawn(async move {
                if let Err(e) = p.log(&req, &resp, &ctx).await {
                    warn!("Error in plugin {} during log phase: {}", p.name(), e);
                }
            });
            
            log_tasks.push(task);
        }
        
        // Wait for all logging tasks to complete or time out
        for task in log_tasks {
            select! {
                _ = task => {},
                _ = sleep(std::time::Duration::from_secs(5)) => {
                    warn!("Logging task timed out after 5 seconds");
                }
            }
        }
        
        Ok(())
    }
    
    /// Get all active plugins for a proxy
    async fn get_active_plugins_for_proxy(&self, proxy: &Proxy) -> Result<Vec<Box<dyn Plugin>>> {
        // Get shared configuration
        let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();
        
        // Get global plugins from cache
        {
            let global_plugins = self.global_plugins.read().await;
            for plugin in global_plugins.iter() {
                plugins.push(self.registry.create_plugin(plugin.name(), serde_json::json!({}))?);
            }
        }
        
        // Process proxy-specific plugins
        for plugin_association in &proxy.plugins {
            if let Some(config) = &plugin_association.embedded_config {
                // If config is embedded in proxy config, use it directly
                if let Ok(config_copy) = serde_json::to_value(config) {
                    if let Ok(plugin_config) = self.get_plugin_config_by_id(&plugin_association.plugin_config_id).await {
                        // Create the plugin with its configuration
                        if let Ok(plugin) = self.registry.create_plugin(&plugin_config.plugin_name, config_copy) {
                            plugins.push(plugin);
                        }
                    }
                }
            } else {
                // Otherwise, look up the plugin config by ID
                if let Ok(plugin_config) = self.get_plugin_config_by_id(&plugin_association.plugin_config_id).await {
                    // Create the plugin with its configuration
                    if let Ok(plugin) = self.registry.create_plugin(&plugin_config.plugin_name, plugin_config.config.clone()) {
                        plugins.push(plugin);
                    }
                }
            }
        }
        
        Ok(plugins)
    }
    
    /// Get plugin config by ID from the shared configuration
    async fn get_plugin_config_by_id(&self, id: &str) -> Result<PluginConfig> {
        // First check in-memory configuration
        {
            let config = self.shared_config.read().await;
            if let Some(plugin_config) = config.plugin_configs.iter().find(|pc| pc.id == id) {
                return Ok(plugin_config.clone());
            }
        }
        
        // If not found in memory, check database if available
        if let Ok(db) = crate::db::get_connection() {
            match crate::db::plugin_configs::get_plugin_config_by_id(&db, id).await {
                Ok(Some(plugin_config)) => return Ok(plugin_config),
                Ok(None) => return Err(anyhow::anyhow!("Plugin config not found: {}", id)),
                Err(e) => return Err(anyhow::anyhow!("Database error looking up plugin config: {}", e)),
            }
        }
        
        // Not found
        Err(anyhow::anyhow!("Plugin config not found: {}", id))
    }
}
