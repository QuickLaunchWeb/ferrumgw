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

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}
