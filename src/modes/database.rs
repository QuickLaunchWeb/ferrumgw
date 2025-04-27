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
use crate::dns::{self, DnsCache};

pub async fn run(config: EnvConfig) -> Result<()> {
    info!("Starting Ferrum Gateway in Database mode");
    
    // Get database configuration
    let db_type_str = config.db_type.clone().context("Database type must be set in Database mode")?;
    let db_url = config.db_url.clone().context("Database URL must be set in Database mode")?;
    
    // Convert config DatabaseType to database module's DatabaseType
    let db_type = match db_type_str {
        crate::config::data_model::DatabaseType::Postgres => crate::database::DatabaseType::Postgres,
        crate::config::data_model::DatabaseType::MySQL => crate::database::DatabaseType::MySQL,
        crate::config::data_model::DatabaseType::SQLite => crate::database::DatabaseType::SQLite,
    };
    
    // Set up database client
    let db_client = DatabaseClient::new(db_type, &db_url)
        .await
        .context("Failed to create database client")?;
    
    // Get DNS cache configuration
    let dns_ttl = config.dns_cache_ttl_seconds;
    let dns_overrides = config.dns_overrides.clone().unwrap_or_default();
    
    // Create DNS cache
    let dns_cache: Arc<crate::dns::cache::DnsCache> = Arc::new(DnsCache::new(dns_ttl, dns_overrides));
    
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
    
    // Load all proxies from config for DNS cache initialization
    {
        let config_read = shared_config.read().await;
        if !config_read.proxies.is_empty() {
            // Warm up DNS cache
            if let Err(e) = dns::warm_up_dns_cache(&dns_cache, &config_read.proxies).await {
                warn!("DNS cache warmup failed: {}", e);
            }
            
            // Start DNS prefetch background task
            let proxies_copy = Arc::new(RwLock::new(config_read.proxies.clone()));
            let dns_cache_clone: Arc<crate::dns::cache::DnsCache> = Arc::clone(&dns_cache);
            dns::start_dns_prefetch_task(
                dns_cache_clone,
                proxies_copy,
                Duration::from_secs(300) // Check every 5 minutes
            );
        }
    }
    
    // Start DNS refresh loop
    let dns_cache_clone: Arc<crate::dns::cache::DnsCache> = Arc::clone(&dns_cache);
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
    let poll_check_interval = config.db_poll_check_interval;
    let use_incremental_polling = config.db_incremental_polling;
    let dns_cache_for_polling: Arc<crate::dns::cache::DnsCache> = Arc::clone(&dns_cache);
    let shared_config_clone = Arc::clone(&shared_config);
    
    let _polling_handle = tokio::spawn(async move {
        let mut last_update_timestamp = shared_config_clone.read().await.last_updated_at;
        let mut poll_timer = tokio::time::interval(poll_interval);
        let mut check_timer = tokio::time::interval(poll_check_interval);
        
        loop {
            tokio::select! {
                // Fast lightweight check for changes
                _ = check_timer.tick() => {
                    // Check if there are any changes without downloading full config
                    match db_client.get_latest_update_timestamp().await {
                        Ok(latest_timestamp) => {
                            if latest_timestamp > last_update_timestamp {
                                debug!("Configuration change detected, update timestamp: {}", latest_timestamp);
                                
                                if use_incremental_polling {
                                    // Use delta updates for efficiency
                                    match db_client.load_configuration_delta(last_update_timestamp).await {
                                        Ok(delta) => {
                                            if !delta.is_empty() {
                                                info!("Applying incremental configuration update with {} proxies, {} consumers, {} plugin configs",
                                                    delta.updated_proxies.len() + delta.deleted_proxy_ids.len(),
                                                    delta.updated_consumers.len() + delta.deleted_consumer_ids.len(),
                                                    delta.updated_plugin_configs.len() + delta.deleted_plugin_config_ids.len());
                                                
                                                // Apply the delta to the shared configuration
                                                {
                                                    let mut config = shared_config_clone.write().await;
                                                    delta.apply_to(&mut *config);
                                                }
                                                
                                                // Update our tracking timestamp
                                                last_update_timestamp = delta.last_updated_at;
                                                
                                                // Check for new backend hosts that need DNS resolution
                                                let new_hosts = delta.updated_proxies.iter()
                                                    .filter(|p| p.dns_override.is_none())
                                                    .map(|p| p.backend_host.clone())
                                                    .collect::<Vec<_>>();
                                                
                                                if !new_hosts.is_empty() {
                                                    // Warm up DNS cache for new hosts in background
                                                    for hostname in new_hosts {
                                                        let dns_cache = Arc::clone(&dns_cache_for_polling);
                                                        tokio::spawn(async move {
                                                            if let Err(e) = dns_cache.resolve(&hostname).await {
                                                                warn!("DNS warmup failed for host {}: {}", hostname, e);
                                                            } else {
                                                                debug!("DNS warmup successful for host {}", hostname);
                                                            }
                                                        });
                                                    }
                                                }
                                                
                                                info!("Configuration updated successfully using incremental update");
                                            } else {
                                                debug!("Incremental update returned empty delta");
                                            }
                                        },
                                        Err(e) => {
                                            error!("Failed to load incremental configuration: {}", e);
                                            
                                            // Fallback to full config load on next poll
                                            poll_timer.reset();
                                        }
                                    }
                                } else {
                                    // Use full config load on next poll
                                    poll_timer.reset();
                                }
                            }
                        },
                        Err(e) => {
                            error!("Failed to check latest update timestamp: {}", e);
                        }
                    }
                },
                
                // Full configuration reload (less frequent)
                _ = poll_timer.tick() => {
                    match db_client.load_full_configuration().await {
                        Ok(new_config) => {
                            // Validate listen_path uniqueness
                            if let Err(e) = validate_listen_path_uniqueness(&new_config) {
                                error!("Configuration validation failed during polling: {}", e);
                                continue;
                            }
                            
                            // Check if configuration has changed
                            if new_config.last_updated_at > last_update_timestamp {
                                info!("Performing full configuration update");
                                {
                                    let mut config = shared_config_clone.write().await;
                                    *config = new_config;
                                }
                                
                                // Update our tracking timestamp
                                last_update_timestamp = new_config.last_updated_at;
                                
                                // After updating the configuration, warm up DNS cache for any new hosts
                                if let Err(e) = dns::warm_up_dns_cache(&dns_cache_for_polling, &new_config.proxies).await {
                                    warn!("DNS cache warmup failed: {}", e);
                                }
                                
                                info!("Configuration updated successfully with full refresh");
                            } else {
                                debug!("Full configuration refresh found no changes");
                            }
                        },
                        Err(e) => {
                            error!("Failed to load configuration from database: {}", e);
                        }
                    }
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
