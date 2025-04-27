// Add this at the top of the file
pub mod proto;
pub mod conversions;

use proto::*;
use conversions::ConfigSnapshot;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status};
use std::sync::{Arc, Mutex, atomic::AtomicU64};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::pin::Pin;
use tracing::{info, debug, warn, error};
use chrono::{Utc, DateTime};
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow, Context};

// Export the ConfigClient module
pub mod config_client;

// Import the proto types
use proto::config_service_server::{ConfigService, ConfigServiceServer};

use crate::config::data_model::{Configuration, Proxy, Consumer, PluginConfig};
use crate::config::cache::ConfigCache;

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
        
        // Create a snapshot of the current configuration
        let mut snapshot = proto::ConfigSnapshot::from(&*config as &Configuration);
        
        // Set the new version
        let version = self.next_version();
        snapshot.version = version;
        
        let update = ConfigUpdate {
            update_type: UpdateType::Full as i32,
            update: Some(config_update::Update::FullSnapshot(snapshot)),
            version,
            updated_at: Utc::now().to_rfc3339(),
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
            let mut snapshot = proto::ConfigSnapshot::from(&*config as &Configuration);
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
        info!("Received snapshot request from node: {}", req.node_id);
        
        // Get current configuration
        let config = self.config_store.read().await;
        
        // Create a snapshot
        let mut snapshot = proto::ConfigSnapshot::from(&*config as &Configuration);
        snapshot.version = self.get_current_version();
        
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
            match Consumer::try_from(&proto_consumer) {
                Ok(consumer) => consumers.push(consumer),
                Err(e) => warn!("Failed to convert consumer: {}", e),
            }
        }
        
        // Convert plugin configs next
        for proto_plugin_config in snapshot.plugin_configs {
            match PluginConfig::try_from(&proto_plugin_config) {
                Ok(plugin_config) => plugin_configs.push(plugin_config),
                Err(e) => warn!("Failed to convert plugin config: {}", e),
            }
        }
        
        // Convert proxies and link to plugin configs
        for proto_proxy in snapshot.proxies {
            match Proxy::try_from(&proto_proxy) {
                Ok(mut proxy) => {
                    // Link plugins to proxy based on IDs
                    for plugin_id in &proto_proxy.plugin_config_ids {
                        if let Some(plugin_config) = plugin_configs.iter().find(|pc| pc.id == *plugin_id) {
                            // Need to convert to PluginAssociation, not directly use PluginConfig
                            proxy.plugins.push(crate::config::data_model::PluginAssociation {
                                plugin_config_id: plugin_id.clone(),
                                embedded_config: None,
                            });
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
