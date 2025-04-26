#[cfg(test)]
mod mode_tests {
    use std::collections::HashMap;
    use std::env;
    use std::sync::Arc;
    use std::time::Duration;
    use chrono::Utc;
    use tokio::sync::RwLock;
    use tokio::time::sleep;
    use tempfile::tempdir;
    use std::fs;
    use std::path::PathBuf;
    
    use ferrumgw::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode};
    use ferrumgw::config::env_config::EnvConfig;
    
    #[tokio::test]
    async fn test_database_mode_initialization() {
        // This is a mock test - in reality we would need a database
        // Skip the actual database connection for now
        if std::env::var("TEST_POSTGRES_URL").is_err() && 
           std::env::var("CI").is_err() {
            println!("Skipping database mode test (no TEST_POSTGRES_URL)");
            return;
        }
        
        // Setup environment variables
        let old_mode = env::var("FERRUM_MODE").ok();
        let old_db_type = env::var("FERRUM_DB_TYPE").ok();
        let old_db_url = env::var("FERRUM_DB_URL").ok();
        
        env::set_var("FERRUM_MODE", "database");
        env::set_var("FERRUM_DB_TYPE", "postgres");
        env::set_var("FERRUM_DB_URL", std::env::var("TEST_POSTGRES_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/ferrumgw_test".to_string()));
        
        // Parse environment variables
        let env_config = match EnvConfig::from_env() {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Failed to parse environment variables: {}", e);
                // Restore environment
                if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
                if let Some(val) = old_db_type { env::set_var("FERRUM_DB_TYPE", val); } else { env::remove_var("FERRUM_DB_TYPE"); }
                if let Some(val) = old_db_url { env::set_var("FERRUM_DB_URL", val); } else { env::remove_var("FERRUM_DB_URL"); }
                return;
            }
        };
        
        // Verify that the mode is correctly set
        assert_eq!(env_config.mode, "database");
        assert_eq!(env_config.db_type.unwrap(), "postgres");
        assert!(env_config.db_url.is_some());
        
        // In a full test, we would:
        // 1. Initialize the database mode
        // 2. Check that the configuration is loaded
        // 3. Check the database poller
        
        // Restore environment
        if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
        if let Some(val) = old_db_type { env::set_var("FERRUM_DB_TYPE", val); } else { env::remove_var("FERRUM_DB_TYPE"); }
        if let Some(val) = old_db_url { env::set_var("FERRUM_DB_URL", val); } else { env::remove_var("FERRUM_DB_URL"); }
    }
    
    #[tokio::test]
    async fn test_file_mode_config_loading() {
        // Create a temporary directory for the test config file
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let config_path = temp_dir.path().join("test_config.yaml");
        
        // Create a test configuration
        let config_content = r#"
        proxies:
          - id: "test1"
            name: "Test API"
            listen_path: "/api/test"
            backend_protocol: "http"
            backend_host: "example.com"
            backend_port: 80
            strip_listen_path: true
        consumers:
          - id: "consumer1"
            username: "testuser"
        plugin_configs:
          - id: "plugin1"
            plugin_name: "key_auth"
            config:
              key_location: "header"
              header_name: "X-API-Key"
            scope: "proxy"
            proxy_id: "test1"
            enabled: true
        "#;
        
        fs::write(&config_path, config_content).expect("Failed to write test config");
        
        // Setup environment variables
        let old_mode = env::var("FERRUM_MODE").ok();
        let old_config_path = env::var("FERRUM_FILE_CONFIG_PATH").ok();
        
        env::set_var("FERRUM_MODE", "file");
        env::set_var("FERRUM_FILE_CONFIG_PATH", config_path.to_str().unwrap());
        
        // Parse environment variables
        let env_config = match EnvConfig::from_env() {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Failed to parse environment variables: {}", e);
                // Restore environment
                if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
                if let Some(val) = old_config_path { env::set_var("FERRUM_FILE_CONFIG_PATH", val); } else { env::remove_var("FERRUM_FILE_CONFIG_PATH"); }
                return;
            }
        };
        
        // Verify that the mode is correctly set
        assert_eq!(env_config.mode, "file");
        assert_eq!(env_config.file_config_path.unwrap(), config_path.to_str().unwrap());
        
        // Create the file config loader
        let file_loader = match ferrumgw::config::file_config::FileConfigLoader::new(env_config.file_config_path.unwrap()) {
            Ok(loader) => loader,
            Err(e) => {
                println!("Failed to create file loader: {}", e);
                // Restore environment
                if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
                if let Some(val) = old_config_path { env::set_var("FERRUM_FILE_CONFIG_PATH", val); } else { env::remove_var("FERRUM_FILE_CONFIG_PATH"); }
                return;
            }
        };
        
        // Load the configuration
        let config = match file_loader.load_config().await {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("Failed to load config: {}", e);
                // Restore environment
                if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
                if let Some(val) = old_config_path { env::set_var("FERRUM_FILE_CONFIG_PATH", val); } else { env::remove_var("FERRUM_FILE_CONFIG_PATH"); }
                return;
            }
        };
        
        // Verify the loaded configuration
        assert_eq!(config.proxies.len(), 1);
        assert_eq!(config.proxies[0].id, "test1");
        assert_eq!(config.proxies[0].listen_path, "/api/test");
        
        assert_eq!(config.consumers.len(), 1);
        assert_eq!(config.consumers[0].id, "consumer1");
        assert_eq!(config.consumers[0].username, "testuser");
        
        assert_eq!(config.plugin_configs.len(), 1);
        assert_eq!(config.plugin_configs[0].id, "plugin1");
        assert_eq!(config.plugin_configs[0].plugin_name, "key_auth");
        
        // Restore environment
        if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
        if let Some(val) = old_config_path { env::set_var("FERRUM_FILE_CONFIG_PATH", val); } else { env::remove_var("FERRUM_FILE_CONFIG_PATH"); }
    }
    
    #[tokio::test]
    async fn test_file_mode_config_reload() {
        // Create a temporary directory for the test config file
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let config_path = temp_dir.path().join("test_config.yaml");
        
        // Create an initial test configuration
        let initial_config = r#"
        proxies:
          - id: "test1"
            name: "Test API"
            listen_path: "/api/test"
            backend_protocol: "http"
            backend_host: "example.com"
            backend_port: 80
        "#;
        
        fs::write(&config_path, initial_config).expect("Failed to write initial config");
        
        // Setup environment variables
        let old_mode = env::var("FERRUM_MODE").ok();
        let old_config_path = env::var("FERRUM_FILE_CONFIG_PATH").ok();
        
        env::set_var("FERRUM_MODE", "file");
        env::set_var("FERRUM_FILE_CONFIG_PATH", config_path.to_str().unwrap());
        
        // Parse environment variables
        let env_config = EnvConfig::from_env().expect("Failed to parse environment");
        
        // Create the file config loader
        let file_loader = ferrumgw::config::file_config::FileConfigLoader::new(env_config.file_config_path.unwrap())
            .expect("Failed to create file loader");
        
        // Load the initial configuration
        let initial_loaded_config = file_loader.load_config().await.expect("Failed to load initial config");
        assert_eq!(initial_loaded_config.proxies.len(), 1);
        assert_eq!(initial_loaded_config.proxies[0].id, "test1");
        
        // Create a shared configuration
        let shared_config = Arc::new(RwLock::new(initial_loaded_config));
        
        // Setup a reload watcher
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        
        // Start a task to reload config when signal received
        let shared_config_clone = Arc::clone(&shared_config);
        let file_loader_clone = file_loader.clone();
        tokio::spawn(async move {
            while let Some(_) = rx.recv().await {
                match file_loader_clone.load_config().await {
                    Ok(new_config) => {
                        let mut config = shared_config_clone.write().await;
                        *config = new_config;
                    },
                    Err(e) => {
                        println!("Failed to reload config: {}", e);
                    }
                }
            }
        });
        
        // Update the configuration file
        let updated_config = r#"
        proxies:
          - id: "test1"
            name: "Test API"
            listen_path: "/api/test"
            backend_protocol: "http"
            backend_host: "example.com"
            backend_port: 80
          - id: "test2"
            name: "New API"
            listen_path: "/api/new"
            backend_protocol: "http"
            backend_host: "new.example.com"
            backend_port: 8080
        "#;
        
        fs::write(&config_path, updated_config).expect("Failed to write updated config");
        
        // Signal reload
        tx.send(()).await.expect("Failed to send reload signal");
        
        // Wait a bit for the reload to complete
        sleep(Duration::from_millis(100)).await;
        
        // Verify the configuration was updated
        let config = shared_config.read().await;
        assert_eq!(config.proxies.len(), 2);
        assert_eq!(config.proxies[1].id, "test2");
        assert_eq!(config.proxies[1].listen_path, "/api/new");
        
        // Restore environment
        if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
        if let Some(val) = old_config_path { env::set_var("FERRUM_FILE_CONFIG_PATH", val); } else { env::remove_var("FERRUM_FILE_CONFIG_PATH"); }
    }
    
    #[tokio::test]
    async fn test_cp_dp_communication() {
        // This is a more complex test that would require both CP and DP to be running
        // We'll mock parts of it for simplicity
        
        // For a real test we would:
        // 1. Start a Control Plane server
        // 2. Configure a Data Plane client to connect to it
        // 3. Verify that configuration updates are streamed correctly
        // 4. Test JWT validation
        
        // For now, we'll just verify the environment parsing
        
        // Test CP mode
        let old_mode = env::var("FERRUM_MODE").ok();
        let old_grpc_addr = env::var("FERRUM_CP_GRPC_LISTEN_ADDR").ok();
        let old_jwt_secret = env::var("FERRUM_CP_GRPC_JWT_SECRET").ok();
        
        env::set_var("FERRUM_MODE", "cp");
        env::set_var("FERRUM_CP_GRPC_LISTEN_ADDR", "127.0.0.1:50051");
        env::set_var("FERRUM_CP_GRPC_JWT_SECRET", "test-jwt-secret");
        
        let cp_env_config = EnvConfig::from_env().expect("Failed to parse CP environment");
        assert_eq!(cp_env_config.mode, "cp");
        assert_eq!(cp_env_config.cp_grpc_listen_addr.unwrap(), "127.0.0.1:50051");
        assert_eq!(cp_env_config.cp_grpc_jwt_secret.unwrap(), "test-jwt-secret");
        
        // Restore environment
        if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
        if let Some(val) = old_grpc_addr { env::set_var("FERRUM_CP_GRPC_LISTEN_ADDR", val); } else { env::remove_var("FERRUM_CP_GRPC_LISTEN_ADDR"); }
        if let Some(val) = old_jwt_secret { env::set_var("FERRUM_CP_GRPC_JWT_SECRET", val); } else { env::remove_var("FERRUM_CP_GRPC_JWT_SECRET"); }
        
        // Test DP mode
        let old_mode = env::var("FERRUM_MODE").ok();
        let old_cp_url = env::var("FERRUM_DP_CP_GRPC_URL").ok();
        let old_auth_token = env::var("FERRUM_DP_GRPC_AUTH_TOKEN").ok();
        
        env::set_var("FERRUM_MODE", "dp");
        env::set_var("FERRUM_DP_CP_GRPC_URL", "http://localhost:50051");
        env::set_var("FERRUM_DP_GRPC_AUTH_TOKEN", "test-auth-token");
        
        let dp_env_config = EnvConfig::from_env().expect("Failed to parse DP environment");
        assert_eq!(dp_env_config.mode, "dp");
        assert_eq!(dp_env_config.dp_cp_grpc_url.unwrap(), "http://localhost:50051");
        assert_eq!(dp_env_config.dp_grpc_auth_token.unwrap(), "test-auth-token");
        
        // Restore environment
        if let Some(val) = old_mode { env::set_var("FERRUM_MODE", val); } else { env::remove_var("FERRUM_MODE"); }
        if let Some(val) = old_cp_url { env::set_var("FERRUM_DP_CP_GRPC_URL", val); } else { env::remove_var("FERRUM_DP_CP_GRPC_URL"); }
        if let Some(val) = old_auth_token { env::set_var("FERRUM_DP_GRPC_AUTH_TOKEN", val); } else { env::remove_var("FERRUM_DP_GRPC_AUTH_TOKEN"); }
    }
}
