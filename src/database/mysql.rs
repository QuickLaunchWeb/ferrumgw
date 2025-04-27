use anyhow::{anyhow, Result, Context};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{mysql::{MySqlPoolOptions, MySqlPool}, Pool, MySql, Row};
use tracing::{debug, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode, ConfigurationDelta, PluginAssociation, PluginScope};

// Module-level functions for use in the DatabaseClient trait
pub async fn load_full_configuration(pool: &Pool<MySql>) -> Result<Configuration> {
    info!("Loading full configuration from MySQL database");
    
    // Begin a transaction to ensure consistent data
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Load all proxies, consumers, and plugin configs
    let proxies = load_proxies(pool).await?;
    let consumers = load_consumers(pool).await?;
    let plugin_configs = load_plugin_configs(pool).await?;
    
    // Create association map between proxies and plugins
    let proxy_plugin_map = load_proxy_plugin_associations(pool).await?;
    
    // Associate plugins with each proxy
    let mut proxies_with_plugins = Vec::with_capacity(proxies.len());
    for mut proxy in proxies {
        if let Some(plugin_ids) = proxy_plugin_map.get(&proxy.id) {
            for plugin_id in plugin_ids {
                if let Some(plugin_config) = plugin_configs.iter().find(|p| &p.id == plugin_id) {
                    proxy.plugins.push(plugin_config.clone());
                }
            }
        }
        proxies_with_plugins.push(proxy);
    }
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    Ok(Configuration {
        proxies: proxies_with_plugins,
        consumers,
        plugin_configs,
        last_updated_at: Utc::now(),
    })
}

pub async fn create_proxy(pool: &Pool<MySql>, proxy: Proxy) -> Result<Proxy> {
    info!("Creating proxy in MySQL database: {}", proxy.id);
    
    // Check if listen_path is unique
    let existing = sqlx::query(
        "SELECT COUNT(*) as count FROM proxies WHERE listen_path = ?"
    )
    .bind(&proxy.listen_path)
    .fetch_one(pool)
    .await
    .map_err(|e| anyhow!("Failed to check for existing proxy: {}", e))?;
    
    let count: i64 = existing.try_get("count")?;
    if count > 0 {
        return Err(anyhow!("A proxy with listen_path '{}' already exists", proxy.listen_path));
    }
    
    // Convert enums to strings
    let backend_protocol = match proxy.backend_protocol {
        Protocol::Http => "http",
        Protocol::Https => "https",
        Protocol::Ws => "ws",
        Protocol::Wss => "wss",
        Protocol::Grpc => "grpc",
    };
    
    let auth_mode = match proxy.auth_mode {
        AuthMode::Single => "single",
        AuthMode::Multi => "multi",
    };
    
    // Insert the proxy
    sqlx::query(
        r#"
        INSERT INTO proxies (
            id, name, listen_path, backend_protocol, backend_host, backend_port,
            backend_path, strip_listen_path, preserve_host_header,
            backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
            backend_tls_client_cert_path, backend_tls_client_key_path,
            backend_tls_verify_server_cert, backend_tls_server_ca_cert_path,
            dns_override, dns_cache_ttl_seconds, auth_mode, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, ?, 
            ?, ?, ?, 
            ?, ?, ?,
            ?, ?,
            ?, ?,
            ?, ?, ?, ?, ?
        )
        "#
    )
    .bind(&proxy.id)
    .bind(&proxy.name)
    .bind(&proxy.listen_path)
    .bind(backend_protocol)
    .bind(&proxy.backend_host)
    .bind(proxy.backend_port)
    .bind(&proxy.backend_path)
    .bind(proxy.strip_listen_path)
    .bind(proxy.preserve_host_header)
    .bind(proxy.backend_connect_timeout_ms as i64)
    .bind(proxy.backend_read_timeout_ms as i64)
    .bind(proxy.backend_write_timeout_ms as i64)
    .bind(&proxy.backend_tls_client_cert_path)
    .bind(&proxy.backend_tls_client_key_path)
    .bind(proxy.backend_tls_verify_server_cert)
    .bind(&proxy.backend_tls_server_ca_cert_path)
    .bind(&proxy.dns_override)
    .bind(proxy.dns_cache_ttl_seconds.map(|ttl| ttl as i64))
    .bind(auth_mode)
    .bind(proxy.created_at)
    .bind(proxy.updated_at)
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create proxy in MySQL: {}", e))?;
    
    info!("Created proxy with ID: {}", proxy.id);
    
    Ok(proxy)
}

async fn load_proxies(pool: &Pool<MySql>) -> Result<Vec<Proxy>> {
    let rows = sqlx::query_as!(
        Proxy,
        r#"
        SELECT 
            id, name, listen_path, 
            backend_protocol as `backend_protocol: String`, 
            backend_host, backend_port, backend_path, 
            strip_listen_path, preserve_host_header, 
            backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
            backend_tls_client_cert_path, backend_tls_client_key_path, backend_tls_verify_server_cert,
            backend_tls_server_ca_cert_path, dns_override, dns_cache_ttl_seconds, 
            auth_mode as `auth_mode: String`,
            created_at, updated_at
        FROM proxies
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow!("Failed to load proxies from MySQL: {}", e))?;
    
    let mut proxies = Vec::with_capacity(rows.len());
    for mut proxy in rows {
        // Parse the protocol and auth_mode strings to enums
        proxy.backend_protocol = match proxy.backend_protocol.as_str() {
            "http" => Protocol::Http,
            "https" => Protocol::Https,
            "ws" => Protocol::Ws,
            "wss" => Protocol::Wss,
            _ => Protocol::Http,
        };
        
        proxy.auth_mode = match proxy.auth_mode.as_str() {
            "single" => AuthMode::Single,
            "multi" => AuthMode::Multi,
            _ => AuthMode::Single,
        };
        
        proxies.push(proxy);
    }
    
    Ok(proxies)
}

async fn load_consumers(pool: &Pool<MySql>) -> Result<Vec<Consumer>> {
    let rows = sqlx::query(
        r#"
        SELECT 
            id, username, custom_id, credentials, created_at, updated_at
        FROM consumers
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow!("Failed to load consumers from MySQL: {}", e))?;
    
    let mut consumers = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id")?;
        let username: String = row.try_get("username")?;
        let custom_id: Option<String> = row.try_get("custom_id")?;
        let credentials_json: Option<String> = row.try_get("credentials")?;
        let created_at: DateTime<Utc> = row.try_get("created_at")?;
        let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
        
        let credentials = match credentials_json {
            Some(json) => serde_json::from_str::<HashMap<String, Value>>(&json)
                .unwrap_or_else(|_| HashMap::new()),
            None => HashMap::new(),
        };
        
        let consumer = Consumer {
            id,
            username,
            custom_id,
            credentials,
            created_at,
            updated_at,
        };
        
        consumers.push(consumer);
    }
    
    Ok(consumers)
}

async fn load_plugin_configs(pool: &Pool<MySql>) -> Result<Vec<PluginConfig>> {
    let rows = sqlx::query(
        r#"
        SELECT 
            id, plugin_name, config, scope, proxy_id, consumer_id, enabled,
            created_at, updated_at
        FROM plugin_configs
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow!("Failed to load plugin configurations from MySQL: {}", e))?;
    
    let mut plugin_configs = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id")?;
        let plugin_name: String = row.try_get("plugin_name")?;
        let config_json: String = row.try_get("config")?;
        let scope: String = row.try_get("scope")?;
        let proxy_id: Option<String> = row.try_get("proxy_id")?;
        let consumer_id: Option<String> = row.try_get("consumer_id")?;
        let enabled: bool = row.try_get("enabled")?;
        let created_at: DateTime<Utc> = row.try_get("created_at")?;
        let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
        
        let config = serde_json::from_str::<Value>(&config_json)
            .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
        
        let plugin_config = PluginConfig {
            id,
            plugin_name,
            config,
            scope,
            proxy_id,
            consumer_id,
            enabled,
            created_at,
            updated_at,
        };
        
        plugin_configs.push(plugin_config);
    }
    
    Ok(plugin_configs)
}

async fn load_proxy_plugin_associations(pool: &Pool<MySql>) -> Result<HashMap<String, Vec<String>>> {
    let rows = sqlx::query(
        r#"
        SELECT 
            proxy_id, plugin_config_id
        FROM proxy_plugin_associations
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow!("Failed to load proxy-plugin associations from MySQL: {}", e))?;
    
    let mut proxy_plugin_map: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        let proxy_id: String = row.try_get("proxy_id")?;
        let plugin_config_id: String = row.try_get("plugin_config_id")?;
        
        proxy_plugin_map.entry(proxy_id)
            .or_insert_with(Vec::new)
            .push(plugin_config_id);
    }
    
    Ok(proxy_plugin_map)
}

/// Get a consumer by ID from the database
pub async fn get_consumer_by_id(pool: &Pool<MySql>, consumer_id: &str) -> Result<Consumer> {
    info!("Fetching consumer from MySQL database by ID: {}", consumer_id);
    
    let row = sqlx::query(
        r#"
        SELECT 
            id, username, custom_id, credentials, created_at, updated_at
        FROM consumers
        WHERE id = ?
        "#
    )
    .bind(consumer_id)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch consumer from MySQL database")?;
    
    match row {
        Some(row) => {
            let id: String = row.try_get("id")?;
            let username: String = row.try_get("username")?;
            let custom_id: Option<String> = row.try_get("custom_id")?;
            let credentials_json: Option<String> = row.try_get("credentials")?;
            let created_at: DateTime<Utc> = row.try_get("created_at")?;
            let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
            
            let credentials = match credentials_json {
                Some(json) => serde_json::from_str::<HashMap<String, Value>>(&json)
                    .unwrap_or_else(|_| HashMap::new()),
                None => HashMap::new(),
            };
            
            Ok(Consumer {
                id,
                username,
                custom_id,
                credentials,
                created_at,
                updated_at,
            })
        },
        None => Err(anyhow!("Consumer with ID '{}' not found", consumer_id))
    }
}

/// MySQL implementation of the database client
pub struct MySqlClient {
    pool: MySqlPool,
}

impl MySqlClient {
    pub async fn new(url: &str, max_connections: u32) -> Result<Self> {
        info!("Initializing MySQL database connection");
        
        let pool = MySqlPoolOptions::new()
            .max_connections(max_connections)
            .connect_timeout(Duration::from_secs(10))
            .connect(url)
            .await
            .map_err(|e| anyhow!("Failed to connect to MySQL database: {}", e))?;
        
        info!("Successfully connected to MySQL database");
        
        Ok(Self { pool })
    }
    
    pub async fn load_configuration(&self) -> Result<Configuration> {
        debug!("Loading full configuration from MySQL database");
        
        // Load proxies
        #[allow(unused_mut)]
        let mut proxies = match sqlx::query_as!(
            Proxy,
            r#"
            SELECT 
                id, name, listen_path, 
                backend_protocol as `backend_protocol: String`, 
                backend_host, backend_port, backend_path, 
                strip_listen_path, preserve_host_header, 
                backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
                backend_tls_client_cert_path, backend_tls_client_key_path, backend_tls_verify_server_cert,
                backend_tls_server_ca_cert_path, dns_override, dns_cache_ttl_seconds, 
                auth_mode as `auth_mode: String`,
                created_at, updated_at
            FROM proxies
            "#
        ).fetch_all(&self.pool).await {
            Ok(proxies) => proxies,
            Err(e) => {
                error!("Failed to load proxies from MySQL database: {}", e);
                return Err(anyhow!("Database error: {}", e));
            }
        };
        
        // Parse the protocol and auth_mode strings to enums
        for proxy in &mut proxies {
            proxy.backend_protocol = match proxy.backend_protocol.as_str() {
                "http" => Protocol::Http,
                "https" => Protocol::Https,
                "ws" => Protocol::Ws,
                "wss" => Protocol::Wss,
                _ => Protocol::Http,
            };
            
            proxy.auth_mode = match proxy.auth_mode.as_str() {
                "single" => AuthMode::Single,
                "multi" => AuthMode::Multi,
                _ => AuthMode::Single,
            };
        }
        
        // Load consumers
        let consumers = match sqlx::query_as!(
            Consumer,
            r#"
            SELECT 
                id, username, custom_id, credentials, created_at, updated_at
            FROM consumers
            "#
        ).fetch_all(&self.pool).await {
            Ok(consumers) => consumers,
            Err(e) => {
                error!("Failed to load consumers from MySQL database: {}", e);
                return Err(anyhow!("Database error: {}", e));
            }
        };
        
        // Load plugin configurations
        let plugin_configs = match sqlx::query_as!(
            PluginConfig,
            r#"
            SELECT 
                id, plugin_name, config, 
                scope, proxy_id, consumer_id, 
                enabled, created_at, updated_at
            FROM plugin_configs
            "#
        ).fetch_all(&self.pool).await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to load plugin configs from MySQL database: {}", e);
                return Err(anyhow!("Database error: {}", e));
            }
        };
        
        // Load plugin associations for each proxy
        for proxy in &mut proxies {
            // Get associated plugin configs
            let plugin_associations = match sqlx::query!(
                r#"
                SELECT plugin_config_id, embedded_config
                FROM proxy_plugin_associations
                WHERE proxy_id = ?
                ORDER BY priority ASC
                "#,
                proxy.id
            ).fetch_all(&self.pool).await {
                Ok(associations) => associations,
                Err(e) => {
                    error!("Failed to load plugin associations for proxy {}: {}", proxy.id, e);
                    continue;
                }
            };
            
            // Add each associated plugin to the proxy
            for association in plugin_associations {
                // Find the plugin config
                if let Some(plugin_config) = plugin_configs.iter().find(|p| p.id == association.plugin_config_id) {
                    proxy.plugins.push(plugin_config.id.clone());
                }
            }
        }
        
        let now = Utc::now();
        
        Ok(Configuration {
            proxies,
            consumers,
            plugin_configs,
            last_updated_at: now,
        })
    }
    
    async fn load_proxies(&self) -> Result<Vec<Proxy>> {
        let rows = sqlx::query_as!(
            Proxy,
            r#"
            SELECT 
                id, name, listen_path, 
                backend_protocol as `backend_protocol: String`, 
                backend_host, backend_port, backend_path, 
                strip_listen_path, preserve_host_header, 
                backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
                backend_tls_client_cert_path, backend_tls_client_key_path, backend_tls_verify_server_cert,
                backend_tls_server_ca_cert_path, dns_override, dns_cache_ttl_seconds, 
                auth_mode as `auth_mode: String`,
                created_at, updated_at
            FROM proxies
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to load proxies from MySQL: {}", e))?;
        
        let mut proxies = Vec::with_capacity(rows.len());
        for mut proxy in rows {
            // Parse the protocol and auth_mode strings to enums
            proxy.backend_protocol = match proxy.backend_protocol.as_str() {
                "http" => Protocol::Http,
                "https" => Protocol::Https,
                "ws" => Protocol::Ws,
                "wss" => Protocol::Wss,
                _ => Protocol::Http,
            };
            
            proxy.auth_mode = match proxy.auth_mode.as_str() {
                "single" => AuthMode::Single,
                "multi" => AuthMode::Multi,
                _ => AuthMode::Single,
            };
            
            proxies.push(proxy);
        }
        
        Ok(proxies)
    }
    
    async fn load_consumers(&self) -> Result<Vec<Consumer>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                id, username, custom_id, credentials, created_at, updated_at
            FROM consumers
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to load consumers from MySQL: {}", e))?;
        
        let mut consumers = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id")?;
            let username: String = row.try_get("username")?;
            let custom_id: Option<String> = row.try_get("custom_id")?;
            let credentials_json: Option<String> = row.try_get("credentials")?;
            let created_at: DateTime<Utc> = row.try_get("created_at")?;
            let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
            
            let credentials = match credentials_json {
                Some(json) => serde_json::from_str::<HashMap<String, Value>>(&json)
                    .unwrap_or_else(|_| HashMap::new()),
                None => HashMap::new(),
            };
            
            let consumer = Consumer {
                id,
                username,
                custom_id,
                credentials,
                created_at,
                updated_at,
            };
            
            consumers.push(consumer);
        }
        
        Ok(consumers)
    }
    
    async fn load_plugin_configs(&self) -> Result<Vec<PluginConfig>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                id, plugin_name, config, scope, proxy_id, consumer_id, enabled,
                created_at, updated_at
            FROM plugin_configs
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to load plugin configurations from MySQL: {}", e))?;
        
        let mut plugin_configs = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id")?;
            let plugin_name: String = row.try_get("plugin_name")?;
            let config_json: String = row.try_get("config")?;
            let scope: String = row.try_get("scope")?;
            let proxy_id: Option<String> = row.try_get("proxy_id")?;
            let consumer_id: Option<String> = row.try_get("consumer_id")?;
            let enabled: bool = row.try_get("enabled")?;
            let created_at: DateTime<Utc> = row.try_get("created_at")?;
            let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
            
            let config = serde_json::from_str::<Value>(&config_json)
                .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
            
            let plugin_config = PluginConfig {
                id,
                plugin_name,
                config,
                scope,
                proxy_id,
                consumer_id,
                enabled,
                created_at,
                updated_at,
            };
            
            plugin_configs.push(plugin_config);
        }
        
        Ok(plugin_configs)
    }
    
    async fn load_proxy_plugin_associations(&self) -> Result<HashMap<String, Vec<String>>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                proxy_id, plugin_config_id
            FROM proxy_plugin_associations
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to load proxy-plugin associations from MySQL: {}", e))?;
        
        let mut proxy_plugin_map: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let proxy_id: String = row.try_get("proxy_id")?;
            let plugin_config_id: String = row.try_get("plugin_config_id")?;
            
            proxy_plugin_map.entry(proxy_id)
                .or_insert_with(Vec::new)
                .push(plugin_config_id);
        }
        
        Ok(proxy_plugin_map)
    }
    
    // CRUD methods for proxies
    pub async fn create_proxy(&self, proxy: &Proxy) -> Result<String> {
        // Check if listen_path is unique
        let existing = sqlx::query(
            "SELECT COUNT(*) as count FROM proxies WHERE listen_path = ?"
        )
        .bind(&proxy.listen_path)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to check for existing proxy: {}", e))?;
        
        let count: i64 = existing.try_get("count")?;
        if count > 0 {
            return Err(anyhow!("A proxy with listen_path '{}' already exists", proxy.listen_path));
        }
        
        // Convert enums to strings
        let backend_protocol = match proxy.backend_protocol {
            Protocol::Http => "http",
            Protocol::Https => "https",
            Protocol::Ws => "ws",
            Protocol::Wss => "wss",
            Protocol::Grpc => "grpc",
        };
        
        let auth_mode = match proxy.auth_mode {
            AuthMode::Single => "single",
            AuthMode::Multi => "multi",
        };
        
        // Insert the proxy
        let result = sqlx::query(
            r#"
            INSERT INTO proxies (
                id, name, listen_path, backend_protocol, backend_host, backend_port,
                backend_path, strip_listen_path, preserve_host_header,
                backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
                backend_tls_client_cert_path, backend_tls_client_key_path,
                backend_tls_verify_server_cert, backend_tls_server_ca_cert_path,
                dns_override, dns_cache_ttl_seconds, auth_mode, created_at, updated_at
            ) VALUES (
                ?, ?, ?, ?, ?, ?, 
                ?, ?, ?, 
                ?, ?, ?,
                ?, ?,
                ?, ?,
                ?, ?, ?, ?, ?
            )
            "#
        )
        .bind(&proxy.id)
        .bind(&proxy.name)
        .bind(&proxy.listen_path)
        .bind(backend_protocol)
        .bind(&proxy.backend_host)
        .bind(proxy.backend_port)
        .bind(&proxy.backend_path)
        .bind(proxy.strip_listen_path)
        .bind(proxy.preserve_host_header)
        .bind(proxy.backend_connect_timeout_ms as i64)
        .bind(proxy.backend_read_timeout_ms as i64)
        .bind(proxy.backend_write_timeout_ms as i64)
        .bind(&proxy.backend_tls_client_cert_path)
        .bind(&proxy.backend_tls_client_key_path)
        .bind(proxy.backend_tls_verify_server_cert)
        .bind(&proxy.backend_tls_server_ca_cert_path)
        .bind(&proxy.dns_override)
        .bind(proxy.dns_cache_ttl_seconds.map(|ttl| ttl as i64))
        .bind(auth_mode)
        .bind(proxy.created_at)
        .bind(proxy.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to create proxy in MySQL: {}", e))?;
        
        info!("Created proxy with ID: {}", proxy.id);
        
        Ok(proxy.id.clone())
    }
    
    /// Update an existing proxy in the database
    pub async fn update_proxy(&self, proxy: &Proxy) -> Result<()> {
        info!("Updating proxy in MySQL database: {}", proxy.id);
        
        // Check if proxy exists
        let exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM proxies WHERE id = ?) as exists",
            proxy.id
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check proxy existence")?
        .exists;
        
        if exists == 0 {
            return Err(anyhow!("Proxy with ID '{}' does not exist", proxy.id));
        }
        
        // Check if the new listen_path would conflict with another proxy
        let path_exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM proxies WHERE listen_path = ? AND id != ?) as exists",
            proxy.listen_path, proxy.id
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check listen_path uniqueness")?
        .exists;
        
        if path_exists != 0 {
            return Err(anyhow!("Another proxy with listen_path '{}' already exists", proxy.listen_path));
        }
        
        // Convert the protocol and auth_mode enums to strings
        let backend_protocol_str = match proxy.backend_protocol {
            Protocol::Http => "http",
            Protocol::Https => "https",
            Protocol::Ws => "ws",
            Protocol::Wss => "wss",
            Protocol::Grpc => "grpc",
        };
        
        let auth_mode_str = match proxy.auth_mode {
            AuthMode::Single => "single",
            AuthMode::Multi => "multi",
        };
        
        // Start a transaction
        let mut tx = self.pool.begin().await.context("Failed to begin transaction")?;
        
        // Update the proxy
        sqlx::query!(
            r#"
            UPDATE proxies
            SET 
                name = ?,
                listen_path = ?,
                backend_protocol = ?,
                backend_host = ?,
                backend_port = ?,
                backend_path = ?,
                strip_listen_path = ?,
                preserve_host_header = ?,
                backend_connect_timeout_ms = ?,
                backend_read_timeout_ms = ?,
                backend_write_timeout_ms = ?,
                backend_tls_client_cert_path = ?,
                backend_tls_client_key_path = ?,
                backend_tls_verify_server_cert = ?,
                backend_tls_server_ca_cert_path = ?,
                dns_override = ?,
                dns_cache_ttl_seconds = ?,
                auth_mode = ?,
                updated_at = NOW()
            WHERE id = ?
            "#,
            proxy.name,
            proxy.listen_path,
            backend_protocol_str,
            proxy.backend_host,
            proxy.backend_port as i32,
            proxy.backend_path,
            proxy.strip_listen_path,
            proxy.preserve_host_header,
            proxy.backend_connect_timeout_ms as i64,
            proxy.backend_read_timeout_ms as i64,
            proxy.backend_write_timeout_ms as i64,
            proxy.backend_tls_client_cert_path,
            proxy.backend_tls_client_key_path,
            proxy.backend_tls_verify_server_cert,
            proxy.backend_tls_server_ca_cert_path,
            proxy.dns_override,
            proxy.dns_cache_ttl_seconds.map(|ttl| ttl as i64),
            auth_mode_str,
            proxy.id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to update proxy")?;
        
        // Delete existing plugin associations
        sqlx::query!(
            "DELETE FROM proxy_plugin_associations WHERE proxy_id = ?",
            proxy.id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete existing plugin associations")?;
        
        // Insert new plugin associations
        for plugin_assoc in &proxy.plugins {
            let embedded_config_json = match &plugin_assoc.embedded_config {
                Some(config) => Some(serde_json::to_value(config)?),
                None => None,
            };
            
            sqlx::query!(
                r#"
                INSERT INTO proxy_plugin_associations (
                    proxy_id, plugin_config_id, embedded_config
                )
                VALUES (?, ?, ?)
                "#,
                proxy.id,
                plugin_assoc.plugin_config_id,
                embedded_config_json
            )
            .execute(&mut *tx)
            .await
            .context("Failed to insert plugin association")?;
        }
        
        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;
        
        info!("Updated proxy with ID: {}", proxy.id);
        Ok(())
    }
    
    /// Delete a proxy from the database
    pub async fn delete_proxy(&self, proxy_id: &str) -> Result<()> {
        info!("Deleting proxy with ID: {}", proxy_id);
        
        // Begin a transaction
        let mut tx = self.pool.begin().await.context("Failed to begin transaction")?;
        
        // First, delete any plugin associations
        sqlx::query!(
            r#"
            DELETE FROM proxy_plugin_associations
            WHERE proxy_id = ?
            "#,
            proxy_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete proxy plugin associations")?;
        
        // Then delete the proxy itself
        let delete_result = sqlx::query!(
            r#"
            DELETE FROM proxies
            WHERE id = ?
            "#,
            proxy_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete proxy")?;
        
        // Insert into proxy_deletions table for incremental updates
        if delete_result.rows_affected() > 0 {
            sqlx::query!(
                r#"
                INSERT INTO proxy_deletions (id, deleted_at)
                VALUES (?, CURRENT_TIMESTAMP)
                ON DUPLICATE KEY UPDATE deleted_at = CURRENT_TIMESTAMP
                "#,
                proxy_id
            )
            .execute(&mut *tx)
            .await
            .context("Failed to track proxy deletion")?;
        }
        
        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;
        
        info!("Deleted proxy with ID: {}", proxy_id);
        Ok(())
    }
    
    /// Create a new consumer in the database
    pub async fn create_consumer(&self, consumer: &Consumer) -> Result<String> {
        info!("Creating new consumer in MySQL database: {}", consumer.username);
        
        // Check if username is unique
        let exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM consumers WHERE username = ?) as exists",
            consumer.username
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check username uniqueness")?
        .exists;
        
        if exists != 0 {
            return Err(anyhow!("A consumer with username '{}' already exists", consumer.username));
        }
        
        // If custom_id is provided, check if it's unique
        if let Some(custom_id) = &consumer.custom_id {
            let custom_id_exists = sqlx::query!(
                "SELECT EXISTS(SELECT 1 FROM consumers WHERE custom_id = ?) as exists",
                custom_id
            )
            .fetch_one(&self.pool)
            .await
            .context("Failed to check custom_id uniqueness")?
            .exists;
            
            if custom_id_exists != 0 {
                return Err(anyhow!("A consumer with custom_id '{}' already exists", custom_id));
            }
        }
        
        // Serialize credentials to JSON
        let credentials_json = serde_json::to_value(&consumer.credentials)
            .context("Failed to serialize consumer credentials")?;
        
        // Generate a UUID for the consumer ID
        let id = uuid::Uuid::new_v4().to_string();
        
        // Insert the consumer
        sqlx::query!(
            r#"
            INSERT INTO consumers (
                id, username, custom_id, credentials, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, NOW(), NOW())
            "#,
            id,
            consumer.username,
            consumer.custom_id,
            credentials_json
        )
        .execute(&self.pool)
        .await
        .context("Failed to insert consumer")?;
        
        info!("Created new consumer with ID: {}", id);
        Ok(id)
    }
    
    /// Update an existing consumer in the database
    pub async fn update_consumer(&self, consumer: &Consumer) -> Result<()> {
        info!("Updating consumer in MySQL database: {}", consumer.id);
        
        // Check if consumer exists
        let exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM consumers WHERE id = ?) as exists",
            consumer.id
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check consumer existence")?
        .exists;
        
        if exists == 0 {
            return Err(anyhow!("Consumer with ID '{}' does not exist", consumer.id));
        }
        
        // Check username uniqueness
        let username_exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM consumers WHERE username = ? AND id != ?) as exists",
            consumer.username, consumer.id
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check username uniqueness")?
        .exists;
        
        if username_exists != 0 {
            return Err(anyhow!("Another consumer with username '{}' already exists", consumer.username));
        }
        
        // If custom_id is provided, check if it's unique
        if let Some(custom_id) = &consumer.custom_id {
            let custom_id_exists = sqlx::query!(
                "SELECT EXISTS(SELECT 1 FROM consumers WHERE custom_id = ? AND id != ?) as exists",
                custom_id, consumer.id
            )
            .fetch_one(&self.pool)
            .await
            .context("Failed to check custom_id uniqueness")?
            .exists;
            
            if custom_id_exists != 0 {
                return Err(anyhow!("Another consumer with custom_id '{}' already exists", custom_id));
            }
        }
        
        // Serialize credentials to JSON
        let credentials_json = serde_json::to_value(&consumer.credentials)
            .context("Failed to serialize consumer credentials")?;
        
        // Update the consumer
        sqlx::query!(
            r#"
            UPDATE consumers
            SET 
                username = ?,
                custom_id = ?,
                credentials = ?,
                updated_at = NOW()
            WHERE id = ?
            "#,
            consumer.username,
            consumer.custom_id,
            credentials_json,
            consumer.id
        )
        .execute(&self.pool)
        .await
        .context("Failed to update consumer")?;
        
        info!("Updated consumer with ID: {}", consumer.id);
        Ok(())
    }
    
    /// Delete a consumer from the database
    pub async fn delete_consumer(&self, consumer_id: &str) -> Result<()> {
        info!("Deleting consumer with ID: {}", consumer_id);
        
        // Begin a transaction
        let mut tx = self.pool.begin().await.context("Failed to begin transaction")?;
        
        // Delete related plugin configs first
        sqlx::query!(
            "DELETE FROM plugin_configs WHERE consumer_id = ?",
            consumer_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete related plugin configurations")?;
        
        // Then delete the consumer
        let delete_result = sqlx::query!(
            "DELETE FROM consumers WHERE id = ?",
            consumer_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete consumer")?;
        
        // Insert into consumer_deletions table for incremental updates
        if delete_result.rows_affected() > 0 {
            sqlx::query!(
                r#"
                INSERT INTO consumer_deletions (id, deleted_at)
                VALUES (?, CURRENT_TIMESTAMP)
                ON DUPLICATE KEY UPDATE deleted_at = CURRENT_TIMESTAMP
                "#,
                consumer_id
            )
            .execute(&mut *tx)
            .await
            .context("Failed to track consumer deletion")?;
        }
        
        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;
        
        info!("Deleted consumer with ID: {}", consumer_id);
        Ok(())
    }
    
    /// Create a new plugin configuration in the database
    pub async fn create_plugin_config(&self, plugin_config: &PluginConfig) -> Result<String> {
        info!("Creating new plugin configuration in MySQL database: {}", plugin_config.plugin_name);
        
        // Serialize config to JSON
        let config_json = serde_json::to_value(&plugin_config.config)
            .context("Failed to serialize plugin configuration")?;
        
        // Generate a UUID for the plugin config ID
        let id = uuid::Uuid::new_v4().to_string();
        
        // Insert the plugin config
        sqlx::query!(
            r#"
            INSERT INTO plugin_configs (
                id, plugin_name, config, scope, proxy_id, consumer_id, enabled,
                created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, NOW(), NOW())
            "#,
            id,
            plugin_config.plugin_name,
            config_json,
            plugin_config.scope,
            plugin_config.proxy_id,
            plugin_config.consumer_id,
            plugin_config.enabled
        )
        .execute(&self.pool)
        .await
        .context("Failed to insert plugin configuration")?;
        
        info!("Created new plugin configuration with ID: {}", id);
        Ok(id)
    }
    
    /// Update an existing plugin configuration in the database
    pub async fn update_plugin_config(&self, plugin_config: &PluginConfig) -> Result<()> {
        info!("Updating plugin configuration in MySQL database: {}", plugin_config.id);
        
        // Check if plugin config exists
        let exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM plugin_configs WHERE id = ?) as exists",
            plugin_config.id
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check plugin config existence")?
        .exists;
        
        if exists == 0 {
            return Err(anyhow!("Plugin configuration with ID '{}' does not exist", plugin_config.id));
        }
        
        // Serialize config to JSON
        let config_json = serde_json::to_value(&plugin_config.config)
            .context("Failed to serialize plugin configuration")?;
        
        // Update the plugin config
        sqlx::query!(
            r#"
            UPDATE plugin_configs
            SET 
                plugin_name = ?,
                config = ?,
                scope = ?,
                proxy_id = ?,
                consumer_id = ?,
                enabled = ?,
                updated_at = NOW()
            WHERE id = ?
            "#,
            plugin_config.plugin_name,
            config_json,
            plugin_config.scope,
            plugin_config.proxy_id,
            plugin_config.consumer_id,
            plugin_config.enabled,
            plugin_config.id
        )
        .execute(&self.pool)
        .await
        .context("Failed to update plugin configuration")?;
        
        info!("Updated plugin configuration with ID: {}", plugin_config.id);
        Ok(())
    }
    
    /// Delete a plugin configuration from the database
    pub async fn delete_plugin_config(&self, plugin_config_id: &str) -> Result<()> {
        info!("Deleting plugin configuration with ID: {}", plugin_config_id);
        
        // Begin a transaction
        let mut tx = self.pool.begin().await.context("Failed to begin transaction")?;
        
        // First, delete any proxy associations
        sqlx::query!(
            "DELETE FROM proxy_plugin_associations WHERE plugin_config_id = ?",
            plugin_config_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete plugin-proxy associations")?;
        
        // Then delete the plugin config itself
        let delete_result = sqlx::query!(
            "DELETE FROM plugin_configs WHERE id = ?",
            plugin_config_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete plugin configuration")?;
        
        // Insert into plugin_config_deletions table for incremental updates
        if delete_result.rows_affected() > 0 {
            sqlx::query!(
                r#"
                INSERT INTO plugin_config_deletions (id, deleted_at)
                VALUES (?, CURRENT_TIMESTAMP)
                ON DUPLICATE KEY UPDATE deleted_at = CURRENT_TIMESTAMP
                "#,
                plugin_config_id
            )
            .execute(&mut *tx)
            .await
            .context("Failed to track plugin config deletion")?;
        }
        
        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;
        
        info!("Deleted plugin configuration with ID: {}", plugin_config_id);
        Ok(())
    }
    
    /// Get the latest update timestamp from the database
    pub async fn get_latest_update_timestamp(&self) -> Result<DateTime<Utc>> {
        debug!("Getting latest update timestamp from MySQL database");
        
        // Use a query that combines the latest timestamps from all tables
        let result = sqlx::query!(
            r#"
            SELECT MAX(latest_time) as max_time
            FROM (
                SELECT MAX(updated_at) as latest_time FROM proxies
                UNION ALL
                SELECT MAX(updated_at) as latest_time FROM consumers
                UNION ALL
                SELECT MAX(updated_at) as latest_time FROM plugin_configs
            ) as latest_updates
            "#
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to get latest update timestamp")?;
        
        // If there are no records, return the current time
        match result.max_time {
            Some(time) => Ok(time.and_utc()),
            None => Ok(Utc::now()),
        }
    }
    
    /// Load configuration changes since a specific timestamp
    pub async fn load_configuration_delta(&self, since: DateTime<Utc>) -> Result<ConfigurationDelta> {
        info!("Loading configuration delta from MySQL database since {}", since);
        
        // Begin a transaction to ensure consistent data
        let mut tx = self.pool.begin().await.context("Failed to begin transaction")?;
        
        // Load updated proxies
        let updated_proxies = sqlx::query_as!(
            Proxy,
            r#"
            SELECT 
                id,
                name, listen_path, backend_protocol as backend_protocol_str, 
                backend_host, backend_port, backend_path,
                strip_listen_path, preserve_host_header,
                backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
                backend_tls_client_cert_path, backend_tls_client_key_path, backend_tls_verify_server_cert,
                backend_tls_server_ca_cert_path, 
                dns_override, dns_cache_ttl_seconds,
                auth_mode as auth_mode_str,
                created_at, updated_at
            FROM proxies
            WHERE updated_at > ?
            ORDER BY updated_at
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await
        .context("Failed to fetch updated proxies from database")?;
        
        // Parse the protocol and auth_mode strings to enums for updated proxies
        let mut processed_proxies = Vec::with_capacity(updated_proxies.len());
        for mut proxy in updated_proxies {
            proxy.backend_protocol = match proxy.backend_protocol_str.as_str() {
                "http" => Protocol::Http,
                "https" => Protocol::Https,
                "ws" => Protocol::Ws,
                "wss" => Protocol::Wss,
                "grpc" => Protocol::Grpc,
                _ => Protocol::Http,
            };
            
            proxy.auth_mode = match proxy.auth_mode_str.as_str() {
                "multi" => AuthMode::Multi,
                _ => AuthMode::Single,
            };
            
            // Load any plugin associations for this proxy
            proxy.plugins = sqlx::query_as!(
                PluginAssociation,
                r#"
                SELECT plugin_config_id, embedded_config as embedded_config_json
                FROM proxy_plugin_associations
                WHERE proxy_id = ?
                "#,
                proxy.id
            )
            .fetch_all(&mut *tx)
            .await
            .context("Failed to fetch plugin associations")?
            .into_iter()
            .map(|assoc| {
                PluginAssociation {
                    plugin_config_id: assoc.plugin_config_id,
                    embedded_config: serde_json::from_str(&assoc.embedded_config_json.unwrap_or_else(|| "{}".to_string()))
                        .unwrap_or_else(|_| serde_json::json!({})),
                }
            })
            .collect();
            
            processed_proxies.push(proxy);
        }
        
        // Get IDs of deleted proxies
        let deleted_proxy_ids = sqlx::query!(
            r#"
            SELECT id
            FROM proxy_deletions
            WHERE deleted_at > ?
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await
        .context("Failed to fetch deleted proxy IDs")?
        .into_iter()
        .map(|row| row.id)
        .collect::<Vec<String>>();
        
        // Load updated consumers
        let updated_consumers = sqlx::query_as!(
            Consumer,
            r#"
            SELECT 
                id, username, custom_id,
                credentials as credentials_json,
                created_at, updated_at
            FROM consumers
            WHERE updated_at > ?
            ORDER BY updated_at
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await
        .context("Failed to fetch updated consumers from database")?
        .into_iter()
        .map(|consumer| {
            Consumer {
                id: consumer.id,
                username: consumer.username,
                custom_id: consumer.custom_id,
                credentials: serde_json::from_str(&consumer.credentials_json.unwrap_or_else(|| "{}".to_string()))
                    .unwrap_or_else(|_| serde_json::json!({})),
                created_at: consumer.created_at,
                updated_at: consumer.updated_at,
            }
        })
        .collect();
        
        // Get IDs of deleted consumers
        let deleted_consumer_ids = sqlx::query!(
            r#"
            SELECT id
            FROM consumer_deletions
            WHERE deleted_at > ?
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await
        .context("Failed to fetch deleted consumer IDs")?
        .into_iter()
        .map(|row| row.id)
        .collect::<Vec<String>>();
        
        // Load updated plugin configs
        let updated_plugin_configs = sqlx::query_as!(
            PluginConfig,
            r#"
            SELECT 
                id, plugin_name,
                config as config_json,
                scope as scope_str,
                proxy_id,
                enabled,
                created_at, updated_at
            FROM plugin_configs
            WHERE updated_at > ?
            ORDER BY updated_at
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await
        .context("Failed to fetch updated plugin configs from database")?;
        
        // Parse the scope enum and JSON config
        let mut processed_plugin_configs = Vec::with_capacity(updated_plugin_configs.len());
        for plugin_config in updated_plugin_configs {
            let scope = match plugin_config.scope_str.as_str() {
                "proxy" => PluginScope::Proxy,
                _ => PluginScope::Global,
            };
            
            let config = serde_json::from_str(&plugin_config.config_json.unwrap_or_else(|| "{}".to_string()))
                .unwrap_or_else(|_| serde_json::json!({}));
            
            processed_plugin_configs.push(PluginConfig {
                id: plugin_config.id,
                plugin_name: plugin_config.plugin_name,
                config,
                scope,
                proxy_id: plugin_config.proxy_id,
                enabled: plugin_config.enabled,
                created_at: plugin_config.created_at,
                updated_at: plugin_config.updated_at,
            });
        }
        
        // Get IDs of deleted plugin configs
        let deleted_plugin_config_ids = sqlx::query!(
            r#"
            SELECT id
            FROM plugin_config_deletions
            WHERE deleted_at > ?
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await
        .context("Failed to fetch deleted plugin config IDs")?
        .into_iter()
        .map(|row| row.id)
        .collect::<Vec<String>>();
        
        // Get the latest update timestamp
        let latest_timestamp = self.get_latest_update_timestamp().await?;
        
        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;
        
        Ok(ConfigurationDelta {
            updated_proxies: processed_proxies,
            deleted_proxy_ids,
            updated_consumers,
            deleted_consumer_ids,
            updated_plugin_configs: processed_plugin_configs,
            deleted_plugin_config_ids,
            last_updated_at: latest_timestamp,
        })
    }
}
