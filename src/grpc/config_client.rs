use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tonic::{Request, Status, transport::Channel};
use tracing::{info, warn, error, debug};

use crate::config::data_model::Configuration;
use super::proto::{
    config_service_client::ConfigServiceClient,
    SubscribeRequest, ConfigUpdate, GetConfigSnapshotRequest,
};

/// Client for the Control Plane gRPC service
pub struct ConfigClient {
    /// The gRPC client for the ConfigService
    client: ConfigServiceClient<Channel>,
    /// Unique ID for this data plane node
    node_id: String,
    /// Authentication token for the Control Plane
    auth_token: String,
    /// Current configuration version
    config_version: Arc<AtomicU64>,
}

impl ConfigClient {
    /// Connect to the Control Plane gRPC service
    pub async fn connect(cp_url: &str, auth_token: String) -> Result<Self> {
        // Generate a unique node ID if not provided
        let node_id = format!("dp-{}", uuid::Uuid::new_v4());
        
        // Connect to the gRPC service
        let channel = tonic::transport::Channel::from_shared(cp_url.to_string())?
            .connect()
            .await
            .map_err(|e| anyhow!("Failed to connect to Control Plane at {}: {}", cp_url, e))?;
        
        let client = ConfigServiceClient::new(channel);
        
        Ok(Self {
            client,
            node_id,
            auth_token,
            config_version: Arc::new(AtomicU64::new(0)),
        })
    }
    
    /// Subscribe to configuration updates from the Control Plane
    pub async fn subscribe(&mut self) -> Result<impl Stream<Item = Result<ConfigUpdate, Status>>> {
        let (tx, rx) = mpsc::channel(100);
        
        // Create the subscribe request with authentication
        let mut request = Request::new(SubscribeRequest {
            node_id: self.node_id.clone(),
            current_version: self.config_version.load(Ordering::SeqCst),
        });
        
        // Add authentication token as metadata
        request.metadata_mut().insert(
            "authorization", 
            format!("Bearer {}", self.auth_token).parse().unwrap()
        );
        
        // Call the gRPC service
        let response = self.client.subscribe_config_updates(request)
            .await
            .map_err(|e| anyhow!("Failed to subscribe to configuration updates: {}", e))?;
        
        // Get the stream from the response
        let mut stream = response.into_inner();
        
        // Create a clone of the config version for the async task
        let config_version = self.config_version.clone();
        
        // Spawn a task to forward updates from the gRPC stream to our channel
        tokio::spawn(async move {
            use futures_util::TryStreamExt;
            
            while let Ok(Some(update)) = stream.try_next().await {
                // Update the config version
                config_version.store(update.version, Ordering::SeqCst);
                
                // Forward the update
                if tx.send(Ok(update)).await.is_err() {
                    break;
                }
            }
        });
        
        // Return the receiver wrapped as a Stream
        Ok(ReceiverStream::new(rx))
    }
    
    /// Get a full configuration snapshot from the Control Plane
    pub async fn get_config_snapshot(&mut self) -> Result<Configuration> {
        // Create the request with authentication
        let mut request = Request::new(GetConfigSnapshotRequest {
            node_id: self.node_id.clone(),
        });
        
        // Add authentication token as metadata
        request.metadata_mut().insert(
            "authorization", 
            format!("Bearer {}", self.auth_token).parse().unwrap()
        );
        
        // Call the gRPC service
        let response = self.client.get_config_snapshot(request)
            .await
            .map_err(|e| anyhow!("Failed to get configuration snapshot: {}", e))?;
        
        // Extract the snapshot
        let snapshot = response.into_inner();
        
        // Update the config version
        self.config_version.store(snapshot.version, Ordering::SeqCst);
        
        // Convert the proto snapshot to a domain Configuration
        let config = snapshot.into_configuration()?;
        
        Ok(config)
    }
}

/// Extension trait to convert proto ConfigUpdate to domain Configuration
impl super::proto::ConfigUpdate {
    pub fn into_configuration(&self) -> Result<Configuration> {
        // Convert from proto format to domain model
        // This is a simplified implementation
        let mut proxies = Vec::new();
        let mut consumers = Vec::new();
        let mut plugin_configs = Vec::new();
        
        // Convert proxies
        for proxy in &self.proxies {
            proxies.push(proxy.try_into()?);
        }
        
        // Convert consumers
        for consumer in &self.consumers {
            consumers.push(consumer.try_into()?);
        }
        
        // Convert plugin configs
        for plugin_config in &self.plugin_configs {
            plugin_configs.push(plugin_config.try_into()?);
        }
        
        Ok(Configuration {
            proxies,
            consumers,
            plugin_configs,
            last_updated_at: chrono::Utc::now(),
        })
    }
}

/// Extension trait to convert proto ConfigSnapshot to domain Configuration
impl super::proto::ConfigSnapshot {
    pub fn into_configuration(&self) -> Result<Configuration> {
        // Convert from proto format to domain model
        // This is a simplified implementation
        let mut proxies = Vec::new();
        let mut consumers = Vec::new();
        let mut plugin_configs = Vec::new();
        
        // Convert proxies
        for proxy in &self.proxies {
            proxies.push(proxy.try_into()?);
        }
        
        // Convert consumers
        for consumer in &self.consumers {
            consumers.push(consumer.try_into()?);
        }
        
        // Convert plugin configs
        for plugin_config in &self.plugin_configs {
            plugin_configs.push(plugin_config.try_into()?);
        }
        
        Ok(Configuration {
            proxies,
            consumers,
            plugin_configs,
            last_updated_at: chrono::Utc::now(),
        })
    }
}
