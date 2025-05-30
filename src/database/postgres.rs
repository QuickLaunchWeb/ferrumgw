use anyhow::{Result, Context};
use sqlx::{Pool, Postgres, Transaction};
use tracing::{info, error, debug};
use chrono::Utc;
use std::collections::HashMap;
use serde_json::Value;

use crate::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, PluginAssociation, Protocol, AuthMode};

#[cfg(test)]
const SKIP_DELETION_TRACKING: bool = true;

#[cfg(not(test))]
const SKIP_DELETION_TRACKING: bool = false;

/// Load the full configuration from the PostgreSQL database
pub async fn load_full_configuration(pool: &Pool<Postgres>) -> Result<Configuration> {
    info!("Loading full configuration from PostgreSQL database");
    
    // Begin a transaction to ensure consistent data
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Load proxies
    let mut proxies = sqlx::query_as!(
        Proxy,
        r#"
        SELECT 
            id,
            name, listen_path, backend_protocol as "backend_protocol: String", 
            backend_host, backend_port, backend_path,
            strip_listen_path, preserve_host_header,
            backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
            backend_tls_client_cert_path, backend_tls_client_key_path, backend_tls_verify_server_cert,
            backend_tls_server_ca_cert_path, 
            dns_override, dns_cache_ttl_seconds,
            auth_mode as "auth_mode: String",
            created_at, updated_at
        FROM proxies
        ORDER BY created_at
        "#
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to fetch proxies from database")?;
    
    // Parse the protocol and auth_mode strings to enums
    for proxy in &mut proxies {
        proxy.backend_protocol = match proxy.backend_protocol.as_str() {
            "http" => Protocol::Http,
            "https" => Protocol::Https,
            "ws" => Protocol::Ws,
            "wss" => Protocol::Wss,
            "grpc" => Protocol::Grpc,
            _ => Protocol::Http,
        };
        
        proxy.auth_mode = match proxy.auth_mode.as_str() {
            "single" => AuthMode::Single,
            "multi" => AuthMode::Multi,
            _ => AuthMode::Single,
        };
    }
    
    // Load consumers
    let consumers = sqlx::query_as!(
        Consumer,
        r#"
        SELECT 
            id, username, custom_id, credentials, created_at, updated_at
        FROM consumers
        ORDER BY created_at
        "#
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to fetch consumers from database")?;
    
    // Load plugin configs
    let plugin_configs = sqlx::query_as!(
        PluginConfig,
        r#"
        SELECT 
            id, plugin_name, config, scope, proxy_id, consumer_id, 
            enabled, created_at, updated_at
        FROM plugin_configs
        ORDER BY created_at
        "#
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to fetch plugin configurations from database")?;
    
    // Load plugin associations for each proxy
    for proxy in &mut proxies {
        // For each proxy, load its associated plugins
        let plugin_associations = sqlx::query!(
            r#"
            SELECT plugin_config_id, embedded_config
            FROM proxy_plugin_associations
            WHERE proxy_id = $1
            ORDER BY id
            "#,
            proxy.id
        )
        .fetch_all(&mut *tx)
        .await
        .context(format!("Failed to load plugin associations for proxy {}", proxy.id))?;
        
        // Transform the database rows into the PluginAssociation model
        proxy.plugins = plugin_associations
            .into_iter()
            .map(|row| PluginAssociation {
                plugin_config_id: row.plugin_config_id,
                embedded_config: row.embedded_config.map(|v| serde_json::from_value(v).unwrap_or_default()),
            })
            .collect();
    }
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    // Get the latest update timestamp
    let last_updated_at = proxies
        .iter()
        .map(|p| p.updated_at)
        .chain(consumers.iter().map(|c| c.updated_at))
        .chain(plugin_configs.iter().map(|pc| pc.updated_at))
        .max()
        .unwrap_or_else(Utc::now);
    
    let config = Configuration {
        proxies,
        consumers,
        plugin_configs,
        last_updated_at,
    };
    
    debug!("Loaded {} proxies, {} consumers, and {} plugin configs", 
        config.proxies.len(), config.consumers.len(), config.plugin_configs.len());
    
    Ok(config)
}

/// Create a new proxy in the database
pub async fn create_proxy(pool: &Pool<Postgres>, proxy: Proxy) -> Result<Proxy> {
    info!("Creating new proxy in PostgreSQL database: {}", proxy.id);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Check if listen_path already exists
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM proxies WHERE listen_path = $1) as exists_flag",
        proxy.listen_path
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check listen_path uniqueness")?
    .exists_flag
    .unwrap_or(false);
    
    if exists {
        anyhow::bail!("Proxy with listen_path '{}' already exists", proxy.listen_path);
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
    
    // Insert the proxy
    let inserted_proxy = sqlx::query!(
        r#"
        INSERT INTO proxies (
            name, listen_path, backend_protocol, backend_host, backend_port, backend_path,
            strip_listen_path, preserve_host_header,
            backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
            backend_tls_client_cert_path, backend_tls_client_key_path,
            backend_tls_verify_server_cert, backend_tls_server_ca_cert_path,
            dns_override, dns_cache_ttl_seconds, auth_mode
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        RETURNING id, created_at, updated_at
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
        auth_mode_str
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to insert proxy")?;
    
    // Insert plugin associations
    for plugin_assoc in &proxy.plugins {
        sqlx::query!(
            r#"
            INSERT INTO proxy_plugin_associations (
                proxy_id, plugin_config_id, embedded_config
            )
            VALUES ($1, $2, $3)
            "#,
            inserted_proxy.id,
            plugin_assoc.plugin_config_id,
            plugin_assoc.embedded_config.as_ref().map(|c| serde_json::to_value(c).unwrap_or_default())
        )
        .execute(&mut *tx)
        .await
        .context("Failed to insert plugin association")?;
    }
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    // Load the newly created proxy
    let mut new_proxy = proxy.clone();
    new_proxy.id = inserted_proxy.id;
    new_proxy.created_at = inserted_proxy.created_at;
    new_proxy.updated_at = inserted_proxy.updated_at;
    
    info!("Created new proxy with ID: {}", new_proxy.id);
    Ok(new_proxy)
}

/// Update an existing proxy in the database
pub async fn update_proxy(pool: &Pool<Postgres>, proxy: Proxy) -> Result<Proxy> {
    info!("Updating proxy in PostgreSQL database: {}", proxy.id);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Check if proxy exists
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM proxies WHERE id = $1) as exists_flag",
        proxy.id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check proxy existence")?
    .exists_flag
    .unwrap_or(false);
    
    if !exists {
        anyhow::bail!("Proxy with ID '{}' does not exist", proxy.id);
    }
    
    // Check if the new listen_path would conflict with another proxy
    let path_exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM proxies WHERE listen_path = $1 AND id != $2) as exists_flag",
        proxy.listen_path, proxy.id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check listen_path uniqueness")?
    .exists_flag
    .unwrap_or(false);
    
    if path_exists {
        anyhow::bail!("Another proxy with listen_path '{}' already exists", proxy.listen_path);
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
    
    // Update the proxy
    let updated = sqlx::query!(
        r#"
        UPDATE proxies
        SET 
            name = $1,
            listen_path = $2,
            backend_protocol = $3,
            backend_host = $4,
            backend_port = $5,
            backend_path = $6,
            strip_listen_path = $7,
            preserve_host_header = $8,
            backend_connect_timeout_ms = $9,
            backend_read_timeout_ms = $10,
            backend_write_timeout_ms = $11,
            backend_tls_client_cert_path = $12,
            backend_tls_client_key_path = $13,
            backend_tls_verify_server_cert = $14,
            backend_tls_server_ca_cert_path = $15,
            dns_override = $16,
            dns_cache_ttl_seconds = $17,
            auth_mode = $18,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $19
        RETURNING updated_at
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
    .fetch_one(&mut *tx)
    .await
    .context("Failed to update proxy")?;
    
    // Delete existing plugin associations
    sqlx::query!(
        "DELETE FROM proxy_plugin_associations WHERE proxy_id = $1",
        proxy.id
    )
    .execute(&mut *tx)
    .await
    .context("Failed to delete existing plugin associations")?;
    
    // Insert new plugin associations
    for plugin_assoc in &proxy.plugins {
        sqlx::query!(
            r#"
            INSERT INTO proxy_plugin_associations (
                proxy_id, plugin_config_id, embedded_config
            )
            VALUES ($1, $2, $3)
            "#,
            proxy.id,
            plugin_assoc.plugin_config_id,
            plugin_assoc.embedded_config.as_ref().map(|c| serde_json::to_value(c).unwrap_or_default())
        )
        .execute(&mut *tx)
        .await
        .context("Failed to insert plugin association")?;
    }
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    // Return the updated proxy
    let mut updated_proxy = proxy.clone();
    updated_proxy.updated_at = updated.updated_at;
    
    info!("Updated proxy with ID: {}", updated_proxy.id);
    Ok(updated_proxy)
}

/// Delete a proxy from the database
pub async fn delete_proxy(pool: &Pool<Postgres>, proxy_id: &str) -> Result<()> {
    info!("Deleting proxy with ID: {}", proxy_id);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // First, delete any plugin associations
    sqlx::query!(
        r#"
        DELETE FROM proxy_plugin_associations
        WHERE proxy_id = $1
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
        WHERE id = $1
        "#,
        proxy_id
    )
    .execute(&mut *tx)
    .await
    .context("Failed to delete proxy")?;
    
    // Insert into proxy_deletions table for incremental updates if it exists
    if delete_result.rows_affected() > 0 && !SKIP_DELETION_TRACKING {
        // Try to insert, but don't fail if the table doesn't exist yet
        // This handles the case where the migration for incremental updates hasn't been applied
        let track_result = sqlx::query!(
            r#"
            INSERT INTO proxy_deletions (id, deleted_at)
            VALUES ($1, CURRENT_TIMESTAMP)
            ON CONFLICT (id) DO UPDATE
            SET deleted_at = CURRENT_TIMESTAMP
            "#,
            proxy_id
        )
        .execute(&mut *tx)
        .await;
        
        // Log but don't fail if tracking table doesn't exist
        if let Err(e) = track_result {
            debug!("Could not track proxy deletion in proxy_deletions table (this is expected if using an older schema version): {}", e);
        }
    }
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    info!("Deleted proxy with ID: {}", proxy_id);
    Ok(())
}

/// Create a new consumer in the database
pub async fn create_consumer(pool: &Pool<Postgres>, consumer: Consumer) -> Result<Consumer> {
    info!("Creating new consumer in PostgreSQL database: {}", consumer.username);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Check if username is unique
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM consumers WHERE username = $1) as exists_flag",
        consumer.username
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check username uniqueness")?
    .exists_flag
    .unwrap_or(false);
    
    if exists {
        anyhow::bail!("A consumer with username '{}' already exists", consumer.username);
    }
    
    // If custom_id is provided, check if it's unique
    if let Some(custom_id) = &consumer.custom_id {
        let custom_id_exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM consumers WHERE custom_id = $1) as exists_flag",
            custom_id
        )
        .fetch_one(&mut *tx)
        .await
        .context("Failed to check custom_id uniqueness")?
        .exists_flag
        .unwrap_or(false);
        
        if custom_id_exists {
            anyhow::bail!("A consumer with custom_id '{}' already exists", custom_id);
        }
    }
    
    // Serialize credentials to JSON
    let credentials_json = serde_json::to_value(&consumer.credentials)
        .context("Failed to serialize consumer credentials")?;
    
    // Insert the consumer
    let inserted = sqlx::query!(
        r#"
        INSERT INTO consumers (
            username, custom_id, credentials
        )
        VALUES ($1, $2, $3)
        RETURNING id, created_at, updated_at
        "#,
        consumer.username,
        consumer.custom_id,
        credentials_json
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to insert consumer")?;
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    // Load the newly created consumer
    let mut new_consumer = consumer.clone();
    new_consumer.id = inserted.id;
    new_consumer.created_at = inserted.created_at;
    new_consumer.updated_at = inserted.updated_at;
    
    info!("Created new consumer with ID: {}", new_consumer.id);
    Ok(new_consumer)
}

/// Update an existing consumer in the database
pub async fn update_consumer(pool: &Pool<Postgres>, consumer: Consumer) -> Result<Consumer> {
    info!("Updating consumer in PostgreSQL database: {}", consumer.id);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Check if consumer exists
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM consumers WHERE id = $1) as exists_flag",
        consumer.id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check consumer existence")?
    .exists_flag
    .unwrap_or(false);
    
    if !exists {
        anyhow::bail!("Consumer with ID '{}' does not exist", consumer.id);
    }
    
    // Check username uniqueness
    let username_exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM consumers WHERE username = $1 AND id != $2) as exists_flag",
        consumer.username, consumer.id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check username uniqueness")?
    .exists_flag
    .unwrap_or(false);
    
    if username_exists {
        anyhow::bail!("Another consumer with username '{}' already exists", consumer.username);
    }
    
    // If custom_id is provided, check if it's unique
    if let Some(custom_id) = &consumer.custom_id {
        let custom_id_exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM consumers WHERE custom_id = $1 AND id != $2) as exists_flag",
            custom_id, consumer.id
        )
        .fetch_one(&mut *tx)
        .await
        .context("Failed to check custom_id uniqueness")?
        .exists_flag
        .unwrap_or(false);
        
        if custom_id_exists {
            anyhow::bail!("Another consumer with custom_id '{}' already exists", custom_id);
        }
    }
    
    // Serialize credentials to JSON
    let credentials_json = serde_json::to_value(&consumer.credentials)
        .context("Failed to serialize consumer credentials")?;
    
    // Update the consumer
    let updated = sqlx::query!(
        r#"
        UPDATE consumers
        SET 
            username = $1,
            custom_id = $2,
            credentials = $3,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $4
        RETURNING updated_at
        "#,
        consumer.username,
        consumer.custom_id,
        credentials_json,
        consumer.id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to update consumer")?;
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    // Return the updated consumer
    let mut updated_consumer = consumer.clone();
    updated_consumer.updated_at = updated.updated_at;
    
    info!("Updated consumer with ID: {}", updated_consumer.id);
    Ok(updated_consumer)
}

/// Delete a consumer from the database
pub async fn delete_consumer(pool: &Pool<Postgres>, consumer_id: &str) -> Result<()> {
    info!("Deleting consumer with ID: {}", consumer_id);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Check if consumer exists
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM consumers WHERE id = $1) as exists_flag",
        consumer_id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check consumer existence")?
    .exists_flag
    .unwrap_or(false);
    
    if !exists {
        anyhow::bail!("Consumer with ID '{}' does not exist", consumer_id);
    }
    
    // Delete the consumer
    let delete_result = sqlx::query!(
        "DELETE FROM consumers WHERE id = $1",
        consumer_id
    )
    .execute(&mut *tx)
    .await
    .context("Failed to delete consumer")?;
    
    // Insert into consumer_deletions table for incremental updates if it exists
    if delete_result.rows_affected() > 0 && !SKIP_DELETION_TRACKING {
        // Try to insert, but don't fail if the table doesn't exist yet
        let track_result = sqlx::query!(
            r#"
            INSERT INTO consumer_deletions (id, deleted_at)
            VALUES ($1, CURRENT_TIMESTAMP)
            ON CONFLICT (id) DO UPDATE
            SET deleted_at = CURRENT_TIMESTAMP
            "#,
            consumer_id
        )
        .execute(&mut *tx)
        .await;
        
        // Log but don't fail if tracking table doesn't exist
        if let Err(e) = track_result {
            debug!("Could not track consumer deletion in consumer_deletions table (this is expected if using an older schema version): {}", e);
        }
    }
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    info!("Deleted consumer with ID: {}", consumer_id);
    Ok(())
}

/// Get a consumer by ID from the database
pub async fn get_consumer_by_id(pool: &Pool<Postgres>, consumer_id: &str) -> Result<Consumer> {
    info!("Fetching consumer from PostgreSQL database by ID: {}", consumer_id);
    
    let row = sqlx::query!(
        r#"
        SELECT 
            id, username, custom_id, credentials, created_at, updated_at
        FROM consumers
        WHERE id = $1
        "#,
        consumer_id
    )
    .fetch_optional(pool)
    .await
    .context("Failed to fetch consumer from PostgreSQL database")?;
    
    match row {
        Some(row) => {
            // PostgreSQL stores credentials as JSONB which we need to convert to HashMap
            let credentials = match row.credentials {
                Some(jsonb) => {
                    // Convert JSONB to HashMap
                    serde_json::from_value::<HashMap<String, Value>>(jsonb.clone())
                        .unwrap_or_else(|_| HashMap::new())
                },
                None => HashMap::new(),
            };
            
            Ok(Consumer {
                id: row.id,
                username: row.username,
                custom_id: row.custom_id,
                credentials,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        },
        None => Err(anyhow!("Consumer with ID '{}' not found", consumer_id))
    }
}

/// Create a new plugin configuration in the database
pub async fn create_plugin_config(pool: &Pool<Postgres>, plugin_config: PluginConfig) -> Result<PluginConfig> {
    info!("Creating new plugin configuration in PostgreSQL database: {}", plugin_config.plugin_name);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Serialize config to JSON
    let config_json = serde_json::to_value(&plugin_config.config)
        .context("Failed to serialize plugin configuration")?;
    
    // Insert the plugin config
    let inserted = sqlx::query!(
        r#"
        INSERT INTO plugin_configs (
            plugin_name, config, scope, proxy_id, consumer_id, enabled
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, created_at, updated_at
        "#,
        plugin_config.plugin_name,
        config_json,
        plugin_config.scope,
        plugin_config.proxy_id,
        plugin_config.consumer_id,
        plugin_config.enabled
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to insert plugin configuration")?;
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    // Load the newly created plugin config
    let mut new_plugin_config = plugin_config.clone();
    new_plugin_config.id = inserted.id;
    new_plugin_config.created_at = inserted.created_at;
    new_plugin_config.updated_at = inserted.updated_at;
    
    info!("Created new plugin configuration with ID: {}", new_plugin_config.id);
    Ok(new_plugin_config)
}

/// Update an existing plugin configuration in the database
pub async fn update_plugin_config(pool: &Pool<Postgres>, plugin_config: PluginConfig) -> Result<PluginConfig> {
    info!("Updating plugin configuration in PostgreSQL database: {}", plugin_config.id);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Check if plugin config exists
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM plugin_configs WHERE id = $1) as exists_flag",
        plugin_config.id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check plugin config existence")?
    .exists_flag
    .unwrap_or(false);
    
    if !exists {
        anyhow::bail!("Plugin configuration with ID '{}' does not exist", plugin_config.id);
    }
    
    // Serialize config to JSON
    let config_json = serde_json::to_value(&plugin_config.config)
        .context("Failed to serialize plugin configuration")?;
    
    // Update the plugin config
    let updated = sqlx::query!(
        r#"
        UPDATE plugin_configs
        SET 
            plugin_name = $1,
            config = $2,
            scope = $3,
            proxy_id = $4,
            consumer_id = $5,
            enabled = $6,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $7
        RETURNING updated_at
        "#,
        plugin_config.plugin_name,
        config_json,
        plugin_config.scope,
        plugin_config.proxy_id,
        plugin_config.consumer_id,
        plugin_config.enabled,
        plugin_config.id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to update plugin configuration")?;
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    // Return the updated plugin config
    let mut updated_plugin_config = plugin_config.clone();
    updated_plugin_config.updated_at = updated.updated_at;
    
    info!("Updated plugin configuration with ID: {}", updated_plugin_config.id);
    Ok(updated_plugin_config)
}

/// Delete a plugin configuration from the database
pub async fn delete_plugin_config(pool: &Pool<Postgres>, plugin_config_id: &str) -> Result<()> {
    info!("Deleting plugin configuration with ID: {}", plugin_config_id);
    
    // Begin a transaction
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Check if plugin config exists
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM plugin_configs WHERE id = $1) as exists_flag",
        plugin_config_id
    )
    .fetch_one(&mut *tx)
    .await
    .context("Failed to check plugin config existence")?
    .exists_flag
    .unwrap_or(false);
    
    if !exists {
        anyhow::bail!("Plugin configuration with ID '{}' does not exist", plugin_config_id);
    }
    
    // First, delete any proxy associations
    sqlx::query!(
        r#"
        DELETE FROM proxy_plugin_associations
        WHERE plugin_config_id = $1
        "#,
        plugin_config_id
    )
    .execute(&mut *tx)
    .await
    .context("Failed to delete plugin-proxy associations")?;
    
    // Then delete the plugin config itself
    let delete_result = sqlx::query!(
        r#"
        DELETE FROM plugin_configs
        WHERE id = $1
        "#,
        plugin_config_id
    )
    .execute(&mut *tx)
    .await
    .context("Failed to delete plugin configuration")?;
    
    // Insert into plugin_config_deletions table for incremental updates if it exists
    if delete_result.rows_affected() > 0 && !SKIP_DELETION_TRACKING {
        // Try to insert, but don't fail if the table doesn't exist yet
        let track_result = sqlx::query!(
            r#"
            INSERT INTO plugin_config_deletions (id, deleted_at)
            VALUES ($1, CURRENT_TIMESTAMP)
            ON CONFLICT (id) DO UPDATE
            SET deleted_at = CURRENT_TIMESTAMP
            "#,
            plugin_config_id
        )
        .execute(&mut *tx)
        .await;
        
        // Log but don't fail if tracking table doesn't exist
        if let Err(e) = track_result {
            debug!("Could not track plugin config deletion in plugin_config_deletions table (this is expected if using an older schema version): {}", e);
        }
    }
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    info!("Deleted plugin configuration with ID: {}", plugin_config_id);
    Ok(())
}

/// Get the latest update timestamp from the database
pub async fn get_latest_update_timestamp(pool: &Pool<Postgres>) -> Result<chrono::DateTime<chrono::Utc>> {
    debug!("Getting latest update timestamp from PostgreSQL database");
    
    // Use a query that combines the latest timestamps from all tables
    let result = sqlx::query!(
        r#"
        SELECT MAX(latest_time) as "max_time: chrono::DateTime<chrono::Utc>"
        FROM (
            SELECT MAX(updated_at) as latest_time FROM proxies
            UNION ALL
            SELECT MAX(updated_at) as latest_time FROM consumers
            UNION ALL
            SELECT MAX(updated_at) as latest_time FROM plugin_configs
        ) as latest_updates
        "#
    )
    .fetch_one(pool)
    .await
    .context("Failed to get latest update timestamp")?;
    
    // If there are no records, return the current time
    match result.max_time {
        Some(time) => Ok(time),
        None => Ok(Utc::now()),
    }
}

/// Load configuration changes since a specific timestamp
pub async fn load_configuration_delta(pool: &Pool<Postgres>, since: chrono::DateTime<chrono::Utc>) -> Result<crate::config::data_model::ConfigurationDelta> {
    info!("Loading configuration delta from PostgreSQL database since {}", since);
    
    // Begin a transaction to ensure consistent data
    let mut tx = pool.begin().await.context("Failed to begin transaction")?;
    
    // Load updated proxies
    let updated_proxies = sqlx::query_as!(
        Proxy,
        r#"
        SELECT 
            id,
            name, listen_path, backend_protocol as "backend_protocol: String", 
            backend_host, backend_port, backend_path,
            strip_listen_path, preserve_host_header,
            backend_connect_timeout_ms, backend_read_timeout_ms, backend_write_timeout_ms,
            backend_tls_client_cert_path, backend_tls_client_key_path, backend_tls_verify_server_cert,
            backend_tls_server_ca_cert_path, 
            dns_override, dns_cache_ttl_seconds,
            auth_mode as "auth_mode: String",
            created_at, updated_at
        FROM proxies
        WHERE updated_at > $1
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
        proxy.backend_protocol = match proxy.backend_protocol.as_str() {
            "http" => Protocol::Http,
            "https" => Protocol::Https,
            "ws" => Protocol::Ws,
            "wss" => Protocol::Wss,
            "grpc" => Protocol::Grpc,
            _ => Protocol::Http,
        };
        
        proxy.auth_mode = match proxy.auth_mode.as_str() {
            "multi" => AuthMode::Multi,
            _ => AuthMode::Single,
        };
        
        // Load any plugin associations for this proxy
        proxy.plugins = sqlx::query_as!(
            PluginAssociation,
            r#"
            SELECT plugin_config_id, embedded_config as "embedded_config: Value"
            FROM proxy_plugin_associations
            WHERE proxy_id = $1
            "#,
            proxy.id
        )
        .fetch_all(&mut *tx)
        .await
        .context("Failed to fetch plugin associations")?;
        
        processed_proxies.push(proxy);
    }
    
    let deleted_proxy_ids = if SKIP_DELETION_TRACKING {
        debug!("Skipping deleted proxy tracking in test mode");
        Vec::new()
    } else {
        // Get IDs of deleted proxies (try with error handling for missing table)
        match sqlx::query!(
            r#"
            SELECT id
            FROM proxy_deletions
            WHERE deleted_at > $1
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await {
            Ok(rows) => rows.into_iter().map(|row| row.id).collect::<Vec<String>>(),
            Err(e) => {
                debug!("Could not fetch deleted proxy IDs (proxy_deletions table may not exist yet): {}", e);
                Vec::new()
            }
        }
    };
    
    // Load updated consumers
    let updated_consumers = sqlx::query_as!(
        Consumer,
        r#"
        SELECT 
            id, username, custom_id,
            credentials as "credentials: Value",
            created_at, updated_at
        FROM consumers
        WHERE updated_at > $1
        ORDER BY updated_at
        "#,
        since
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to fetch updated consumers from database")?;
    
    let deleted_consumer_ids = if SKIP_DELETION_TRACKING {
        debug!("Skipping deleted consumer tracking in test mode");
        Vec::new()
    } else {
        // Get IDs of deleted consumers (try with error handling for missing table)
        match sqlx::query!(
            r#"
            SELECT id
            FROM consumer_deletions
            WHERE deleted_at > $1
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await {
            Ok(rows) => rows.into_iter().map(|row| row.id).collect::<Vec<String>>(),
            Err(e) => {
                debug!("Could not fetch deleted consumer IDs (consumer_deletions table may not exist yet): {}", e);
                Vec::new()
            }
        }
    };
    
    // Load updated plugin configs
    let updated_plugin_configs = sqlx::query_as!(
        PluginConfig,
        r#"
        SELECT 
            id, plugin_name,
            config as "config: Value",
            scope as "scope: String",
            proxy_id,
            enabled,
            created_at, updated_at
        FROM plugin_configs
        WHERE updated_at > $1
        ORDER BY updated_at
        "#,
        since
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to fetch updated plugin configs from database")?;
    
    // Parse the scope enum
    let mut processed_plugin_configs = Vec::with_capacity(updated_plugin_configs.len());
    for mut plugin_config in updated_plugin_configs {
        plugin_config.scope = match plugin_config.scope.as_str() {
            "proxy" => crate::config::data_model::PluginScope::Proxy,
            _ => crate::config::data_model::PluginScope::Global,
        };
        
        processed_plugin_configs.push(plugin_config);
    }
    
    let deleted_plugin_config_ids = if SKIP_DELETION_TRACKING {
        debug!("Skipping deleted plugin config tracking in test mode");
        Vec::new()
    } else {
        // Get IDs of deleted plugin configs (try with error handling for missing table)
        match sqlx::query!(
            r#"
            SELECT id
            FROM plugin_config_deletions
            WHERE deleted_at > $1
            "#,
            since
        )
        .fetch_all(&mut *tx)
        .await {
            Ok(rows) => rows.into_iter().map(|row| row.id).collect::<Vec<String>>(),
            Err(e) => {
                debug!("Could not fetch deleted plugin config IDs (plugin_config_deletions table may not exist yet): {}", e);
                Vec::new()
            }
        }
    };
    
    // Get the latest update timestamp
    let latest_timestamp = get_latest_update_timestamp(pool).await?;
    
    // Commit the transaction
    tx.commit().await.context("Failed to commit transaction")?;
    
    Ok(crate::config::data_model::ConfigurationDelta {
        updated_proxies: processed_proxies,
        deleted_proxy_ids,
        updated_consumers,
        deleted_consumer_ids,
        updated_plugin_configs: processed_plugin_configs,
        deleted_plugin_config_ids,
        last_updated_at: latest_timestamp,
    })
}
