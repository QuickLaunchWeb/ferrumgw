#[cfg(test)]
mod proxy_tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use chrono::Utc;
    use http::{Request, Uri};
    use hyper::Body;
    use tokio::sync::RwLock;
    
    use ferrumgw::config::data_model::{Configuration, Proxy, Protocol, AuthMode};
    use ferrumgw::proxy::router::Router;
    use ferrumgw::proxy::handler::RequestContext;
    
    // Helper function to create a test proxy
    fn create_test_proxy(id: &str, listen_path: &str, backend_host: &str, backend_port: u16) -> Proxy {
        Proxy {
            id: id.to_string(),
            name: Some(format!("Test Proxy {}", id)),
            listen_path: listen_path.to_string(),
            backend_protocol: Protocol::Http,
            backend_host: backend_host.to_string(),
            backend_port,
            backend_path: Some("/api".to_string()),
            strip_listen_path: true,
            preserve_host_header: false,
            backend_connect_timeout_ms: 5000,
            backend_read_timeout_ms: 30000,
            backend_write_timeout_ms: 30000,
            backend_tls_client_cert_path: None,
            backend_tls_client_key_path: None,
            backend_tls_verify_server_cert: true,
            backend_tls_server_ca_cert_path: None,
            dns_override: None,
            dns_cache_ttl_seconds: None,
            auth_mode: AuthMode::Single,
            plugins: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
    
    #[tokio::test]
    async fn test_router_longest_prefix_match() {
        // Create test proxies with different listen paths
        let proxies = vec![
            create_test_proxy("1", "/api", "api.example.com", 80),
            create_test_proxy("2", "/api/users", "users.example.com", 80),
            create_test_proxy("3", "/api/products", "products.example.com", 80),
            create_test_proxy("4", "/services", "services.example.com", 80),
        ];
        
        let config = Configuration {
            proxies: proxies.clone(),
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        
        let config_store = Arc::new(RwLock::new(config));
        let router = Router::new(config_store);
        
        // Test exact matches
        let result = router.find_proxy("/api").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "1");
        
        let result = router.find_proxy("/api/users").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "2");
        
        let result = router.find_proxy("/services").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "4");
        
        // Test prefix matches
        let result = router.find_proxy("/api/users/123").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "2"); // Should match "/api/users"
        
        let result = router.find_proxy("/api/other").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "1"); // Should match "/api"
        
        // Test non-matching path
        let result = router.find_proxy("/non-existent").await;
        assert!(result.is_none());
    }
    
    #[tokio::test]
    async fn test_path_rewriting() {
        // Test with strip_listen_path = true (default)
        let proxy = create_test_proxy("1", "/api", "api.example.com", 80);
        let mut ctx = RequestContext {
            proxy: proxy.clone(),
            original_uri: Uri::from_static("https://gateway.example.com/api/users/123?query=value"),
            client_addr: "127.0.0.1:12345".parse().unwrap(),
            consumer: None,
            auth_status: None,
            request_id: "req123".to_string(),
            start_time: Utc::now(),
            metrics: HashMap::new(),
        };
        
        // Strip listen_path and add backend_path
        let target_uri = ferrumgw::proxy::router::build_backend_uri(&mut ctx).unwrap();
        assert_eq!(target_uri.to_string(), "http://api.example.com:80/api/users/123?query=value");
        
        // Test with strip_listen_path = false
        let mut proxy_no_strip = proxy.clone();
        proxy_no_strip.strip_listen_path = false;
        
        let mut ctx2 = RequestContext {
            proxy: proxy_no_strip,
            original_uri: Uri::from_static("https://gateway.example.com/api/users/123?query=value"),
            client_addr: "127.0.0.1:12345".parse().unwrap(),
            consumer: None,
            auth_status: None,
            request_id: "req123".to_string(),
            start_time: Utc::now(),
            metrics: HashMap::new(),
        };
        
        // Keep original path and add backend_path
        let target_uri = ferrumgw::proxy::router::build_backend_uri(&mut ctx2).unwrap();
        assert_eq!(target_uri.to_string(), "http://api.example.com:80/api/api/users/123?query=value");
    }
    
    #[tokio::test]
    async fn test_header_propagation() {
        // Create test proxy
        let proxy = create_test_proxy("1", "/api", "api.example.com", 80);
        
        // Create request with headers to be modified
        let mut request = Request::builder()
            .uri("https://gateway.example.com/api/test")
            .header("Host", "gateway.example.com")
            .header("X-Custom-Header", "custom-value")
            .body(Body::empty())
            .unwrap();
        
        // Test with preserve_host_header = false (default)
        let ctx = RequestContext {
            proxy: proxy.clone(),
            original_uri: request.uri().clone(),
            client_addr: "127.0.0.1:12345".parse().unwrap(),
            consumer: None,
            auth_status: None,
            request_id: "req123".to_string(),
            start_time: Utc::now(),
            metrics: HashMap::new(),
        };
        
        ferrumgw::proxy::handler::set_forwarding_headers(&mut request, &ctx, "127.0.0.1");
        
        // Host header should be backend host
        assert_eq!(request.headers().get("Host").unwrap(), "api.example.com:80");
        
        // Should add X-Forwarded headers
        assert!(request.headers().contains_key("X-Forwarded-For"));
        assert_eq!(request.headers().get("X-Forwarded-Proto").unwrap(), "https");
        assert_eq!(request.headers().get("X-Forwarded-Host").unwrap(), "gateway.example.com");
        
        // Test with preserve_host_header = true
        let mut proxy_preserve_host = proxy.clone();
        proxy_preserve_host.preserve_host_header = true;
        
        let mut request2 = Request::builder()
            .uri("https://gateway.example.com/api/test")
            .header("Host", "gateway.example.com")
            .header("X-Custom-Header", "custom-value")
            .body(Body::empty())
            .unwrap();
        
        let ctx2 = RequestContext {
            proxy: proxy_preserve_host,
            original_uri: request2.uri().clone(),
            client_addr: "127.0.0.1:12345".parse().unwrap(),
            consumer: None,
            auth_status: None,
            request_id: "req123".to_string(),
            start_time: Utc::now(),
            metrics: HashMap::new(),
        };
        
        ferrumgw::proxy::handler::set_forwarding_headers(&mut request2, &ctx2, "127.0.0.1");
        
        // Host header should remain unchanged
        assert_eq!(request2.headers().get("Host").unwrap(), "gateway.example.com");
    }
}
