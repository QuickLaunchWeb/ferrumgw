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
use crate::dns::cache::DnsCache;

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
    let dns_overrides = config.dns_overrides.clone().unwrap_or_default();
    
    // Create DNS cache
    let dns_cache = Arc::new(DnsCache::new(dns_ttl, dns_overrides));
    
    // Create shared configuration
    let shared_config = Arc::new(RwLock::new(initial_config));
    
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
    
    // Start DNS refresh loop
    let dns_cache_clone = Arc::clone(&dns_cache);
    let shared_config_for_dns = Arc::clone(&shared_config);
    
    let _dns_refresh_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        
        loop {
            interval.tick().await;
            
            // Refresh DNS cache entries that are expiring soon
            let config = shared_config_for_dns.read().await;
            
            if !config.proxies.is_empty() {
                debug!("Refreshing DNS cache entries");
                
                for proxy in &config.proxies {
                    if let Some(dns_override) = &proxy.dns_override {
                        // Skip proxies with explicit overrides
                        continue;
                    }
                    
                    // Get the TTL for this proxy, or fall back to global TTL
                    let ttl = proxy.dns_cache_ttl_seconds.unwrap_or(dns_ttl);
                    
                    // Refresh only if the entry will expire in the next minute
                    // This is a prefetch strategy to avoid cache misses
                    dns_cache_clone.prefetch(&proxy.backend_host, ttl).await;
                }
            }
        }
    });
    
    // Start gRPC client to connect to Control Plane
    let shared_config_clone = Arc::clone(&shared_config);
    let dns_cache_for_grpc = Arc::clone(&dns_cache);
    
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
    dns_cache: Arc<DnsCache>,
    reconnect_notify: tokio::sync::mpsc::Sender<()>,
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
            warm_up_dns_cache(&shared_config, &dns_cache).await;
            
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
                
                // Update shared configuration
                {
                    let mut config = shared_config.write().await;
                    *config = updated_config;
                }
                
                // Update DNS cache for new/changed backend hosts
                warm_up_dns_cache(&shared_config, &dns_cache).await;
                
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

/// Warm up the DNS cache with all backend hosts in the configuration
async fn warm_up_dns_cache(config: &Arc<RwLock<Configuration>>, dns_cache: &Arc<DnsCache>) {
    let config_read = config.read().await;
    
    if config_read.proxies.is_empty() {
        debug!("No proxies in configuration, skipping DNS cache warmup");
        return;
    }
    
    info!("Warming up DNS cache for {} proxies", config_read.proxies.len());
    
    // Collect unique backend hosts
    let mut hosts = std::collections::HashSet::new();
    
    for proxy in &config_read.proxies {
        if proxy.dns_override.is_none() {
            hosts.insert(proxy.backend_host.clone());
        }
    }
    
    // Perform DNS lookups concurrently
    let mut tasks = Vec::new();
    
    for host in hosts {
        let dns_cache = Arc::clone(dns_cache);
        let task = tokio::spawn(async move {
            if let Err(e) = dns_cache.lookup(&host).await {
                warn!("DNS warmup lookup failed for host {}: {}", host, e);
            } else {
                debug!("DNS cache warmed up for host {}", host);
            }
        });
        
        tasks.push(task);
    }
    
    // Wait for all lookups to complete
    for task in tasks {
        let _ = task.await;
    }
    
    info!("DNS cache warmup completed");
}
