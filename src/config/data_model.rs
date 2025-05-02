use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseType {
    Postgres,
    MySQL,
    SQLite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendProtocol {
    Http,
    Https,
    Ws,
    Wss,
    Grpc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthMode {
    #[serde(rename = "single")]
    Single,
    #[serde(rename = "multi")]
    Multi,
}

impl Default for AuthMode {
    fn default() -> Self {
        AuthMode::Single
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginScope {
    #[serde(rename = "global")]
    Global,
    #[serde(rename = "proxy")]
    Proxy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proxy {
    pub id: String,
    pub name: Option<String>,
    pub listen_path: String,
    pub backend_protocol: BackendProtocol,
    pub backend_host: String,
    pub backend_port: u16,
    pub backend_path: Option<String>,
    
    #[serde(default = "default_true")]
    pub strip_listen_path: bool,
    
    #[serde(default = "default_false")]
    pub preserve_host_header: bool,
    
    pub backend_connect_timeout_ms: u64,
    pub backend_read_timeout_ms: u64,
    pub backend_write_timeout_ms: u64,
    
    pub backend_tls_client_cert_path: Option<String>,
    pub backend_tls_client_key_path: Option<String>,
    
    #[serde(default = "default_true")]
    pub backend_tls_verify_server_cert: bool,
    
    pub backend_tls_server_ca_cert_path: Option<String>,
    pub dns_override: Option<String>,
    pub dns_cache_ttl_seconds: Option<u64>,
    
    #[serde(default)]
    pub auth_mode: AuthMode,
    
    #[serde(default)]
    pub plugins: Vec<PluginAssociation>,
    
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAssociation {
    pub plugin_config_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedded_config: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consumer {
    pub id: String,
    pub username: String,
    pub custom_id: Option<String>,
    pub credentials: HashMap<String, Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub id: String,
    pub plugin_name: String,
    pub config: Value,
    pub scope: PluginScope,
    pub proxy_id: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Configuration {
    pub proxies: Vec<Proxy>,
    pub consumers: Vec<Consumer>,
    pub plugin_configs: Vec<PluginConfig>,
    pub last_updated_at: DateTime<Utc>,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            proxies: Vec::new(),
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(), // Initialize with current time
        }
    }
}

/// Represents incremental changes to the configuration since a specific timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationDelta {
    /// New or updated proxies
    pub updated_proxies: Vec<Proxy>,
    /// IDs of proxies that were deleted
    pub deleted_proxy_ids: Vec<String>,
    
    /// New or updated consumers
    pub updated_consumers: Vec<Consumer>,
    /// IDs of consumers that were deleted
    pub deleted_consumer_ids: Vec<String>,
    
    /// New or updated plugin configurations
    pub updated_plugin_configs: Vec<PluginConfig>,
    /// IDs of plugin configurations that were deleted
    pub deleted_plugin_config_ids: Vec<String>,
    
    /// The timestamp of the latest change in this delta
    pub last_updated_at: DateTime<Utc>,
}

impl ConfigurationDelta {
    /// Apply this delta to the given configuration
    pub fn apply_to(&self, config: &mut Configuration) {
        // Apply proxy changes
        for proxy in &self.updated_proxies {
            if let Some(existing) = config.proxies.iter_mut().find(|p| p.id == proxy.id) {
                *existing = proxy.clone();
            } else {
                config.proxies.push(proxy.clone());
            }
        }
        config.proxies.retain(|p| !self.deleted_proxy_ids.contains(&p.id));
        
        // Apply consumer changes
        for consumer in &self.updated_consumers {
            if let Some(existing) = config.consumers.iter_mut().find(|c| c.id == consumer.id) {
                *existing = consumer.clone();
            } else {
                config.consumers.push(consumer.clone());
            }
        }
        config.consumers.retain(|c| !self.deleted_consumer_ids.contains(&c.id));
        
        // Apply plugin config changes
        for plugin_config in &self.updated_plugin_configs {
            if let Some(existing) = config.plugin_configs.iter_mut().find(|p| p.id == plugin_config.id) {
                *existing = plugin_config.clone();
            } else {
                config.plugin_configs.push(plugin_config.clone());
            }
        }
        config.plugin_configs.retain(|p| !self.deleted_plugin_config_ids.contains(&p.id));
        
        // Update the last_updated_at timestamp
        if self.last_updated_at > config.last_updated_at {
            config.last_updated_at = self.last_updated_at;
        }
    }
    
    /// Check if this delta is empty (no changes)
    pub fn is_empty(&self) -> bool {
        self.updated_proxies.is_empty() &&
        self.deleted_proxy_ids.is_empty() &&
        self.updated_consumers.is_empty() &&
        self.deleted_consumer_ids.is_empty() &&
        self.updated_plugin_configs.is_empty() &&
        self.deleted_plugin_config_ids.is_empty()
    }
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}
