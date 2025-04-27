// DNS module for Ferrum Gateway
//
// This module provides DNS resolution and caching functionality.

pub mod cache;

pub use cache::DnsCache;
pub use cache::DnsCacheStats;

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use anyhow::Result;

use crate::config::data_model::Proxy;

/// Warm up DNS cache with all configured proxy backend hosts
pub async fn warm_up_dns_cache(
    dns_cache: &DnsCache, 
    proxies: &[Proxy]
) -> Result<()> {
    debug!("Pre-warming DNS cache with {} proxy backend hosts", proxies.len());
    
    let mut unique_hosts = HashSet::new();
    for proxy in proxies {
        unique_hosts.insert(proxy.backend_host.clone());
    }
    
    let mut success_count = 0;
    let mut error_count = 0;
    
    for host in unique_hosts {
        match dns_cache.lookup(&host).await {
            Ok(ip) => {
                debug!("DNS cache pre-warmed for {}: {}", host, ip);
                success_count += 1;
            },
            Err(e) => {
                warn!("Failed to pre-warm DNS cache for {}: {}", host, e);
                error_count += 1;
            }
        }
    }
    
    info!(
        "DNS cache pre-warming complete: {} successful, {} failed", 
        success_count, error_count
    );
    
    Ok(())
}

/// Start a background task to periodically prefetch DNS entries
pub fn start_dns_prefetch_task(
    dns_cache: Arc<DnsCache>,
    proxies: Arc<RwLock<Vec<Proxy>>>,
    check_interval: std::time::Duration
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(check_interval);
        
        loop {
            interval.tick().await;
            
            let proxies_guard = proxies.read().await;
            let unique_hosts: HashSet<String> = proxies_guard
                .iter()
                .map(|p| p.backend_host.clone())
                .collect();
            drop(proxies_guard); // Release lock before async operations
            
            for host in unique_hosts {
                // Try to prefetch if entry is about to expire
                dns_cache.prefetch(&host, 300).await; // 5-minute TTL
            }
            
            // Purge expired entries occasionally
            let _ = dns_cache.purge_expired();
        }
    })
}
