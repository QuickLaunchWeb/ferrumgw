#[cfg(test)]
mod auth_plugin_tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use chrono::Utc;
    use http::{HeaderMap, Request, Response, StatusCode};
    use hyper::Body;
    use tokio::sync::RwLock;
    use serde_json::json;
    
    use ferrumgw::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode};
    use ferrumgw::proxy::handler::RequestContext;
    use ferrumgw::plugins::{Plugin, PluginManager};
    use ferrumgw::plugins::jwt_auth::{JwtAuthPlugin, JwtAuthConfig};
    use ferrumgw::plugins::key_auth::{KeyAuthPlugin, KeyAuthConfig};
    use ferrumgw::plugins::basic_auth::{BasicAuthPlugin, BasicAuthConfig};
    use ferrumgw::plugins::oauth2_auth::{OAuth2AuthPlugin, OAuth2AuthConfig};
    
    // Helper function to create a test configuration with a test consumer
    fn create_test_config() -> Configuration {
        let mut credentials = HashMap::new();
        
        // Add a basic auth credential
        credentials.insert("basicauth".to_string(), 
            json!({
                "username": "testuser",
                // This is "password" hashed with bcrypt
                "password": "$2y$12$K8JVl.U7DQHfI3oG.nY2/eAUVCZDOs9ZQjOzlRQoiHGQ0k2ljsaKS" 
            })
        );
        
        // Add a key auth credential
        credentials.insert("keyauth".to_string(), 
            json!({
                "key": "test-api-key"
            })
        );
        
        // Add JWT credentials
        credentials.insert("jwtauth".to_string(), 
            json!({
                "subject": "test-user"
            })
        );
        
        // Add OAuth2 credentials
        credentials.insert("oauth2".to_string(), 
            json!({
                "provider": "github",
                "provider_user_id": "user123"
            })
        );
        
        let consumer = Consumer {
            id: "consumer1".to_string(),
            username: "testuser".to_string(),
            custom_id: Some("custom1".to_string()),
            credentials,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        Configuration {
            proxies: Vec::new(),
            consumers: vec![consumer],
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        }
    }
    
    // Helper to create a test request context
    fn create_test_context() -> RequestContext {
        RequestContext {
            proxy: None,
            consumer: None,
            authenticated: false,
            authorized: false,
            request_timestamp: Utc::now(),
            response_timestamp: None,
            response_status: None,
            request_size: 0,
            response_size: 0,
            error: None,
        }
    }
    
    #[tokio::test]
    async fn test_jwt_auth_verification() {
        // Create a test configuration with our consumer
        let config = create_test_config();
        let shared_config = Arc::new(RwLock::new(config));
        
        // Create JWT auth config
        let jwt_config = JwtAuthConfig {
            token_lookup: "header".to_string(),
            header_name: "Authorization".to_string(),
            header_value_prefix: "Bearer ".to_string(),
            query_param_name: None,
            cookie_name: None,
            secret: "test-jwt-secret".to_string(),
            public_key: None,
            algorithm: "HS256".to_string(),
            consumer_claim_field: "sub".to_string(),
            issuer: None,
            audience: None,
            ignore_expiration: false,
        };
        
        // Create JWT auth plugin
        let plugin = JwtAuthPlugin::new(
            jwt_config,
            shared_config.clone(),
        );
        
        // Test with a valid JWT
        // Generate a valid JWT using jsonwebtoken
        use jsonwebtoken::{encode, Header, EncodingKey, Algorithm};
        
        let claims = json!({
            "sub": "test-user",
            "exp": Utc::now().timestamp() + 3600,
            "iat": Utc::now().timestamp(),
        });
        
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret("test-jwt-secret".as_bytes()),
        ).unwrap();
        
        // Create a request with the JWT
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", format!("Bearer {}", token).parse().unwrap());
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/test")
            .headers(headers)
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // Test the plugin
        let result = plugin.authenticate(&mut req.clone(), &mut ctx).await;
        assert!(result.is_ok(), "Authentication failed: {:?}", result);
        assert!(result.unwrap(), "Authentication should succeed");
        assert!(ctx.authenticated);
        assert!(ctx.consumer.is_some());
        assert_eq!(ctx.consumer.as_ref().unwrap().id, "consumer1");
        
        // Test with invalid JWT
        let invalid_token = "invalid.jwt.token";
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", format!("Bearer {}", invalid_token).parse().unwrap());
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/test")
            .headers(headers)
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // This should not authenticate but not error out
        let result = plugin.authenticate(&mut req.clone(), &mut ctx).await;
        assert!(result.is_ok(), "Authentication failed unexpectedly: {:?}", result);
        assert!(!result.unwrap(), "Authentication should fail");
        assert!(!ctx.authenticated);
        assert!(ctx.consumer.is_none());
    }
    
    #[tokio::test]
    async fn test_key_auth_verification() {
        // Create a test configuration with our consumer
        let config = create_test_config();
        let shared_config = Arc::new(RwLock::new(config));
        
        // Create key auth config
        let key_config = KeyAuthConfig {
            key_location: "header".to_string(),
            header_name: "X-API-Key".to_string(),
            query_param_name: None,
            cookie_name: None,
        };
        
        // Create key auth plugin
        let plugin = KeyAuthPlugin::new(
            key_config,
            shared_config.clone(),
        );
        
        // Test with valid API key
        let mut headers = HeaderMap::new();
        headers.insert("X-API-Key", "test-api-key".parse().unwrap());
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/test")
            .headers(headers)
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // Test the plugin
        let result = plugin.authenticate(&mut req.clone(), &mut ctx).await;
        assert!(result.is_ok(), "Authentication failed: {:?}", result);
        assert!(result.unwrap(), "Authentication should succeed");
        assert!(ctx.authenticated);
        assert!(ctx.consumer.is_some());
        assert_eq!(ctx.consumer.as_ref().unwrap().id, "consumer1");
        
        // Test with invalid API key
        let mut headers = HeaderMap::new();
        headers.insert("X-API-Key", "invalid-api-key".parse().unwrap());
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/test")
            .headers(headers)
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // This should not authenticate but not error out
        let result = plugin.authenticate(&mut req.clone(), &mut ctx).await;
        assert!(result.is_ok(), "Authentication failed unexpectedly: {:?}", result);
        assert!(!result.unwrap(), "Authentication should fail");
        assert!(!ctx.authenticated);
        assert!(ctx.consumer.is_none());
    }
    
    #[tokio::test]
    async fn test_basic_auth_verification() {
        // Create a test configuration with our consumer
        let config = create_test_config();
        let shared_config = Arc::new(RwLock::new(config));
        
        // Create basic auth config
        let basic_config = BasicAuthConfig {
            realm: "Test Realm".to_string(),
        };
        
        // Create basic auth plugin
        let plugin = BasicAuthPlugin::new(
            basic_config,
            shared_config.clone(),
        );
        
        // Test with valid Basic Auth
        // Base64 encode "testuser:password"
        let auth_value = format!("Basic {}", base64::encode("testuser:password"));
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", auth_value.parse().unwrap());
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/test")
            .headers(headers)
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // Test the plugin
        let result = plugin.authenticate(&mut req.clone(), &mut ctx).await;
        assert!(result.is_ok(), "Authentication failed: {:?}", result);
        assert!(result.unwrap(), "Authentication should succeed");
        assert!(ctx.authenticated);
        assert!(ctx.consumer.is_some());
        assert_eq!(ctx.consumer.as_ref().unwrap().id, "consumer1");
        
        // Test with invalid credentials
        let auth_value = format!("Basic {}", base64::encode("testuser:wrongpassword"));
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", auth_value.parse().unwrap());
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/test")
            .headers(headers)
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // This should not authenticate but not error out
        let result = plugin.authenticate(&mut req.clone(), &mut ctx).await;
        assert!(result.is_ok(), "Authentication failed unexpectedly: {:?}", result);
        assert!(!result.unwrap(), "Authentication should fail");
        assert!(!ctx.authenticated);
        assert!(ctx.consumer.is_none());
    }
    
    #[tokio::test]
    async fn test_oauth2_auth_verification() {
        // This test would typically mock external OAuth2 introspection
        // For now, we'll just test the consumer matching logic
        
        // Create a test configuration with our consumer
        let config = create_test_config();
        let shared_config = Arc::new(RwLock::new(config));
        
        // Create OAuth2 auth config (token matching mode without introspection)
        let oauth2_config = OAuth2AuthConfig {
            validation_mode: "token_match".to_string(),
            token_lookup: "header".to_string(),
            header_name: "Authorization".to_string(),
            header_value_prefix: "Bearer ".to_string(),
            query_param_name: None,
            cookie_name: None,
            introspection_url: None,
            introspection_client_id: None,
            introspection_client_secret: None,
            consumer_claim_field: "provider_user_id".to_string(),
            provider_name: "github".to_string(),
        };
        
        // Create OAuth2 auth plugin
        let plugin = OAuth2AuthPlugin::new(
            oauth2_config,
            shared_config.clone(),
        );
        
        // In token_match mode, the plugin directly matches the token to a consumer
        // So we'll simulate a token representing the github user "user123"
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", "Bearer user123".parse().unwrap());
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/test")
            .headers(headers)
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // Test the plugin
        let result = plugin.authenticate(&mut req.clone(), &mut ctx).await;
        assert!(result.is_ok(), "Authentication failed: {:?}", result);
        assert!(result.unwrap(), "Authentication should succeed");
        assert!(ctx.authenticated);
        assert!(ctx.consumer.is_some());
        assert_eq!(ctx.consumer.as_ref().unwrap().id, "consumer1");
        
        // Test with invalid token
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", "Bearer invaliduser".parse().unwrap());
        
        let req = Request::builder()
            .method("GET")
            .uri("/api/test")
            .headers(headers)
            .body(Body::empty())
            .unwrap();
        
        let mut ctx = create_test_context();
        
        // This should not authenticate but not error out
        let result = plugin.authenticate(&mut req.clone(), &mut ctx).await;
        assert!(result.is_ok(), "Authentication failed unexpectedly: {:?}", result);
        assert!(!result.unwrap(), "Authentication should fail");
        assert!(!ctx.authenticated);
        assert!(ctx.consumer.is_none());
    }
}
