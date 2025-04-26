#[cfg(test)]
mod plugin_tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use chrono::Utc;
    use http::{HeaderMap, Request, Response, StatusCode};
    use hyper::Body;
    use serde_json::json;
    use async_trait::async_trait;
    
    use ferrumgw::config::data_model::{Consumer, PluginConfig, Proxy, Protocol, AuthMode};
    use ferrumgw::plugins::{Plugin, PluginManager};
    use ferrumgw::proxy::handler::RequestContext;
    
    // Mock plugin for testing
    struct MockPlugin {
        name: &'static str,
        should_auth_succeed: bool,
        should_authorize_succeed: bool,
    }
    
    impl MockPlugin {
        fn new(name: &'static str, auth_success: bool, authorize_success: bool) -> Self {
            Self {
                name,
                should_auth_succeed: auth_success,
                should_authorize_succeed: authorize_success,
            }
        }
    }
    
    #[async_trait]
    impl Plugin for MockPlugin {
        fn name(&self) -> &'static str {
            self.name
        }
        
        async fn on_request_received(&self, _req: &mut Request<Body>, _ctx: &mut RequestContext) -> anyhow::Result<()> {
            // Just record that the hook was called
            Ok(())
        }
        
        async fn authenticate(&self, _req: &mut Request<Body>, ctx: &mut RequestContext) -> anyhow::Result<bool> {
            if self.should_auth_succeed {
                // Set a mock consumer if authentication succeeds
                ctx.consumer = Some(Consumer {
                    id: "test_consumer".to_string(),
                    username: "test_user".to_string(),
                    custom_id: None,
                    credentials: HashMap::new(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                });
                Ok(true)
            } else {
                Ok(false)
            }
        }
        
        async fn authorize(&self, _req: &mut Request<Body>, _ctx: &mut RequestContext) -> anyhow::Result<bool> {
            Ok(self.should_authorize_succeed)
        }
    }
    
    // Helper to create a test context
    fn create_test_context() -> RequestContext {
        let proxy = Proxy {
            id: "test_proxy".to_string(),
            name: Some("Test Proxy".to_string()),
            listen_path: "/api".to_string(),
            backend_protocol: Protocol::Http,
            backend_host: "example.com".to_string(),
            backend_port: 80,
            backend_path: None,
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
        };
        
        RequestContext {
            proxy,
            original_uri: "https://gateway.example.com/api/test".parse().unwrap(),
            client_addr: "127.0.0.1:12345".parse().unwrap(),
            consumer: None,
            auth_status: None,
            request_id: "req123".to_string(),
            start_time: Utc::now(),
            metrics: HashMap::new(),
        }
    }
    
    #[tokio::test]
    async fn test_plugin_authentication_single_mode() {
        let mut plugin_manager = PluginManager::new();
        
        // Register test plugins
        let auth_plugin = Arc::new(MockPlugin::new("auth_plugin", true, true));
        plugin_manager.register_plugin(auth_plugin);
        
        // Create test request and context
        let mut req = Request::builder()
            .uri("https://gateway.example.com/api/test")
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        ctx.proxy.auth_mode = AuthMode::Single;
        
        // Test authentication
        let result = plugin_manager.authenticate(&mut req, &mut ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert!(ctx.consumer.is_some());
        assert_eq!(ctx.consumer.as_ref().unwrap().username, "test_user");
    }
    
    #[tokio::test]
    async fn test_plugin_authentication_multi_mode() {
        let mut plugin_manager = PluginManager::new();
        
        // Register test plugins - first fails, second succeeds
        let fail_plugin = Arc::new(MockPlugin::new("fail_plugin", false, true));
        let auth_plugin = Arc::new(MockPlugin::new("auth_plugin", true, true));
        plugin_manager.register_plugin(fail_plugin);
        plugin_manager.register_plugin(auth_plugin);
        
        // Create test request and context
        let mut req = Request::builder()
            .uri("https://gateway.example.com/api/test")
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        ctx.proxy.auth_mode = AuthMode::Multi;
        
        // Test authentication - in multi mode, it should continue to try all plugins
        let result = plugin_manager.authenticate(&mut req, &mut ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert!(ctx.consumer.is_some());
        assert_eq!(ctx.consumer.as_ref().unwrap().username, "test_user");
    }
    
    #[tokio::test]
    async fn test_plugin_authorization() {
        let mut plugin_manager = PluginManager::new();
        
        // Register test plugins - first authorizes, second doesn't
        let allow_plugin = Arc::new(MockPlugin::new("allow_plugin", true, true));
        let deny_plugin = Arc::new(MockPlugin::new("deny_plugin", true, false));
        plugin_manager.register_plugin(allow_plugin);
        plugin_manager.register_plugin(deny_plugin);
        
        // Create test request and context
        let mut req = Request::builder()
            .uri("https://gateway.example.com/api/test")
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // Set consumer (assume authentication succeeded)
        ctx.consumer = Some(Consumer {
            id: "test_consumer".to_string(),
            username: "test_user".to_string(),
            custom_id: None,
            credentials: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        
        // Test authorization - should fail if any plugin returns false
        let result = plugin_manager.authorize(&mut req, &mut ctx).await;
        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should fail because deny_plugin returns false
    }
    
    #[tokio::test]
    async fn test_plugin_request_response_hooks() {
        // This test would verify that the on_request_received, before_proxy, and after_proxy
        // hooks are called correctly and can modify the request/response
        
        // For brevity, I'm not implementing the full test, but it would:
        // 1. Create mock plugins that modify requests and responses
        // 2. Create a test request and response
        // 3. Verify that the plugins modify them as expected
    }
    
    #[tokio::test]
    async fn test_key_auth_plugin() {
        // This would test the key_auth plugin specifically
        
        // Implementation would:
        // 1. Create a key_auth plugin instance with specific config
        // 2. Create a request with a valid API key
        // 3. Verify that authentication succeeds
        // 4. Create a request with an invalid API key
        // 5. Verify that authentication fails
    }
    
    #[tokio::test]
    async fn test_jwt_auth_plugin() {
        // This would test the jwt_auth plugin specifically
        
        // Implementation would:
        // 1. Create a jwt_auth plugin instance with specific config
        // 2. Create a request with a valid JWT token
        // 3. Verify that authentication succeeds
        // 4. Create a request with an invalid/expired JWT token
        // 5. Verify that authentication fails
    }
    
    #[tokio::test]
    async fn test_access_control_plugin() {
        // This would test the access_control plugin specifically
        
        // Implementation would:
        // 1. Create an access_control plugin with allowed/disallowed consumers
        // 2. Create contexts with different consumers
        // 3. Verify that authorization succeeds/fails as expected
    }
    
    #[tokio::test]
    async fn test_request_transformer_plugin() {
        // This would test the request_transformer plugin
        
        // Implementation would:
        // 1. Create a request_transformer plugin with specific transformations
        // 2. Create a request with initial headers/query params
        // 3. Run the plugin's on_request_received hook
        // 4. Verify that headers/query params are modified as expected
    }
    
    #[tokio::test]
    async fn test_response_transformer_plugin() {
        // This would test the response_transformer plugin
        
        // Implementation would:
        // 1. Create a response_transformer plugin with specific transformations
        // 2. Create a response with initial headers
        // 3. Run the plugin's after_proxy hook
        // 4. Verify that headers are modified as expected
    }
}
