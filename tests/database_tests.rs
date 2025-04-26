#[cfg(test)]
mod database_tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use chrono::Utc;
    use tokio::sync::RwLock;
    
    use ferrumgw::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode};
    use ferrumgw::database::{DatabaseAdapter, Error as DbError};
    
    // We'll conditionally compile these tests when their respective features are enabled
    #[cfg(feature = "postgres")]
    mod postgres_tests {
        use super::*;
        use ferrumgw::database::postgres::PostgresAdapter;
        use sqlx::postgres::PgPoolOptions;
        
        // Helper function to create a test database client
        async fn create_test_client() -> Result<PostgresAdapter, anyhow::Error> {
            // This assumes you have a test database running with appropriate credentials
            // In a CI environment, you'd use a Docker container with a fresh database
            let db_url = std::env::var("TEST_POSTGRES_URL")
                .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/ferrumgw_test".to_string());
            
            // Create connection pool
            let pool = PgPoolOptions::new()
                .max_connections(5)
                .connect(&db_url).await?;
            
            // Clear any existing test data
            sqlx::query("TRUNCATE proxies, consumers, plugin_configs CASCADE")
                .execute(&pool).await?;
            
            Ok(PostgresAdapter::new(pool))
        }
        
        #[tokio::test]
        async fn test_postgres_proxy_crud() {
            // Skip if we don't have a test database
            if std::env::var("TEST_POSTGRES_URL").is_err() && 
               std::env::var("CI").is_err() {
                println!("Skipping PostgreSQL test (no TEST_POSTGRES_URL)");
                return;
            }
            
            let client = match create_test_client().await {
                Ok(c) => c,
                Err(e) => {
                    println!("Skipping PostgreSQL test: {}", e);
                    return;
                }
            };
            
            // Test proxy CRUD operations
            let proxy = Proxy {
                id: "test1".to_string(),
                name: Some("Test Proxy".to_string()),
                listen_path: "/api/test".to_string(),
                backend_protocol: Protocol::Http,
                backend_host: "example.com".to_string(),
                backend_port: 80,
                backend_path: Some("/backend".to_string()),
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
            
            // CREATE
            let result = client.create_proxy(&proxy).await;
            assert!(result.is_ok(), "Failed to create proxy: {:?}", result);
            
            // READ
            let fetched = client.get_proxy("test1").await;
            assert!(fetched.is_ok(), "Failed to get proxy: {:?}", fetched);
            let fetched_proxy = fetched.unwrap();
            assert_eq!(fetched_proxy.id, proxy.id);
            assert_eq!(fetched_proxy.listen_path, proxy.listen_path);
            
            // UPDATE
            let mut updated_proxy = proxy.clone();
            updated_proxy.name = Some("Updated Test Proxy".to_string());
            updated_proxy.backend_port = 8080;
            let update_result = client.update_proxy(&updated_proxy).await;
            assert!(update_result.is_ok(), "Failed to update proxy: {:?}", update_result);
            
            // Verify the update
            let fetched_again = client.get_proxy("test1").await.unwrap();
            assert_eq!(fetched_again.name, Some("Updated Test Proxy".to_string()));
            assert_eq!(fetched_again.backend_port, 8080);
            
            // LIST
            let all_proxies = client.list_proxies().await;
            assert!(all_proxies.is_ok(), "Failed to list proxies: {:?}", all_proxies);
            assert_eq!(all_proxies.unwrap().len(), 1);
            
            // DELETE
            let delete_result = client.delete_proxy("test1").await;
            assert!(delete_result.is_ok(), "Failed to delete proxy: {:?}", delete_result);
            
            // Verify deletion
            let after_delete = client.get_proxy("test1").await;
            assert!(after_delete.is_err());
            assert!(matches!(after_delete, Err(DbError::NotFound)));
        }
        
        #[tokio::test]
        async fn test_postgres_consumer_crud() {
            // Skip if we don't have a test database
            if std::env::var("TEST_POSTGRES_URL").is_err() && 
               std::env::var("CI").is_err() {
                println!("Skipping PostgreSQL test (no TEST_POSTGRES_URL)");
                return;
            }
            
            let client = match create_test_client().await {
                Ok(c) => c,
                Err(e) => {
                    println!("Skipping PostgreSQL test: {}", e);
                    return;
                }
            };
            
            // Test consumer CRUD operations
            let mut credentials = HashMap::new();
            credentials.insert("keyauth".to_string(), 
                               serde_json::json!({ "key": "test-api-key" }));
            
            let consumer = Consumer {
                id: "consumer1".to_string(),
                username: "testuser".to_string(),
                custom_id: Some("custom1".to_string()),
                credentials,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            
            // CREATE
            let result = client.create_consumer(&consumer).await;
            assert!(result.is_ok(), "Failed to create consumer: {:?}", result);
            
            // READ
            let fetched = client.get_consumer("consumer1").await;
            assert!(fetched.is_ok(), "Failed to get consumer: {:?}", fetched);
            let fetched_consumer = fetched.unwrap();
            assert_eq!(fetched_consumer.id, consumer.id);
            assert_eq!(fetched_consumer.username, consumer.username);
            
            // Verify credentials were stored
            assert!(fetched_consumer.credentials.contains_key("keyauth"));
            
            // UPDATE
            let mut updated_consumer = consumer.clone();
            updated_consumer.username = "updated_user".to_string();
            let update_result = client.update_consumer(&updated_consumer).await;
            assert!(update_result.is_ok(), "Failed to update consumer: {:?}", update_result);
            
            // Verify the update
            let fetched_again = client.get_consumer("consumer1").await.unwrap();
            assert_eq!(fetched_again.username, "updated_user");
            
            // LIST
            let all_consumers = client.list_consumers().await;
            assert!(all_consumers.is_ok(), "Failed to list consumers: {:?}", all_consumers);
            assert_eq!(all_consumers.unwrap().len(), 1);
            
            // DELETE
            let delete_result = client.delete_consumer("consumer1").await;
            assert!(delete_result.is_ok(), "Failed to delete consumer: {:?}", delete_result);
            
            // Verify deletion
            let after_delete = client.get_consumer("consumer1").await;
            assert!(after_delete.is_err());
            assert!(matches!(after_delete, Err(DbError::NotFound)));
        }
        
        #[tokio::test]
        async fn test_postgres_plugin_config_crud() {
            // Skip if we don't have a test database
            if std::env::var("TEST_POSTGRES_URL").is_err() && 
               std::env::var("CI").is_err() {
                println!("Skipping PostgreSQL test (no TEST_POSTGRES_URL)");
                return;
            }
            
            let client = match create_test_client().await {
                Ok(c) => c,
                Err(e) => {
                    println!("Skipping PostgreSQL test: {}", e);
                    return;
                }
            };
            
            // First create a proxy since we'll reference it
            let proxy = Proxy {
                id: "test-proxy".to_string(),
                name: Some("Test Proxy".to_string()),
                listen_path: "/api/test".to_string(),
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
            
            let _ = client.create_proxy(&proxy).await.unwrap();
            
            // Test plugin config CRUD operations
            let plugin_config = PluginConfig {
                id: "plugin1".to_string(),
                plugin_name: "key_auth".to_string(),
                config: serde_json::json!({
                    "key_location": "header",
                    "header_name": "X-API-Key"
                }),
                scope: "proxy".to_string(),
                proxy_id: Some("test-proxy".to_string()),
                consumer_id: None,
                enabled: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            
            // CREATE
            let result = client.create_plugin_config(&plugin_config).await;
            assert!(result.is_ok(), "Failed to create plugin config: {:?}", result);
            
            // READ
            let fetched = client.get_plugin_config("plugin1").await;
            assert!(fetched.is_ok(), "Failed to get plugin config: {:?}", fetched);
            let fetched_config = fetched.unwrap();
            assert_eq!(fetched_config.id, plugin_config.id);
            assert_eq!(fetched_config.plugin_name, plugin_config.plugin_name);
            
            // UPDATE
            let mut updated_config = plugin_config.clone();
            updated_config.enabled = false;
            let update_result = client.update_plugin_config(&updated_config).await;
            assert!(update_result.is_ok(), "Failed to update plugin config: {:?}", update_result);
            
            // Verify the update
            let fetched_again = client.get_plugin_config("plugin1").await.unwrap();
            assert_eq!(fetched_again.enabled, false);
            
            // LIST
            let all_configs = client.list_plugin_configs().await;
            assert!(all_configs.is_ok(), "Failed to list plugin configs: {:?}", all_configs);
            assert_eq!(all_configs.unwrap().len(), 1);
            
            // DELETE
            let delete_result = client.delete_plugin_config("plugin1").await;
            assert!(delete_result.is_ok(), "Failed to delete plugin config: {:?}", delete_result);
            
            // Verify deletion
            let after_delete = client.get_plugin_config("plugin1").await;
            assert!(after_delete.is_err());
            assert!(matches!(after_delete, Err(DbError::NotFound)));
        }
        
        #[tokio::test]
        async fn test_postgres_uniqueness_constraints() {
            // Skip if we don't have a test database
            if std::env::var("TEST_POSTGRES_URL").is_err() && 
               std::env::var("CI").is_err() {
                println!("Skipping PostgreSQL test (no TEST_POSTGRES_URL)");
                return;
            }
            
            let client = match create_test_client().await {
                Ok(c) => c,
                Err(e) => {
                    println!("Skipping PostgreSQL test: {}", e);
                    return;
                }
            };
            
            // Create a proxy
            let proxy1 = Proxy {
                id: "test1".to_string(),
                name: Some("Test Proxy 1".to_string()),
                listen_path: "/api/test".to_string(),
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
            
            let _ = client.create_proxy(&proxy1).await.unwrap();
            
            // Try to create another proxy with the same listen_path
            let proxy2 = Proxy {
                id: "test2".to_string(),
                name: Some("Test Proxy 2".to_string()),
                listen_path: "/api/test".to_string(), // Same as proxy1
                backend_protocol: Protocol::Http,
                backend_host: "other.example.com".to_string(),
                backend_port: 8080,
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
            
            // This should fail due to uniqueness constraint on listen_path
            let result = client.create_proxy(&proxy2).await;
            assert!(result.is_err());
            assert!(matches!(result, Err(DbError::AlreadyExists(_))));
            
            // Test consumer username uniqueness
            let consumer1 = Consumer {
                id: "c1".to_string(),
                username: "testuser".to_string(),
                custom_id: None,
                credentials: HashMap::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            
            let _ = client.create_consumer(&consumer1).await.unwrap();
            
            // Try to create another consumer with the same username
            let consumer2 = Consumer {
                id: "c2".to_string(),
                username: "testuser".to_string(), // Same as consumer1
                custom_id: None,
                credentials: HashMap::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            
            // This should fail due to uniqueness constraint on username
            let result = client.create_consumer(&consumer2).await;
            assert!(result.is_err());
            assert!(matches!(result, Err(DbError::AlreadyExists(_))));
        }
    }
    
    #[cfg(feature = "mysql")]
    mod mysql_tests {
        use super::*;
        use ferrumgw::database::mysql::MySQLAdapter;
        use sqlx::mysql::MySqlPoolOptions;
        
        // Helper function to create a test database client
        async fn create_test_client() -> Result<MySQLAdapter, anyhow::Error> {
            // This assumes you have a test database running with appropriate credentials
            let db_url = std::env::var("TEST_MYSQL_URL")
                .unwrap_or_else(|_| "mysql://root:password@localhost/ferrumgw_test".to_string());
            
            // Create connection pool
            let pool = MySqlPoolOptions::new()
                .max_connections(5)
                .connect(&db_url).await?;
            
            // Clear any existing test data
            sqlx::query("TRUNCATE TABLE proxies")
                .execute(&pool).await?;
            sqlx::query("TRUNCATE TABLE consumers")
                .execute(&pool).await?;
            sqlx::query("TRUNCATE TABLE plugin_configs")
                .execute(&pool).await?;
            
            Ok(MySQLAdapter::new(pool))
        }
        
        #[tokio::test]
        async fn test_mysql_proxy_crud() {
            // Skip if we don't have a test database
            if std::env::var("TEST_MYSQL_URL").is_err() && 
               std::env::var("CI").is_err() {
                println!("Skipping MySQL test (no TEST_MYSQL_URL)");
                return;
            }
            
            let client = match create_test_client().await {
                Ok(c) => c,
                Err(e) => {
                    println!("Skipping MySQL test: {}", e);
                    return;
                }
            };
            
            // Test proxy CRUD operations
            let proxy = Proxy {
                id: "test1".to_string(),
                name: Some("Test Proxy".to_string()),
                listen_path: "/api/test".to_string(),
                backend_protocol: Protocol::Http,
                backend_host: "example.com".to_string(),
                backend_port: 80,
                backend_path: Some("/backend".to_string()),
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
            
            // CREATE
            let result = client.create_proxy(&proxy).await;
            assert!(result.is_ok(), "Failed to create proxy: {:?}", result);
            
            // READ
            let fetched = client.get_proxy("test1").await;
            assert!(fetched.is_ok(), "Failed to get proxy: {:?}", fetched);
            let fetched_proxy = fetched.unwrap();
            assert_eq!(fetched_proxy.id, proxy.id);
            assert_eq!(fetched_proxy.listen_path, proxy.listen_path);
            
            // Additional MySQL-specific tests as needed
        }
    }
    
    #[cfg(feature = "sqlite")]
    mod sqlite_tests {
        use super::*;
        use ferrumgw::database::sqlite::SQLiteAdapter;
        use sqlx::sqlite::SqlitePoolOptions;
        use std::path::Path;
        use tempfile::tempdir;
        
        // Helper function to create a test database client with temp file
        async fn create_test_client() -> Result<(SQLiteAdapter, tempfile::TempDir), anyhow::Error> {
            // Create a temporary directory for the SQLite database
            let temp_dir = tempdir()?;
            let db_path = temp_dir.path().join("test_db.sqlite");
            let db_url = format!("sqlite:{}", db_path.display());
            
            // Create connection pool
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect(&db_url).await?;
            
            // Run migrations or create tables
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS proxies (
                    id TEXT PRIMARY KEY,
                    name TEXT,
                    listen_path TEXT NOT NULL UNIQUE,
                    data TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                )"
            ).execute(&pool).await?;
            
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS consumers (
                    id TEXT PRIMARY KEY,
                    username TEXT NOT NULL UNIQUE,
                    custom_id TEXT UNIQUE,
                    data TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                )"
            ).execute(&pool).await?;
            
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS plugin_configs (
                    id TEXT PRIMARY KEY,
                    plugin_name TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    proxy_id TEXT,
                    consumer_id TEXT,
                    enabled INTEGER NOT NULL,
                    config TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                )"
            ).execute(&pool).await?;
            
            Ok((SQLiteAdapter::new(pool), temp_dir))
        }
        
        #[tokio::test]
        async fn test_sqlite_proxy_crud() {
            let (client, _temp_dir) = match create_test_client().await {
                Ok(c) => c,
                Err(e) => {
                    println!("Failed to create SQLite test client: {}", e);
                    return;
                }
            };
            
            // Test proxy CRUD operations
            let proxy = Proxy {
                id: "test1".to_string(),
                name: Some("Test Proxy".to_string()),
                listen_path: "/api/test".to_string(),
                backend_protocol: Protocol::Http,
                backend_host: "example.com".to_string(),
                backend_port: 80,
                backend_path: Some("/backend".to_string()),
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
            
            // CREATE
            let result = client.create_proxy(&proxy).await;
            assert!(result.is_ok(), "Failed to create proxy: {:?}", result);
            
            // READ
            let fetched = client.get_proxy("test1").await;
            assert!(fetched.is_ok(), "Failed to get proxy: {:?}", fetched);
            let fetched_proxy = fetched.unwrap();
            assert_eq!(fetched_proxy.id, proxy.id);
            assert_eq!(fetched_proxy.listen_path, proxy.listen_path);
            
            // Additional SQLite-specific tests as needed
        }
    }
}
