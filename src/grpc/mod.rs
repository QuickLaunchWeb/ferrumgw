use std::sync::{Arc, Mutex, atomic::AtomicU64};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc};
use tokio_stream::{Stream, StreamExt, wrappers::ReceiverStream};
use futures_util::TryStreamExt;
use anyhow::{Result, anyhow, Context};
use tracing::{info, warn, error, debug};
use tonic::{Request, Response, Status, Streaming};
use tonic::transport::{Server, Channel};
use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};
use chrono::{Utc, DateTime};
use serde::{Serialize, Deserialize};

// This mod includes the generated protobuf/gRPC code
pub mod proto {
    tonic::include_proto!("ferrumgw.config");
}

// Export the ConfigClient module
pub mod config_client;

// Export the conversion functions
pub mod conversions;

// Import the proto types
use proto::*;
use proto::config_service_server::{ConfigService, ConfigServiceServer};

use crate::config::data_model::{Configuration, Proxy, Consumer, PluginConfig};

// Convert our domain model to protobuf model
impl From<&Proxy> for ProtoProxy {
    fn from(proxy: &Proxy) -> Self {
        let auth_mode = match proxy.auth_mode {
            crate::config::data_model::AuthMode::Single => "single",
            crate::config::data_model::AuthMode::Multi => "multi",
        };
        
        let backend_protocol = match proxy.backend_protocol {
            crate::config::data_model::Protocol::Http => "http",
            crate::config::data_model::Protocol::Https => "https",
            crate::config::data_model::Protocol::Ws => "ws",
            crate::config::data_model::Protocol::Wss => "wss",
            crate::config::data_model::Protocol::Grpc => "grpc",
        };
        
        let plugin_config_ids = proxy.plugins.iter()
            .map(|pc| pc.id.clone())
            .collect();
        
        ProtoProxy {
            id: proxy.id.clone(),
            name: proxy.name.clone().unwrap_or_default(),
            listen_path: proxy.listen_path.clone(),
            backend_protocol: backend_protocol.to_string(),
            backend_host: proxy.backend_host.clone(),
            backend_port: proxy.backend_port as u32,
            backend_path: proxy.backend_path.clone().unwrap_or_default(),
            strip_listen_path: proxy.strip_listen_path,
            preserve_host_header: proxy.preserve_host_header,
            backend_connect_timeout_ms: proxy.backend_connect_timeout_ms,
            backend_read_timeout_ms: proxy.backend_read_timeout_ms,
            backend_write_timeout_ms: proxy.backend_write_timeout_ms,
            backend_tls_client_cert_path: proxy.backend_tls_client_cert_path.clone().unwrap_or_default(),
            backend_tls_client_key_path: proxy.backend_tls_client_key_path.clone().unwrap_or_default(),
            backend_tls_verify_server_cert: proxy.backend_tls_verify_server_cert,
            backend_tls_server_ca_cert_path: proxy.backend_tls_server_ca_cert_path.clone().unwrap_or_default(),
            dns_override: proxy.dns_override.clone().unwrap_or_default(),
            dns_cache_ttl_seconds: proxy.dns_cache_ttl_seconds.unwrap_or(0),
            auth_mode: auth_mode.to_string(),
            plugin_config_ids,
            created_at: proxy.created_at.to_rfc3339(),
            updated_at: proxy.updated_at.to_rfc3339(),
        }
    }
}

// Convert protobuf model to our domain model
impl TryFrom<ProtoProxy> for Proxy {
    type Error = anyhow::Error;
    
    fn try_from(proto: ProtoProxy) -> Result<Self> {
        let backend_protocol = match proto.backend_protocol.as_str() {
            "http" => crate::config::data_model::Protocol::Http,
            "https" => crate::config::data_model::Protocol::Https,
            "ws" => crate::config::data_model::Protocol::Ws,
            "wss" => crate::config::data_model::Protocol::Wss,
            "grpc" => crate::config::data_model::Protocol::Grpc,
            _ => return Err(anyhow!("Invalid backend protocol: {}", proto.backend_protocol)),
        };
        
        let auth_mode = match proto.auth_mode.as_str() {
            "single" => crate::config::data_model::AuthMode::Single,
            "multi" => crate::config::data_model::AuthMode::Multi,
            _ => return Err(anyhow!("Invalid auth mode: {}", proto.auth_mode)),
        };
        
        // Parse ISO8601 timestamps
        let created_at = chrono::DateTime::parse_from_rfc3339(&proto.created_at)
            .map_err(|e| anyhow!("Invalid created_at timestamp: {}", e))?
            .with_timezone(&chrono::Utc);
        
        let updated_at = chrono::DateTime::parse_from_rfc3339(&proto.updated_at)
            .map_err(|e| anyhow!("Invalid updated_at timestamp: {}", e))?
            .with_timezone(&chrono::Utc);
        
        let backend_path = if proto.backend_path.is_empty() {
            None
        } else {
            Some(proto.backend_path)
        };
        
        let tls_client_cert_path = if proto.backend_tls_client_cert_path.is_empty() {
            None
        } else {
            Some(proto.backend_tls_client_cert_path)
        };
        
        let tls_client_key_path = if proto.backend_tls_client_key_path.is_empty() {
            None
        } else {
            Some(proto.backend_tls_client_key_path)
        };
        
        let tls_server_ca_cert_path = if proto.backend_tls_server_ca_cert_path.is_empty() {
            None
        } else {
            Some(proto.backend_tls_server_ca_cert_path)
        };
        
        let dns_override = if proto.dns_override.is_empty() {
            None
        } else {
            Some(proto.dns_override)
        };
        
        let dns_cache_ttl_seconds = if proto.dns_cache_ttl_seconds == 0 {
            None
        } else {
            Some(proto.dns_cache_ttl_seconds)
        };
        
        let name = if proto.name.is_empty() {
            None
        } else {
            Some(proto.name)
        };
        
        Ok(Proxy {
            id: proto.id,
            name,
            listen_path: proto.listen_path,
            backend_protocol,
            backend_host: proto.backend_host,
            backend_port: proto.backend_port as u16,
            backend_path,
            strip_listen_path: proto.strip_listen_path,
            preserve_host_header: proto.preserve_host_header,
            backend_connect_timeout_ms: proto.backend_connect_timeout_ms,
            backend_read_timeout_ms: proto.backend_read_timeout_ms,
            backend_write_timeout_ms: proto.backend_write_timeout_ms,
            backend_tls_client_cert_path: tls_client_cert_path,
            backend_tls_client_key_path: tls_client_key_path,
            backend_tls_verify_server_cert: proto.backend_tls_verify_server_cert,
            backend_tls_server_ca_cert_path: tls_server_ca_cert_path,
            dns_override,
            dns_cache_ttl_seconds,
            auth_mode,
            plugins: Vec::new(), // Will be populated separately
            created_at,
            updated_at,
        })
    }
}

// Convert domain model to protobuf model
impl From<&Consumer> for ProtoConsumer {
    fn from(consumer: &Consumer) -> Self {
        let credentials_json = serde_json::to_string(&consumer.credentials)
            .unwrap_or_else(|_| "{}".to_string());
        
        ProtoConsumer {
            id: consumer.id.clone(),
            username: consumer.username.clone(),
            custom_id: consumer.custom_id.clone().unwrap_or_default(),
            credentials_json,
            created_at: consumer.created_at.to_rfc3339(),
            updated_at: consumer.updated_at.to_rfc3339(),
        }
    }
}

// Convert protobuf model to domain model
impl TryFrom<ProtoConsumer> for Consumer {
    type Error = anyhow::Error;
    
    fn try_from(proto: ProtoConsumer) -> Result<Self> {
        // Parse ISO8601 timestamps
        let created_at = chrono::DateTime::parse_from_rfc3339(&proto.created_at)
            .map_err(|e| anyhow!("Invalid created_at timestamp: {}", e))?
            .with_timezone(&chrono::Utc);
        
        let updated_at = chrono::DateTime::parse_from_rfc3339(&proto.updated_at)
            .map_err(|e| anyhow!("Invalid updated_at timestamp: {}", e))?
            .with_timezone(&chrono::Utc);
        
        let credentials = if proto.credentials_json.is_empty() {
            std::collections::HashMap::new()
        } else {
            serde_json::from_str(&proto.credentials_json)
                .map_err(|e| anyhow!("Invalid credentials JSON: {}", e))?
        };
        
        let custom_id = if proto.custom_id.is_empty() {
            None
        } else {
            Some(proto.custom_id)
        };
        
        Ok(Consumer {
            id: proto.id,
            username: proto.username,
            custom_id,
            credentials,
            created_at,
            updated_at,
        })
    }
}

// Convert domain model to protobuf model
impl From<&PluginConfig> for ProtoPluginConfig {
    fn from(plugin_config: &PluginConfig) -> Self {
        let config_json = serde_json::to_string(&plugin_config.config)
            .unwrap_or_else(|_| "{}".to_string());
        
        ProtoPluginConfig {
            id: plugin_config.id.clone(),
            plugin_name: plugin_config.plugin_name.clone(),
            config_json,
            scope: plugin_config.scope.clone(),
            proxy_id: plugin_config.proxy_id.clone().unwrap_or_default(),
            consumer_id: plugin_config.consumer_id.clone().unwrap_or_default(),
            enabled: plugin_config.enabled,
            created_at: plugin_config.created_at.to_rfc3339(),
            updated_at: plugin_config.updated_at.to_rfc3339(),
        }
    }
}

// Convert protobuf model to domain model
impl TryFrom<ProtoPluginConfig> for PluginConfig {
    type Error = anyhow::Error;
    
    fn try_from(proto: ProtoPluginConfig) -> Result<Self> {
        // Parse ISO8601 timestamps
        let created_at = chrono::DateTime::parse_from_rfc3339(&proto.created_at)
            .map_err(|e| anyhow!("Invalid created_at timestamp: {}", e))?
            .with_timezone(&chrono::Utc);
        
        let updated_at = chrono::DateTime::parse_from_rfc3339(&proto.updated_at)
            .map_err(|e| anyhow!("Invalid updated_at timestamp: {}", e))?
            .with_timezone(&chrono::Utc);
        
        let config = if proto.config_json.is_empty() {
            serde_json::Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&proto.config_json)
                .map_err(|e| anyhow!("Invalid plugin config JSON: {}", e))?
        };
        
        let proxy_id = if proto.proxy_id.is_empty() {
            None
        } else {
            Some(proto.proxy_id)
        };
        
        let consumer_id = if proto.consumer_id.is_empty() {
            None
        } else {
            Some(proto.consumer_id)
        };
        
        Ok(PluginConfig {
            id: proto.id,
            plugin_name: proto.plugin_name,
            config,
            scope: proto.scope,
            proxy_id,
            consumer_id,
            enabled: proto.enabled,
            created_at,
            updated_at,
        })
    }
}

// Convert Configuration to ConfigSnapshot
impl From<&Configuration> for ConfigSnapshot {
    fn from(config: &Configuration) -> Self {
        let proxies = config.proxies.iter()
            .map(ProtoProxy::from)
            .collect();
        
        let consumers = config.consumers.iter()
            .map(ProtoConsumer::from)
            .collect();
        
        let plugin_configs = config.plugin_configs.iter()
            .map(ProtoPluginConfig::from)
            .collect();
        
        ConfigSnapshot {
            proxies,
            consumers,
            plugin_configs,
            version: 0, // Will be set by the caller
            created_at: config.last_updated_at.to_rfc3339(),
        }
    }
}

// Control Plane implementation
pub struct ConfigServiceImpl {
    // Shared configuration store
    config_store: Arc<tokio::sync::RwLock<Configuration>>,
    // Current configuration version
    version: Arc<std::sync::atomic::AtomicU64>,
    // Active DP subscribers mapped to their channels
    subscribers: Arc<tokio::sync::RwLock<std::collections::HashMap<String, tokio::sync::mpsc::Sender<Result<ConfigUpdate, Status>>>>>,
}

impl ConfigServiceImpl {
    pub fn new(config_store: Arc<tokio::sync::RwLock<Configuration>>) -> Self {
        Self {
            config_store,
            version: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            subscribers: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
    
    pub fn new_server(config_store: Arc<tokio::sync::RwLock<Configuration>>) -> ConfigServiceServer<Self> {
        ConfigServiceServer::new(Self::new(config_store))
    }
    
    // Get current config version
    pub fn get_current_version(&self) -> u64 {
        self.version.load(std::sync::atomic::Ordering::SeqCst)
    }
    
    // Increment and get next config version
    pub fn next_version(&self) -> u64 {
        self.version.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }
    
    // Push a configuration update to all subscribers
    pub async fn push_config_update(&self, update: ConfigUpdate) -> Result<()> {
        let mut subscribers = self.subscribers.write().await;
        let mut to_remove = Vec::new();
        
        for (node_id, tx) in subscribers.iter() {
            match tx.send(Ok(update.clone())).await {
                Ok(_) => {
                    debug!("Sent config update to node: {}", node_id);
                }
                Err(e) => {
                    warn!("Failed to send config update to node {}: {}", node_id, e);
                    to_remove.push(node_id.clone());
                }
            }
        }
        
        // Clean up disconnected subscribers
        for node_id in to_remove {
            subscribers.remove(&node_id);
            info!("Removed disconnected node from subscribers: {}", node_id);
        }
        
        Ok(())
    }
    
    // Push a full configuration update to all subscribers
    pub async fn push_full_config(&self) -> Result<()> {
        let config = self.config_store.read().await;
        let version = self.get_current_version();
        
        let mut snapshot = ConfigSnapshot::from(&*config);
        snapshot.version = version;
        
        let update = ConfigUpdate {
            update_type: UpdateType::Full as i32,
            version,
            updated_at: chrono::Utc::now().to_rfc3339(),
            update: Some(proto::config_update::Update::FullSnapshot(snapshot)),
        };
        
        self.push_config_update(update).await
    }
}

#[tonic::async_trait]
impl ConfigService for ConfigServiceImpl {
    type SubscribeConfigUpdatesStream = tokio_stream::wrappers::ReceiverStream<Result<ConfigUpdate, Status>>;
    
    async fn subscribe_config_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeConfigUpdatesStream>, Status> {
        let req = request.into_inner();
        let node_id = req.node_id;
        
        info!("Data Plane node {} subscribing to config updates", node_id);
        
        // Create channel for streaming updates to this subscriber
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        
        // Register the subscriber
        self.subscribers.write().await.insert(node_id.clone(), tx.clone());
        
        // Send initial configuration based on client's current version
        let config = self.config_store.read().await;
        let current_version = self.get_current_version();
        
        // If client has older or no config, send full snapshot
        if req.current_version < current_version {
            let mut snapshot = ConfigSnapshot::from(&*config);
            snapshot.version = current_version;
            
            let update = ConfigUpdate {
                update_type: UpdateType::Full as i32,
                version: current_version,
                updated_at: chrono::Utc::now().to_rfc3339(),
                update: Some(proto::config_update::Update::FullSnapshot(snapshot)),
            };
            
            // Send initial config to the new subscriber
            if let Err(e) = tx.send(Ok(update)).await {
                error!("Failed to send initial config to node {}: {}", node_id, e);
                return Err(Status::internal("Failed to send initial configuration"));
            }
        }
        
        Ok(Response::new(stream))
    }
    
    async fn get_config_snapshot(
        &self,
        request: Request<SnapshotRequest>,
    ) -> Result<Response<ConfigSnapshot>, Status> {
        let req = request.into_inner();
        let node_id = req.node_id;
        
        info!("Data Plane node {} requesting config snapshot", node_id);
        
        let config = self.config_store.read().await;
        let current_version = self.get_current_version();
        
        let mut snapshot = ConfigSnapshot::from(&*config);
        snapshot.version = current_version;
        
        Ok(Response::new(snapshot))
    }
    
    async fn report_health(
        &self,
        request: Request<HealthReport>,
    ) -> Result<Response<HealthAck>, Status> {
        let report = request.into_inner();
        let node_id = report.node_id;
        
        debug!("Received health report from node {}: status={}", node_id, report.status);
        
        // Store health report or process metrics if needed
        
        let ack = HealthAck {
            success: true,
            message: "Health report received".to_string(),
        };
        
        Ok(Response::new(ack))
    }
}

// Data Plane gRPC client
pub struct DataPlaneClient {
    // gRPC client
    client: proto::config_service_client::ConfigServiceClient<tonic::transport::Channel>,
    // Node ID
    node_id: String,
    // Current config version
    current_version: std::sync::atomic::AtomicU64,
}

impl DataPlaneClient {
    pub async fn new(grpc_url: &str, node_id: String) -> Result<Self> {
        let client = proto::config_service_client::ConfigServiceClient::connect(grpc_url.to_string())
            .await
            .map_err(|e| anyhow!("Failed to connect to Control Plane at {}: {}", grpc_url, e))?;
        
        Ok(Self {
            client,
            node_id,
            current_version: std::sync::atomic::AtomicU64::new(0),
        })
    }
    
    // Subscribe to configuration updates from the Control Plane
    pub async fn subscribe(&mut self) -> Result<impl tokio_stream::Stream<Item = Result<ConfigUpdate, Status>>> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        
        // Create a streaming request
        let request = tonic::Request::new(SubscribeRequest {
            node_id: self.node_id.clone(),
            current_version: self.current_version.load(std::sync::atomic::Ordering::SeqCst),
        });
        
        // Call the gRPC service subscribe method
        let response = self.client.subscribe_config_updates(request).await?;
        
        // Get the stream from the response
        let mut stream = response.into_inner();
        
        // Spawn a task to handle the stream
        tokio::spawn(async move {
            while let Ok(Some(update)) = stream.try_next().await {
                if tx.send(Ok(update)).await.is_err() {
                    break;
                }
            }
        });
        
        // Return the receiver as a Stream
        Ok(tokio_stream::wrappers::ReceiverStream::new(rx))
    }
    
    // Get a full configuration snapshot from the Control Plane
    pub async fn get_snapshot(&mut self) -> Result<ConfigSnapshot> {
        let request = SnapshotRequest {
            node_id: self.node_id.clone(),
        };
        
        let response = self.client.get_config_snapshot(request).await
            .map_err(|e| anyhow!("Failed to get config snapshot from Control Plane: {}", e))?;
        
        let snapshot = response.into_inner();
        
        // Update current version
        self.current_version.store(snapshot.version, std::sync::atomic::Ordering::SeqCst);
        
        Ok(snapshot)
    }
    
    // Send a health report to the Control Plane
    pub async fn report_health(&mut self, status: &str, metrics: std::collections::HashMap<String, String>) -> Result<()> {
        let current_version = self.current_version.load(std::sync::atomic::Ordering::SeqCst);
        
        let report = HealthReport {
            node_id: self.node_id.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            config_version: current_version,
            metrics,
            status: status.to_string(),
        };
        
        self.client.report_health(report).await
            .map_err(|e| anyhow!("Failed to send health report to Control Plane: {}", e))?;
        
        Ok(())
    }
    
    // Convert a ConfigSnapshot to our domain Configuration
    pub async fn snapshot_to_configuration(snapshot: ConfigSnapshot) -> Result<Configuration> {
        let mut proxies = Vec::new();
        let mut consumers = Vec::new();
        let mut plugin_configs = Vec::new();
        
        // Convert consumers first since proxies and plugin configs might reference them
        for proto_consumer in snapshot.consumers {
            match Consumer::try_from(proto_consumer) {
                Ok(consumer) => consumers.push(consumer),
                Err(e) => warn!("Failed to convert consumer: {}", e),
            }
        }
        
        // Convert plugin configs next
        for proto_plugin_config in snapshot.plugin_configs {
            match PluginConfig::try_from(proto_plugin_config) {
                Ok(plugin_config) => plugin_configs.push(plugin_config),
                Err(e) => warn!("Failed to convert plugin config: {}", e),
            }
        }
        
        // Convert proxies and link to plugin configs
        for proto_proxy in snapshot.proxies {
            match Proxy::try_from(proto_proxy.clone()) {
                Ok(mut proxy) => {
                    // Link plugins to proxy based on IDs
                    for plugin_id in proto_proxy.plugin_config_ids {
                        if let Some(plugin_config) = plugin_configs.iter().find(|pc| pc.id == plugin_id) {
                            proxy.plugins.push(plugin_config.clone());
                        }
                    }
                    proxies.push(proxy);
                },
                Err(e) => warn!("Failed to convert proxy: {}", e),
            }
        }
        
        // Parse creation timestamp
        let last_updated_at = chrono::DateTime::parse_from_rfc3339(&snapshot.created_at)
            .map_err(|e| anyhow!("Invalid created_at timestamp: {}", e))?
            .with_timezone(&chrono::Utc);
        
        Ok(Configuration {
            proxies,
            consumers,
            plugin_configs,
            last_updated_at,
        })
    }
}
