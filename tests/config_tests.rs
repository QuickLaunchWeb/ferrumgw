#[cfg(test)]
mod config_tests {
    use std::collections::HashMap;
    use std::env;
    use chrono::Utc;
    use tempfile::tempdir;
    use std::fs;
    use std::path::PathBuf;
    
    use ferrumgw::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode};
    use ferrumgw::config::env_config::EnvConfig;
    use ferrumgw::config::file_config::FileConfigLoader;
    
    // Helper function to create a test proxy
    fn create_test_proxy(id: &str, listen_path: &str) -> Proxy {
        Proxy {
            id: id.to_string(),
            name: Some(format!("Test Proxy {}", id)),
            listen_path: listen_path.to_string(),
            backend_protocol: Protocol::Http,
            backend_host: "example.com".to_string(),
            backend_port: 80,
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
    
    #[test]
    fn test_proxy_listen_path_validation() {
        let proxies = vec![
            create_test_proxy("1", "/api"),
            create_test_proxy("2", "/api/users"),
            create_test_proxy("3", "/services"),
        ];
        
        // Test valid configuration (no duplicate paths)
        let config = Configuration {
            proxies: proxies.clone(),
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        assert!(ferrumgw::config::data_model::validate_proxy_listen_paths(&config.proxies).is_ok());
        
        // Test invalid configuration (duplicate listen_path)
        let mut invalid_proxies = proxies.clone();
        invalid_proxies.push(create_test_proxy("4", "/api")); // Duplicate listen_path
        
        let invalid_config = Configuration {
            proxies: invalid_proxies,
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        assert!(ferrumgw::config::data_model::validate_proxy_listen_paths(&invalid_config.proxies).is_err());
    }
    
    #[test]
    fn test_env_config_validation() {
        // Test database mode with required variables
        let mut env_vars = HashMap::new();
        env_vars.insert("FERRUM_MODE".to_string(), "database".to_string());
        env_vars.insert("FERRUM_DB_TYPE".to_string(), "postgres".to_string());
        env_vars.insert("FERRUM_DB_URL".to_string(), "postgres://user:pass@localhost/db".to_string());
        env_vars.insert("FERRUM_ADMIN_JWT_SECRET".to_string(), "secret".to_string());
        
        let result = EnvConfig::from_map(&env_vars);
        assert!(result.is_ok());
        
        // Test database mode with missing required variable
        let mut incomplete_env_vars = env_vars.clone();
        incomplete_env_vars.remove("FERRUM_DB_URL");
        
        let result = EnvConfig::from_map(&incomplete_env_vars);
        assert!(result.is_err());
        
        // Test file mode with required variables
        let mut file_env_vars = HashMap::new();
        file_env_vars.insert("FERRUM_MODE".to_string(), "file".to_string());
        file_env_vars.insert("FERRUM_FILE_CONFIG_PATH".to_string(), "/path/to/config.yaml".to_string());
        
        let result = EnvConfig::from_map(&file_env_vars);
        assert!(result.is_ok());
        
        // Test CP mode with required variables
        let mut cp_env_vars = HashMap::new();
        cp_env_vars.insert("FERRUM_MODE".to_string(), "cp".to_string());
        cp_env_vars.insert("FERRUM_DB_TYPE".to_string(), "postgres".to_string());
        cp_env_vars.insert("FERRUM_DB_URL".to_string(), "postgres://user:pass@localhost/db".to_string());
        cp_env_vars.insert("FERRUM_ADMIN_JWT_SECRET".to_string(), "secret".to_string());
        cp_env_vars.insert("FERRUM_CP_GRPC_LISTEN_ADDR".to_string(), "0.0.0.0:50051".to_string());
        cp_env_vars.insert("FERRUM_CP_GRPC_JWT_SECRET".to_string(), "grpc_secret".to_string());
        
        let result = EnvConfig::from_map(&cp_env_vars);
        assert!(result.is_ok());
        
        // Test DP mode with required variables
        let mut dp_env_vars = HashMap::new();
        dp_env_vars.insert("FERRUM_MODE".to_string(), "dp".to_string());
        dp_env_vars.insert("FERRUM_DP_CP_GRPC_URL".to_string(), "http://cp-host:50051".to_string());
        dp_env_vars.insert("FERRUM_DP_GRPC_AUTH_TOKEN".to_string(), "jwt_token".to_string());
        
        let result = EnvConfig::from_map(&dp_env_vars);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_file_config_loading() {
        // Create temporary directory for test files
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("config.json");
        
        // Create test configuration file
        let config_json = r#"{
            "proxies": [
                {
                    "id": "1",
                    "name": "Test API",
                    "listen_path": "/api",
                    "backend_protocol": "http",
                    "backend_host": "example.com",
                    "backend_port": 80,
                    "backend_path": "/backend",
                    "strip_listen_path": true,
                    "preserve_host_header": false,
                    "backend_connect_timeout_ms": 5000,
                    "backend_read_timeout_ms": 30000,
                    "backend_write_timeout_ms": 30000,
                    "auth_mode": "single"
                }
            ],
            "consumers": [
                {
                    "id": "1",
                    "username": "testuser",
                    "credentials": {
                        "keyauth": {
                            "key": "testkey"
                        }
                    }
                }
            ],
            "plugin_configs": [
                {
                    "id": "1",
                    "plugin_name": "key_auth",
                    "config": {
                        "key_location": "header",
                        "header_name": "X-API-Key"
                    },
                    "scope": "proxy",
                    "proxy_id": "1",
                    "enabled": true
                }
            ]
        }"#;
        
        fs::write(&config_path, config_json).unwrap();
        
        // Test loading from file
        let loader = FileConfigLoader::new(&config_path.to_string_lossy());
        let config = loader.load_config().unwrap();
        
        assert_eq!(config.proxies.len(), 1);
        assert_eq!(config.proxies[0].id, "1");
        assert_eq!(config.proxies[0].listen_path, "/api");
        
        assert_eq!(config.consumers.len(), 1);
        assert_eq!(config.consumers[0].username, "testuser");
        
        assert_eq!(config.plugin_configs.len(), 1);
        assert_eq!(config.plugin_configs[0].plugin_name, "key_auth");
    }
}
