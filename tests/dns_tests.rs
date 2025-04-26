#[cfg(test)]
mod dns_tests {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};
    
    use ferrumgw::proxy::dns::DnsCache;
    
    #[tokio::test]
    async fn test_dns_cache_lookup() {
        // Create a new DNS cache with a short TTL for testing
        let ttl_seconds = 1;
        let dns_cache = Arc::new(DnsCache::new(ttl_seconds, HashMap::new()));
        
        // Test lookup for a reliable domain
        let host = "example.com";
        let result = dns_cache.resolve(host).await;
        assert!(result.is_ok(), "Failed to resolve {}: {:?}", host, result);
        
        let ip = result.unwrap();
        assert!(ip.is_ipv4() || ip.is_ipv6(), "Expected valid IP address, got: {:?}", ip);
        
        // Verify that the result is cached
        let cached_result = dns_cache.resolve(host).await;
        assert!(cached_result.is_ok());
        assert_eq!(cached_result.unwrap(), ip, "Cache didn't return the same IP");
        
        // Wait for the TTL to expire
        sleep(Duration::from_secs(ttl_seconds + 1)).await;
        
        // After TTL expiration, a new lookup should happen
        let new_result = dns_cache.resolve(host).await;
        assert!(new_result.is_ok());
        // The IP might be the same in a real environment, but the lookup should have happened
    }
    
    #[tokio::test]
    async fn test_dns_overrides() {
        // Create DNS cache with overrides
        let mut overrides = HashMap::new();
        overrides.insert(
            "override.example.com".to_string(), 
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100))
        );
        
        let dns_cache = Arc::new(DnsCache::new(300, overrides));
        
        // Test that override works
        let host = "override.example.com";
        let result = dns_cache.resolve(host).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(), 
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            "Override didn't work correctly"
        );
        
        // Test a host without an override
        let normal_host = "example.com";
        let normal_result = dns_cache.resolve(normal_host).await;
        assert!(normal_result.is_ok());
        assert_ne!(
            normal_result.unwrap(),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            "Normal lookup incorrectly used override"
        );
    }
    
    #[tokio::test]
    async fn test_dns_warmup() {
        // Create DNS cache
        let dns_cache = Arc::new(DnsCache::new(300, HashMap::new()));
        
        // List of hosts to warm up
        let hosts = vec![
            "example.com",
            "google.com",
            "cloudflare.com",
        ];
        
        // Warm up the cache
        let result = dns_cache.warmup(&hosts).await;
        assert!(result.is_ok(), "Failed to warm up DNS cache: {:?}", result);
        
        // Verify that all hosts are cached
        for host in hosts {
            // This should be near-instant since it's cached
            let lookup_future = dns_cache.resolve(host);
            let result = timeout(Duration::from_millis(50), lookup_future).await;
            assert!(result.is_ok(), "Lookup timed out for {}", host);
            let ip = result.unwrap();
            assert!(ip.is_ok(), "Failed to resolve {}: {:?}", host, ip);
        }
    }
    
    #[tokio::test]
    async fn test_dns_error_handling() {
        let dns_cache = Arc::new(DnsCache::new(300, HashMap::new()));
        
        // Test with an invalid hostname
        let invalid_host = "thishostdoesnotexist.invalid";
        let result = dns_cache.resolve(invalid_host).await;
        assert!(result.is_err(), "Expected error for invalid hostname");
        
        // Test that cache doesn't store failed lookups permanently
        // (A real implementation might have negative caching with a shorter TTL)
        let second_try = dns_cache.resolve(invalid_host).await;
        assert!(second_try.is_err(), "Expected error on second try");
    }
    
    #[tokio::test]
    async fn test_dns_per_proxy_ttl() {
        // This test would validate that per-proxy TTL overrides work correctly
        // In a real implementation, the DnsCache would need to support this feature
        
        // Create a DNS cache with a default TTL
        let default_ttl = 300;
        let dns_cache = Arc::new(DnsCache::new(default_ttl, HashMap::new()));
        
        // In a real implementation, we would:
        // 1. Resolve a host with the default TTL
        // 2. Resolve a host with a custom TTL specified in the proxy config
        // 3. Wait for the shorter TTL to expire
        // 4. Verify that only the custom TTL host needs re-resolution
        
        // For now we'll just test basic resolution
        let host = "example.com";
        let result = dns_cache.resolve(host).await;
        assert!(result.is_ok(), "Failed to resolve {}: {:?}", host, result);
    }
    
    #[tokio::test]
    async fn test_dns_concurrent_lookups() {
        let dns_cache = Arc::new(DnsCache::new(300, HashMap::new()));
        
        // Start multiple concurrent lookups for the same host
        let host = "example.com";
        let lookup_tasks: Vec<_> = (0..10)
            .map(|_| {
                let cache = Arc::clone(&dns_cache);
                let h = host.to_string();
                tokio::spawn(async move {
                    cache.resolve(&h).await
                })
            })
            .collect();
        
        // Wait for all lookups to complete
        let results = futures::future::join_all(lookup_tasks).await;
        
        // All lookups should succeed and return the same IP
        let first_result = results[0].as_ref().unwrap().as_ref().unwrap();
        
        for (i, result) in results.iter().enumerate() {
            assert!(result.is_ok(), "Task {} failed", i);
            let inner_result = result.as_ref().unwrap();
            assert!(inner_result.is_ok(), "Lookup {} failed: {:?}", i, inner_result);
            assert_eq!(
                inner_result.as_ref().unwrap(),
                first_result,
                "Lookup {} returned different IP", i
            );
        }
    }
}
