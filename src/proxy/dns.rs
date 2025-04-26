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
    pub fn new(default_ttl: Duration, overrides: HashMap<String, String>) -> Self {
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
    
    /// Resolves a hostname to an IP address, with caching
    pub async fn resolve(&self, hostname: &str) -> Result<String> {
        self.resolve_with_ttl(hostname, self.default_ttl).await
    }
    
    /// Resolves a hostname to an IP address with a specific TTL, with caching
    pub async fn resolve_with_ttl(&self, hostname: &str, ttl: Duration) -> Result<String> {
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
    
    /// Forces a refresh of the cache entry for a hostname
    pub async fn refresh(&self, hostname: &str) -> Result<String> {
        // Remove any existing cache entry
        self.cache.remove(hostname);
        
        // Perform a new lookup
        self.resolve(hostname).await
    }
    
    /// Clears the entire cache
    pub fn clear(&self) {
        self.cache.clear();
        debug!("DNS cache cleared");
    }
}
