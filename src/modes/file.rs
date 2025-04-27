use std::sync::Arc;
use std::path::Path;
use std::fs;
use std::time::Duration;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};

use crate::config::env_config::EnvConfig;
use crate::config::data_model::Configuration;
use crate::proxy::ProxyServer;
use crate::config::file_config;
use crate::dns::{self, DnsCache};

pub async fn run(config: EnvConfig) -> Result<()> {
    info!("Starting Ferrum Gateway in File mode");
    
    // Get configuration file path
    let config_path = config.file_config_path
        .as_ref()
        .context("File configuration path must be set in File mode")?;
    
    // Load initial configuration
    info!("Loading initial configuration from file: {}", config_path);
    let initial_config = load_configuration_from_file(config_path)
        .context("Failed to load initial configuration from file")?;
    
    // Validate listen_path uniqueness
    validate_listen_path_uniqueness(&initial_config)?;
    
    // Get DNS cache configuration
    let dns_ttl = config.dns_cache_ttl_seconds;
    let dns_overrides = config.dns_overrides.clone().unwrap_or_default();
    
    // Create DNS cache
    let dns_cache = Arc::new(DnsCache::new(dns_ttl, dns_overrides));
    
    // Create shared configuration
    let shared_config = Arc::new(RwLock::new(initial_config));
    
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
            let dns_cache_copy = Arc::clone(&dns_cache);
            dns::start_dns_prefetch_task(
                dns_cache_copy,
                proxies_copy,
                Duration::from_secs(300) // Check every 5 minutes
            );
        }
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
    
    // Set up signal handling for config reloading
    let shared_config_clone = Arc::clone(&shared_config);
    let config_path_clone = config_path.clone();
    let dns_cache_for_reload = Arc::clone(&dns_cache);
    
    let _reload_handle = tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            
            // Set up SIGHUP handler for config reload
            let mut sighup = signal(SignalKind::hangup()).unwrap();
            
            loop {
                // Wait for SIGHUP
                sighup.recv().await;
                info!("Received SIGHUP, reloading configuration from {}", config_path_clone);
                
                // Reload configuration
                match load_configuration_from_file(&config_path_clone) {
                    Ok(new_config) => {
                        // Validate listen_path uniqueness
                        if let Err(e) = validate_listen_path_uniqueness(&new_config) {
                            error!("Configuration validation failed during reload: {}", e);
                            continue;
                        }
                        
                        // Update shared configuration
                        let mut config = shared_config_clone.write().await;
                        *config = new_config;
                        info!("Configuration reloaded successfully");
                        
                        // Warm up DNS cache with new configuration
                        drop(config); // Release the write lock
                        {
                            let config_read = shared_config_clone.read().await;
                            if !config_read.proxies.is_empty() {
                                // Warm up DNS cache
                                if let Err(e) = dns::warm_up_dns_cache(&dns_cache_for_reload, &config_read.proxies).await {
                                    warn!("DNS cache warmup failed: {}", e);
                                }
                            }
                        }
                    },
                    Err(e) => {
                        error!("Failed to reload configuration: {}", e);
                    }
                }
            }
        }
        
        #[cfg(not(unix))]
        {
            // On non-unix platforms, we can't use SIGHUP, so we'll periodically check for changes
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            let mut last_modified = get_last_modified(&config_path_clone).unwrap_or_else(|_| std::time::SystemTime::now());
            
            loop {
                interval.tick().await;
                
                // Check if file was modified
                if let Ok(modified) = get_last_modified(&config_path_clone) {
                    if modified > last_modified {
                        info!("Configuration file changed, reloading from {}", config_path_clone);
                        last_modified = modified;
                        
                        // Reload configuration
                        match load_configuration_from_file(&config_path_clone) {
                            Ok(new_config) => {
                                // Validate listen_path uniqueness
                                if let Err(e) = validate_listen_path_uniqueness(&new_config) {
                                    error!("Configuration validation failed during reload: {}", e);
                                    continue;
                                }
                                
                                // Update shared configuration
                                let mut config = shared_config_clone.write().await;
                                *config = new_config;
                                info!("Configuration reloaded successfully");
                                
                                // Warm up DNS cache with new configuration
                                drop(config); // Release the write lock
                                {
                                    let config_read = shared_config_clone.read().await;
                                    if !config_read.proxies.is_empty() {
                                        // Warm up DNS cache
                                        if let Err(e) = dns::warm_up_dns_cache(&dns_cache_for_reload, &config_read.proxies).await {
                                            warn!("DNS cache warmup failed: {}", e);
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Failed to reload configuration: {}", e);
                            }
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

#[cfg(not(unix))]
fn get_last_modified(path: &str) -> Result<std::time::SystemTime> {
    let path = Path::new(path);
    if path.is_dir() {
        // Find the most recently modified file in the directory
        let mut latest = std::time::SystemTime::UNIX_EPOCH;
        
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_file() {
                let modified = metadata.modified()?;
                if modified > latest {
                    latest = modified;
                }
            }
        }
        
        if latest == std::time::SystemTime::UNIX_EPOCH {
            Err(anyhow::anyhow!("No files found in directory"))
        } else {
            Ok(latest)
        }
    } else {
        // Just get the modification time of the file
        let metadata = std::fs::metadata(path)?;
        metadata.modified().map_err(|e| e.into())
    }
}

fn load_configuration_from_file(config_path: &str) -> Result<Configuration> {
    let path = Path::new(config_path);
    
    if path.is_dir() {
        // If it's a directory, load all config files from it
        return file_config::load_from_directory(path);
    } else {
        // Read file content
        let content = fs::read_to_string(path)
            .context(format!("Failed to read configuration file: {}", config_path))?;
        
        // Parse based on file extension
        if let Some(extension) = path.extension() {
            if extension == "json" {
                return file_config::parse_json_config(&content);
            } else if extension == "yaml" || extension == "yml" {
                return file_config::parse_yaml_config(&content);
            }
        }
        
        // If no extension or unrecognized, try both formats
        if let Ok(config) = file_config::parse_json_config(&content) {
            return Ok(config);
        }
        if let Ok(config) = file_config::parse_yaml_config(&content) {
            return Ok(config);
        }
        
        anyhow::bail!("Unsupported configuration file format, expected JSON or YAML")
    }
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
