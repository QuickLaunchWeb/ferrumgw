use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use chrono::Utc;

use crate::config::env_config::EnvConfig;
use crate::config::data_model::Configuration;
use crate::database::DatabaseClient;
use crate::proxy::ProxyServer;
use crate::admin::AdminServer;
use crate::dns::cache::DnsCache;

pub async fn run(config: EnvConfig) -> Result<()> {
    info!("Starting Ferrum Gateway in Database mode");
    
    // Set up database client
    let db_type = config.db_type.clone().context("Database type must be set in Database mode")?;
    let db_url = config.db_url.clone().context("Database URL must be set in Database mode")?;
    
    let db_client = DatabaseClient::new(db_type, &db_url)
        .await
        .context("Failed to create database client")?;
    
    // Get DNS cache configuration
    let dns_ttl = config.dns_cache_ttl_seconds;
    let dns_overrides = config.dns_overrides.clone().unwrap_or_default();
    
    // Create DNS cache
    let dns_cache = Arc::new(DnsCache::new(dns_ttl, dns_overrides));
    
    // Create shared configuration
    let shared_config = Arc::new(RwLock::new(Configuration {
        proxies: Vec::new(),
        consumers: Vec::new(),
        plugin_configs: Vec::new(),
        last_updated_at: Utc::now(),
    }));
    
    // Load initial configuration from database
    let initial_config = db_client.load_full_configuration()
        .await
        .context("Failed to load initial configuration from database")?;
    
    // Update shared configuration
    {
        let mut config_write = shared_config.write().await;
        *config_write = initial_config;
    }
    
    // Validate listen_path uniqueness
    validate_listen_path_uniqueness(&shared_config.read().await)?;
    
    // Warm up DNS cache with initial configuration
    warm_up_dns_cache(&shared_config, &dns_cache).await;
    
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
                    dns_cache_clone.prefetch(&proxy.backend_host, ttl).await;
                }
            }
        }
    });
    
    // Start proxy server if ports are configured
    let _proxy_server = if config.proxy_http_port.is_some() || config.proxy_https_port.is_some() {
        info!("Starting proxy server");
        let proxy_server = ProxyServer::new(
            config.clone(),
            shared_config.clone(),
            Arc::clone(&dns_cache),
        )?;
        
        tokio::spawn(async move {
            if let Err(e) = proxy_server.start().await {
                error!("Proxy server error: {}", e);
            }
        });
        
        Some(proxy_server)
    } else {
        warn!("No proxy HTTP/HTTPS ports configured, proxy server will not be started");
        None
    };
    
    // Start admin server if ports are configured
    let _admin_server = if config.admin_http_port.is_some() || config.admin_https_port.is_some() {
        info!("Starting admin server");
        let admin_jwt_secret = config.admin_jwt_secret.clone()
            .context("Admin JWT secret must be set in Database mode")?;
            
        let admin_server = AdminServer::new(
            config.clone(),
            shared_config.clone(),
            db_client.clone(),
            admin_jwt_secret,
        )?;
        
        tokio::spawn(async move {
            if let Err(e) = admin_server.start().await {
                error!("Admin server error: {}", e);
            }
        });
        
        Some(admin_server)
    } else {
        warn!("No admin HTTP/HTTPS ports configured, admin server will not be started");
        None
    };
    
    // Start configuration polling
    let poll_interval = config.db_poll_interval;
    let dns_cache_for_polling = Arc::clone(&dns_cache);
    let shared_config_clone = Arc::clone(&shared_config);
    
    let _polling_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(poll_interval);
        
        loop {
            interval.tick().await;
            
            match db_client.load_full_configuration().await {
                Ok(new_config) => {
                    // Validate listen_path uniqueness
                    if let Err(e) = validate_listen_path_uniqueness(&new_config) {
                        error!("Configuration validation failed during polling: {}", e);
                        continue;
                    }
                    
                    // Check if configuration has changed
                    let current_updated_at = shared_config_clone.read().await.last_updated_at;
                    if new_config.last_updated_at > current_updated_at {
                        info!("Configuration changed, updating...");
                        {
                            let mut config = shared_config_clone.write().await;
                            *config = new_config;
                        }
                        
                        // After updating the configuration, warm up DNS cache for any new hosts
                        warm_up_dns_cache(&shared_config_clone, &dns_cache_for_polling).await;
                        
                        info!("Configuration updated successfully");
                    }
                },
                Err(e) => {
                    error!("Failed to load configuration from database: {}", e);
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

fn validate_listen_path_uniqueness(config: &Configuration) -> Result<()> {
    let mut seen_paths = std::collections::HashSet::new();
    
    for proxy in &config.proxies {
        if !seen_paths.insert(&proxy.listen_path) {
            return Err(anyhow::anyhow!(
                "Duplicate listen_path detected: {}. All paths must be unique.", 
                proxy.listen_path
            ));
        }
    }
    
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
