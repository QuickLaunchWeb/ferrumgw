use std::sync::Arc;
use std::time::Duration;
use std::cmp::min;
use tokio::sync::{RwLock, mpsc};
use tokio_stream::StreamExt;
use rand::Rng;
use anyhow::{Result, Context, anyhow};
use tracing::{info, warn, error, debug};

use crate::config::env_config::EnvConfig;
use crate::config::data_model::Configuration;
use crate::proxy::ProxyServer;
use crate::grpc::config_client::ConfigClient;
use crate::dns::{self, DnsCache};

pub async fn run(config: EnvConfig) -> Result<()> {
    info!("Starting Ferrum Gateway in Data Plane mode");
    
    // Get gRPC client connection details
    let cp_grpc_url = config.dp_cp_grpc_url.clone()
        .context("Control Plane gRPC URL must be set in Data Plane mode")?;
    
    let grpc_auth_token = config.dp_grpc_auth_token.clone()
        .context("gRPC Auth Token must be set in Data Plane mode")?;
    
    // Initialize with empty configuration (will be populated by CP)
    let initial_config = Configuration {
        proxies: Vec::new(),
        consumers: Vec::new(),
        plugin_configs: Vec::new(),
        last_updated_at: chrono::Utc::now(),
    };
    
    // Get DNS cache configuration
    let dns_ttl = config.dns_cache_ttl_seconds;
    let dns_overrides = config.dns_overrides.clone();
    
    // Create DNS cache
    let dns_cache = Arc::new(DnsCache::new(dns_ttl, dns_overrides));
    
    // Create shared configuration
    let shared_config = Arc::new(RwLock::new(initial_config));
    
    // Initialize DNS prefetch task
    {
        // Start DNS prefetch background task with empty proxies initially
        // It will be updated as configurations come in from the control plane
        let proxies_copy = Arc::new(RwLock::new(Vec::new()));
        let dns_cache_copy = Arc::clone(&dns_cache);
        dns::start_dns_prefetch_task(
            dns_cache_copy,
            proxies_copy,
            Duration::from_secs(300) // Check every 5 minutes
        );
    }
    
    // Start proxy server with the configuration
    info!("Starting proxy server");
    let proxy_server = ProxyServer::new(
        config.clone(),
        Arc::clone(&shared_config),
        Arc::clone(&dns_cache),
    )?;
    
    let _proxy_handle = tokio::spawn(async move {
        if let Err(e) = proxy_server.start().await {
            error!("Proxy server error: {}", e);
        }
    });
    
    // Create a channel for reconnect notifications
    let (reconnect_notify_tx, mut reconnect_notify_rx) = mpsc::channel(1);
    
    // Start gRPC client to connect to Control Plane
    let shared_config_clone = Arc::clone(&shared_config);
    let dns_cache_for_grpc: Arc<crate::dns::cache::DnsCache> = Arc::clone(&dns_cache);
    
    let _grpc_client_handle = tokio::spawn(async move {
        let mut connection_retry_delay = Duration::from_secs(1);
        const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
        const MIN_RETRY_DELAY: Duration = Duration::from_secs(1);
        
        loop {
            info!("Connecting to Control Plane at {}", cp_grpc_url);
            
            match connect_to_control_plane(
                &cp_grpc_url, 
                &grpc_auth_token, 
                shared_config_clone.clone(),
                dns_cache_for_grpc.clone(),
                reconnect_notify_tx.clone()
            ).await {
                Ok(()) => {
                    info!("Connection to Control Plane closed normally, reconnecting immediately");
                    // If the connection closed normally, reset the retry delay
                    connection_retry_delay = MIN_RETRY_DELAY;
                },
                Err(e) => {
                    error!("Control Plane connection error: {}", e);
                    
                    // Notify about connection loss
                    if let Err(e) = reconnect_notify_tx.send(()).await {
                        error!("Failed to send reconnection notification: {}", e);
                    }
                    
                    // Apply exponential backoff with jitter for reconnect attempts
                    let jitter = std::time::Duration::from_millis(
                        (rand::random::<f32>() * 500.0) as u64
                    );
                    
                    let retry_delay = connection_retry_delay.saturating_add(jitter);
                    info!("Retrying connection in {:?}", retry_delay);
                    tokio::time::sleep(retry_delay).await;
                    
                    // Increase retry delay with exponential backoff
                    connection_retry_delay = min(
                        connection_retry_delay.saturating_mul(2), 
                        MAX_RETRY_DELAY
                    );
                }
            }
        }
    });
    
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await
        .context("Failed to listen for ctrl-c signal")?;
    
    info!("Shutdown signal received, stopping services");
    
    // Allow in-flight requests to complete
    info!("Waiting for in-flight requests to complete...");
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    info!("Shutdown complete");
    Ok(())
}

/// Connect to the control plane and start receiving configuration updates
async fn connect_to_control_plane(
    cp_url: &str,
    auth_token: &str, 
    shared_config: Arc<RwLock<Configuration>>,
    dns_cache: Arc<crate::dns::cache::DnsCache>,
    reconnect_notify: mpsc::Sender<()>,
) -> Result<()> {
    // Connect to the Control Plane gRPC service
    info!("Connecting to Control Plane gRPC service at {}", cp_url);
    let mut client = ConfigClient::connect(cp_url, auth_token.to_string()).await?;
    
    // First, get a full configuration snapshot
    info!("Requesting initial configuration snapshot");
    match client.get_config_snapshot().await {
        Ok(snapshot) => {
            info!("Received initial configuration with {} proxies, {} consumers, and {} plugin configs",
                snapshot.proxies.len(), snapshot.consumers.len(), snapshot.plugin_configs.len());
            
            // Update shared configuration
            {
                let mut config = shared_config.write().await;
                *config = snapshot;
            }
            
            // Warm up DNS cache for all backend hosts
            if let Err(e) = dns::warm_up_dns_cache(&dns_cache, &shared_config.read().await.proxies).await {
                warn!("DNS cache warmup for initial proxies failed: {}", e);
            }
            
            info!("Initial configuration loaded successfully");
        },
        Err(e) => {
            error!("Failed to get initial configuration snapshot: {}", e);
            return Err(anyhow!("Failed to retrieve initial configuration: {}", e));
        }
    }
    
    // Now subscribe to ongoing configuration updates
    info!("Subscribing to configuration updates");
    let mut stream = client.subscribe().await?;
    
    // Process configuration updates
    while let Some(update) = stream.next().await {
        match update {
            Ok(config_update) => {
                info!("Received configuration update from Control Plane (version: {})", config_update.version);
                
                // Convert proto message to domain model
                let updated_config = match config_update.into_configuration() {
                    Ok(config) => config,
                    Err(e) => {
                        error!("Failed to convert configuration update: {}", e);
                        continue;
                    }
                };
                
                // Apply update to shared configuration
                {
                    let mut config = shared_config.write().await;
                    let old_proxies_count = config.proxies.len();
                    
                    // Update configuration
                    *config = updated_config;
                    
                    // Warm up DNS cache with new configuration
                    drop(config); // Release the write lock
                    
                    let config_read = shared_config.read().await;
                    // Only warm up if there are proxies and we've actually added new ones
                    if !config_read.proxies.is_empty() && config_read.proxies.len() > old_proxies_count {
                        if let Err(e) = dns::warm_up_dns_cache(&dns_cache, &config_read.proxies).await {
                            warn!("DNS cache warmup for new proxies failed: {}", e);
                        }
                    }
                    drop(config_read);
                }
                
                info!("Configuration updated successfully");
            },
            Err(e) => {
                error!("Error receiving configuration update: {}", e);
                return Err(anyhow!("Control Plane stream error: {}", e));
            }
        }
    }
    
    info!("Configuration update stream ended");
    Ok(())
}
