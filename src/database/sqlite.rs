use anyhow::{anyhow, Result, Context};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{sqlite::{SqlitePoolOptions, SqlitePool}, Pool, Sqlite, Row};
use tracing::{debug, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode};

// Module-level functions for use in the DatabaseClient trait
pub async fn load_full_configuration(pool: &Pool<Sqlite>) -> Result<Configuration> {
    info!("Loading full configuration from SQLite database");
    
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

pub async fn create_proxy(pool: &Pool<Sqlite>, proxy: Proxy) -> Result<Proxy> {
    info!("Creating proxy in SQLite database: {}", proxy.id);
    
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
    
    // SQLite doesn't have native DateTime, convert to ISO8601 strings
    let created_at = proxy.created_at.to_rfc3339();
    let updated_at = proxy.updated_at.to_rfc3339();
    
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
    .bind(if proxy.strip_listen_path { 1 } else { 0 })
    .bind(if proxy.preserve_host_header { 1 } else { 0 })
    .bind(proxy.backend_connect_timeout_ms as i64)
    .bind(proxy.backend_read_timeout_ms as i64)
    .bind(proxy.backend_write_timeout_ms as i64)
    .bind(&proxy.backend_tls_client_cert_path)
    .bind(&proxy.backend_tls_client_key_path)
    .bind(if proxy.backend_tls_verify_server_cert { 1 } else { 0 })
    .bind(&proxy.backend_tls_server_ca_cert_path)
    .bind(&proxy.dns_override)
    .bind(proxy.dns_cache_ttl_seconds.map(|ttl| ttl as i64))
    .bind(auth_mode)
    .bind(created_at)
    .bind(updated_at)
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create proxy in SQLite: {}", e))?;
    
    info!("Created proxy with ID: {}", proxy.id);
    
    Ok(proxy)
}

async fn load_proxies(pool: &Pool<Sqlite>) -> Result<Vec<Proxy>> {
    let rows = sqlx::query_as!(
        Proxy,
        r#"
        SELECT 
            id, name, listen_path, 
            backend_protocol as "backend_protocol: String", 
            backend_host, backend_port, backend_path, 
            strip_listen_path, preserve_host_header, 
            backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
            backend_tls_client_cert_path, backend_tls_client_key_path, backend_tls_verify_server_cert,
            backend_tls_server_ca_cert_path, dns_override, dns_cache_ttl_seconds, 
            auth_mode as "auth_mode: String",
            created_at, updated_at
        FROM proxies
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow!("Failed to load proxies from SQLite: {}", e))?;
    
    let mut proxies = Vec::with_capacity(rows.len());
    for proxy in rows {
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

async fn load_consumers(pool: &Pool<Sqlite>) -> Result<Vec<Consumer>> {
    let rows = sqlx::query_as!(
        Consumer,
        r#"
        SELECT 
            id, username, custom_id, credentials, created_at, updated_at
        FROM consumers
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow!("Failed to load consumers from SQLite: {}", e))?;
    
    Ok(rows)
}

async fn load_plugin_configs(pool: &Pool<Sqlite>) -> Result<Vec<PluginConfig>> {
    let rows = sqlx::query_as!(
        PluginConfig,
        r#"
        SELECT 
            id, plugin_name, config, 
            scope, proxy_id, consumer_id, 
            enabled, created_at, updated_at
        FROM plugin_configs
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow!("Failed to load plugin configurations from SQLite: {}", e))?;
    
    Ok(rows)
}

async fn load_proxy_plugin_associations(pool: &Pool<Sqlite>) -> Result<HashMap<String, Vec<String>>> {
    let rows = sqlx::query(
        r#"
        SELECT 
            proxy_id, plugin_config_id
        FROM proxy_plugin_associations
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow!("Failed to load proxy-plugin associations from SQLite: {}", e))?;
    
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
pub async fn get_consumer_by_id(pool: &Pool<Sqlite>, consumer_id: &str) -> Result<Consumer> {
    info!("Fetching consumer from SQLite database by ID: {}", consumer_id);
    
    let row = sqlx::query_as!(
        Consumer,
        r#"
        SELECT 
            id, username, custom_id, credentials, created_at, updated_at
        FROM consumers
        WHERE id = ?
        "#,
        consumer_id
    )
    .fetch_optional(pool)
    .await
    .context("Failed to fetch consumer from SQLite database")?;
    
    match row {
        Some(consumer) => Ok(consumer),
        None => Err(anyhow!("Consumer with ID '{}' not found", consumer_id))
    }
}

/// SQLite implementation of the database client
pub struct SqliteClient {
    pool: SqlitePool,
}

impl SqliteClient {
    pub async fn new(url: &str, max_connections: u32) -> Result<Self> {
        info!("Initializing SQLite database connection");
        
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_timeout(Duration::from_secs(10))
            .connect(url)
            .await
            .map_err(|e| anyhow!("Failed to connect to SQLite database: {}", e))?;
        
        // Ensure the database has the required tables
        Self::ensure_tables(&pool).await?;
        
        info!("Successfully connected to SQLite database");
        
        Ok(Self { pool })
    }
    
    /// Ensure that all required tables exist in the SQLite database
    async fn ensure_tables(pool: &SqlitePool) -> Result<()> {
        debug!("Ensuring SQLite tables exist");
        
        // Create proxies table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS proxies (
                id TEXT PRIMARY KEY,
                name TEXT,
                listen_path TEXT NOT NULL UNIQUE,
                backend_protocol TEXT NOT NULL,
                backend_host TEXT NOT NULL,
                backend_port INTEGER NOT NULL,
                backend_path TEXT,
                strip_listen_path INTEGER NOT NULL DEFAULT 1,
                preserve_host_header INTEGER NOT NULL DEFAULT 0,
                backend_connect_timeout_ms INTEGER NOT NULL,
                backend_read_timeout_ms INTEGER NOT NULL,
                backend_write_timeout_ms INTEGER NOT NULL,
                backend_tls_client_cert_path TEXT,
                backend_tls_client_key_path TEXT,
                backend_tls_verify_server_cert INTEGER NOT NULL DEFAULT 1,
                backend_tls_server_ca_cert_path TEXT,
                dns_override TEXT,
                dns_cache_ttl_seconds INTEGER,
                auth_mode TEXT NOT NULL DEFAULT 'single',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_proxies_listen_path ON proxies(listen_path);
            "#
        )
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create proxies table: {}", e))?;
        
        // Create consumers table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS consumers (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                custom_id TEXT,
                credentials TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_consumers_username ON consumers(username);
            "#
        )
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create consumers table: {}", e))?;
        
        // Create plugin_configs table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS plugin_configs (
                id TEXT PRIMARY KEY,
                plugin_name TEXT NOT NULL,
                config TEXT NOT NULL,
                scope TEXT NOT NULL,
                proxy_id TEXT,
                consumer_id TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (proxy_id) REFERENCES proxies(id) ON DELETE CASCADE,
                FOREIGN KEY (consumer_id) REFERENCES consumers(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_plugin_configs_plugin_name ON plugin_configs(plugin_name);
            CREATE INDEX IF NOT EXISTS idx_plugin_configs_proxy_id ON plugin_configs(proxy_id);
            CREATE INDEX IF NOT EXISTS idx_plugin_configs_consumer_id ON plugin_configs(consumer_id);
            "#
        )
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create plugin_configs table: {}", e))?;
        
        // Create proxy_plugin_associations table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS proxy_plugin_associations (
                proxy_id TEXT NOT NULL,
                plugin_config_id TEXT NOT NULL,
                PRIMARY KEY (proxy_id, plugin_config_id),
                FOREIGN KEY (proxy_id) REFERENCES proxies(id) ON DELETE CASCADE,
                FOREIGN KEY (plugin_config_id) REFERENCES plugin_configs(id) ON DELETE CASCADE
            );
            "#
        )
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create proxy_plugin_associations table: {}", e))?;
        
        debug!("SQLite tables created/verified");
        
        Ok(())
    }
    
    pub async fn load_configuration(&self) -> Result<Configuration> {
        debug!("Loading full configuration from SQLite database");
        
        // Load all proxies
        let proxies = self.load_proxies().await?;
        debug!("Loaded {} proxies", proxies.len());
        
        // Load all consumers
        let consumers = self.load_consumers().await?;
        debug!("Loaded {} consumers", consumers.len());
        
        // Load all plugin configs
        let plugin_configs = self.load_plugin_configs().await?;
        debug!("Loaded {} plugin configs", plugin_configs.len());
        
        // Create association map between proxies and plugins
        let proxy_plugin_map = self.load_proxy_plugin_associations().await?;
        
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
        
        Ok(Configuration {
            proxies: proxies_with_plugins,
            consumers,
            plugin_configs,
            last_updated_at: Utc::now(),
        })
    }
    
    async fn load_proxies(&self) -> Result<Vec<Proxy>> {
        let rows = sqlx::query_as!(
            Proxy,
            r#"
            SELECT 
                id, name, listen_path, 
                backend_protocol as "backend_protocol: String", 
                backend_host, backend_port, backend_path, 
                strip_listen_path, preserve_host_header, 
                backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
                backend_tls_client_cert_path, backend_tls_client_key_path, backend_tls_verify_server_cert,
                backend_tls_server_ca_cert_path, dns_override, dns_cache_ttl_seconds, 
                auth_mode as "auth_mode: String",
                created_at, updated_at
            FROM proxies
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to load proxies from SQLite: {}", e))?;
        
        let mut proxies = Vec::with_capacity(rows.len());
        for proxy in rows {
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
        let rows = sqlx::query_as!(
            Consumer,
            r#"
            SELECT 
                id, username, custom_id, credentials, created_at, updated_at
            FROM consumers
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to load consumers from SQLite: {}", e))?;
        
        Ok(rows)
    }
    
    async fn load_plugin_configs(&self) -> Result<Vec<PluginConfig>> {
        let rows = sqlx::query_as!(
            PluginConfig,
            r#"
            SELECT 
                id, plugin_name, config, 
                scope, proxy_id, consumer_id, 
                enabled, created_at, updated_at
            FROM plugin_configs
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to load plugin configurations from SQLite: {}", e))?;
        
        Ok(rows)
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
        .map_err(|e| anyhow!("Failed to load proxy-plugin associations from SQLite: {}", e))?;
        
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
        
        // SQLite doesn't have native DateTime, convert to ISO8601 strings
        let created_at = proxy.created_at.to_rfc3339();
        let updated_at = proxy.updated_at.to_rfc3339();
        
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
        .bind(if proxy.strip_listen_path { 1 } else { 0 })
        .bind(if proxy.preserve_host_header { 1 } else { 0 })
        .bind(proxy.backend_connect_timeout_ms as i64)
        .bind(proxy.backend_read_timeout_ms as i64)
        .bind(proxy.backend_write_timeout_ms as i64)
        .bind(&proxy.backend_tls_client_cert_path)
        .bind(&proxy.backend_tls_client_key_path)
        .bind(if proxy.backend_tls_verify_server_cert { 1 } else { 0 })
        .bind(&proxy.backend_tls_server_ca_cert_path)
        .bind(&proxy.dns_override)
        .bind(proxy.dns_cache_ttl_seconds.map(|ttl| ttl as i64))
        .bind(auth_mode)
        .bind(created_at)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| anyhow!("Failed to create proxy in SQLite: {}", e))?;
        
        info!("Created proxy with ID: {}", proxy.id);
        
        Ok(proxy.id.clone())
    }
    
    /// Update an existing proxy in the database
    pub async fn update_proxy(&self, proxy: &Proxy) -> Result<()> {
        info!("Updating proxy in SQLite database: {}", proxy.id);
        
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
                updated_at = datetime('now')
            WHERE id = ?
            "#,
            proxy.name,
            proxy.listen_path,
            backend_protocol_str,
            proxy.backend_host,
            proxy.backend_port as i64,
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
        info!("Deleting proxy from SQLite database: {}", proxy_id);
        
        // Check if proxy exists
        let exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM proxies WHERE id = ?) as exists",
            proxy_id
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check proxy existence")?
        .exists;
        
        if exists == 0 {
            return Err(anyhow!("Proxy with ID '{}' does not exist", proxy_id));
        }
        
        // Start a transaction
        let mut tx = self.pool.begin().await.context("Failed to begin transaction")?;
        
        // Delete plugin associations first (due to foreign key constraint)
        sqlx::query!(
            "DELETE FROM proxy_plugin_associations WHERE proxy_id = ?",
            proxy_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete plugin associations")?;
        
        // Delete the proxy
        sqlx::query!(
            "DELETE FROM proxies WHERE id = ?",
            proxy_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete proxy")?;
        
        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;
        
        info!("Deleted proxy with ID: {}", proxy_id);
        Ok(())
    }
    
    /// Create a new consumer in the database
    pub async fn create_consumer(&self, consumer: &Consumer) -> Result<String> {
        info!("Creating new consumer in SQLite database: {}", consumer.username);
        
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
            VALUES (?, ?, ?, ?, datetime('now'), datetime('now'))
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
        info!("Updating consumer in SQLite database: {}", consumer.id);
        
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
                updated_at = datetime('now')
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
        info!("Deleting consumer from SQLite database: {}", consumer_id);
        
        // Check if consumer exists
        let exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM consumers WHERE id = ?) as exists",
            consumer_id
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check consumer existence")?
        .exists;
        
        if exists == 0 {
            return Err(anyhow!("Consumer with ID '{}' does not exist", consumer_id));
        }
        
        // Start a transaction
        let mut tx = self.pool.begin().await.context("Failed to begin transaction")?;
        
        // Delete related plugin configs first
        sqlx::query!(
            "DELETE FROM plugin_configs WHERE consumer_id = ?",
            consumer_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete related plugin configurations")?;
        
        // Delete the consumer
        sqlx::query!(
            "DELETE FROM consumers WHERE id = ?",
            consumer_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete consumer")?;
        
        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;
        
        info!("Deleted consumer with ID: {}", consumer_id);
        Ok(())
    }
    
    /// Create a new plugin configuration in the database
    pub async fn create_plugin_config(&self, plugin_config: &PluginConfig) -> Result<String> {
        info!("Creating new plugin configuration in SQLite database: {}", plugin_config.plugin_name);
        
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
            VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))
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
        info!("Updating plugin configuration in SQLite database: {}", plugin_config.id);
        
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
                updated_at = datetime('now')
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
        info!("Deleting plugin configuration from SQLite database: {}", plugin_config_id);
        
        // Check if plugin config exists
        let exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM plugin_configs WHERE id = ?) as exists",
            plugin_config_id
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check plugin config existence")?
        .exists;
        
        if exists == 0 {
            return Err(anyhow!("Plugin configuration with ID '{}' does not exist", plugin_config_id));
        }
        
        // Start a transaction
        let mut tx = self.pool.begin().await.context("Failed to begin transaction")?;
        
        // Delete any proxy plugin associations first
        sqlx::query!(
            "DELETE FROM proxy_plugin_associations WHERE plugin_config_id = ?",
            plugin_config_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete proxy plugin associations")?;
        
        // Delete the plugin config
        sqlx::query!(
            "DELETE FROM plugin_configs WHERE id = ?",
            plugin_config_id
        )
        .execute(&mut *tx)
        .await
        .context("Failed to delete plugin configuration")?;
        
        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;
        
        info!("Deleted plugin configuration with ID: {}", plugin_config_id);
        Ok(())
    }
}
