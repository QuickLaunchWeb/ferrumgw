use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::{Result, Context};
use dashmap::DashMap;
use tokio::net::lookup_host;
use tracing::{debug, warn, trace};

/// A cache entry for resolved DNS records
#[derive(Debug, Clone)]
struct CacheEntry {
    /// The resolved IP address
    ip: String,
    /// When this entry was created
    created_at: Instant,
    /// Time-to-live for this entry
    ttl: Duration,
}

impl CacheEntry {
    /// Creates a new cache entry
    fn new(ip: String, ttl: Duration) -> Self {
        Self {
            ip,
            created_at: Instant::now(),
            ttl,
        }
    }
    
    /// Checks if this entry has expired
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.ttl
    }

    /// Returns time until expiration
    fn time_until_expiry(&self) -> Duration {
        if self.is_expired() {
            Duration::from_secs(0)
        } else {
            self.ttl.saturating_sub(self.created_at.elapsed())
        }
    }
}

/// A DNS cache that provides async resolution of hostnames with TTL-based expiration
#[derive(Debug)]
pub struct DnsCache {
    /// The underlying thread-safe cache
    cache: Arc<DashMap<String, CacheEntry>>,
    /// Default TTL for cache entries
    default_ttl: Duration,
    /// Static overrides (hostname -> IP) that never expire
    overrides: HashMap<String, String>,
}

impl DnsCache {
    /// Creates a new DNS cache with the specified default TTL and static overrides
    pub fn new(default_ttl_seconds: u64, overrides: HashMap<String, String>) -> Self {
        let default_ttl = Duration::from_secs(default_ttl_seconds);
        Self {
            cache: Arc::new(DashMap::new()),
            default_ttl,
            overrides,
        }
    }
    
    /// Gets the default TTL for this cache
    pub fn default_ttl(&self) -> Duration {
        self.default_ttl
    }
    
    /// Lookup a hostname, either from cache or by performing a new lookup
    pub async fn lookup(&self, hostname: &str) -> Result<String> {
        self.lookup_with_ttl(hostname, self.default_ttl).await
    }
    
    /// Lookup a hostname with a specific TTL
    pub async fn lookup_with_ttl(&self, hostname: &str, ttl: Duration) -> Result<String> {
        // Check if there's a static override for this hostname
        if let Some(ip) = self.overrides.get(hostname) {
            debug!("Using static DNS override for {}: {}", hostname, ip);
            return Ok(ip.clone());
        }
        
        // Check if there's a valid cache entry
        if let Some(entry) = self.cache.get(hostname) {
            if !entry.is_expired() {
                trace!("DNS cache hit for {}: {}", hostname, entry.ip);
                return Ok(entry.ip.clone());
            }
            
            // Entry is expired, remove it
            trace!("DNS cache entry for {} is expired, removing", hostname);
            self.cache.remove(hostname);
        }
        
        // No cache entry or expired, perform a lookup
        debug!("DNS cache miss for {}, resolving", hostname);
        let ip = self.perform_lookup(hostname).await?;
        
        // Cache the result
        let entry = CacheEntry::new(ip.clone(), ttl);
        self.cache.insert(hostname.to_string(), entry);
        debug!("Cached DNS result for {}: {} (TTL: {:?})", hostname, ip, ttl);
        
        Ok(ip)
    }
    
    /// Performs an actual DNS lookup
    async fn perform_lookup(&self, hostname: &str) -> Result<String> {
        // Use tokio's DNS resolver to look up the host
        let addrs = lookup_host(format!("{}:0", hostname))
            .await
            .context(format!("Failed to resolve hostname: {}", hostname))?;
        
        // Use the first address returned
        let addr = addrs
            .into_iter()
            .next()
            .context(format!("No addresses found for hostname: {}", hostname))?;
        
        // Extract the IP address as a string
        let ip = addr.ip().to_string();
        
        Ok(ip)
    }
    
    /// Prefetch a hostname if it will expire soon
    pub async fn prefetch(&self, hostname: &str, ttl: u64) -> Option<String> {
        // Skip prefetch for hostnames with static overrides
        if self.overrides.contains_key(hostname) {
            return None;
        }
        
        // Check if there's a current cache entry
        if let Some(entry) = self.cache.get(hostname) {
            // If entry isn't expired but will expire soon, do a prefetch
            if !entry.is_expired() {
                let ttl_duration = Duration::from_secs(ttl);
                let time_to_expiry = entry.time_until_expiry();
                
                if time_to_expiry < Duration::from_secs(60) {
                    debug!("Prefetching DNS entry for {} (expires in {:?})", 
                        hostname, time_to_expiry);
                    
                    // Clone values we need before dropping the entry reference
                    let current_ip = entry.ip.clone();
                    drop(entry);
                    
                    // Perform lookup in the background
                    let dns_cache = Arc::clone(&self.cache);
                    let hostname = hostname.to_string();
                    
                    tokio::spawn(async move {
                        match lookup_host(format!("{}:0", hostname)).await {
                            Ok(addrs) => {
                                if let Some(addr) = addrs.into_iter().next() {
                                    let new_ip = addr.ip().to_string();
                                    if new_ip != current_ip {
                                        debug!("DNS prefetch: IP for {} changed from {} to {}", 
                                            hostname, current_ip, new_ip);
                                    }
                                    let entry = CacheEntry::new(new_ip, ttl_duration);
                                    dns_cache.insert(hostname, entry);
                                }
                            }
                            Err(e) => {
                                warn!("DNS prefetch failed for {}: {}", hostname, e);
                            }
                        }
                    });
                    
                    return Some(current_ip);
                }
            }
        }
        
        None
    }
    
    /// Forces a refresh of the cache entry for a hostname
    pub async fn refresh(&self, hostname: &str) -> Result<String> {
        // Remove any existing cache entry
        self.cache.remove(hostname);
        
        // Perform a new lookup
        self.lookup(hostname).await
    }
    
    /// Clears the entire cache
    pub fn clear(&self) {
        self.cache.clear();
        debug!("DNS cache cleared");
    }

    /// Gets statistics about the cache
    pub fn stats(&self) -> DnsCacheStats {
        let total_entries = self.cache.len();
        let mut active_entries = 0;
        let mut expired_entries = 0;
        
        for entry in self.cache.iter() {
            if entry.is_expired() {
                expired_entries += 1;
            } else {
                active_entries += 1;
            }
        }
        
        DnsCacheStats {
            total_entries,
            active_entries,
            expired_entries,
            override_entries: self.overrides.len(),
        }
    }
    
    /// Purges expired entries from the cache
    pub fn purge_expired(&self) -> usize {
        let mut purged = 0;
        
        for entry in self.cache.iter() {
            let hostname = entry.key();
            if entry.is_expired() {
                self.cache.remove(hostname);
                purged += 1;
            }
        }
        
        if purged > 0 {
            debug!("Purged {} expired DNS cache entries", purged);
        }
        
        purged
    }
}

/// Statistics about the DNS cache
#[derive(Debug, Clone, Copy)]
pub struct DnsCacheStats {
    /// Total number of entries (active + expired)
    pub total_entries: usize,
    /// Number of active (unexpired) entries
    pub active_entries: usize,
    /// Number of expired entries
    pub expired_entries: usize,
    /// Number of static override entries
    pub override_entries: usize,
}
