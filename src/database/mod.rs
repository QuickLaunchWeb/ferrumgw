use std::sync::Arc;
use anyhow::{Result, Context};
use sqlx::Pool;
use sqlx::postgres::PgPoolOptions;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::sqlite::SqlitePoolOptions;
use tracing::{info, error};
use chrono::{DateTime, Utc};

use crate::config::data_model::{Configuration, DatabaseType, Proxy, Consumer, PluginConfig, ConfigurationDelta};

mod postgres;
mod mysql;
mod sqlite;

#[derive(Debug, Clone)]
pub enum DatabaseType {
    Postgres,
    MySQL,
    SQLite,
}

// Add a flag to disable database features during testing
#[cfg(test)]
const DISABLE_DB_FEATURES: bool = true;

#[cfg(not(test))]
const DISABLE_DB_FEATURES: bool = false;

#[derive(Debug, Clone)]
pub struct DatabaseClient {
    db_type: DatabaseType,
    pool: Arc<DbPool>,
}

// Enum to hold different database connection pools
#[derive(Debug)]
enum DbPool {
    Postgres(Pool<sqlx::Postgres>),
    MySQL(Pool<sqlx::MySql>),
    SQLite(Pool<sqlx::Sqlite>),
}

impl DatabaseClient {
    pub async fn new(db_type: DatabaseType, connection_url: &str) -> Result<Self> {
        info!("Initializing database connection: {:?}", db_type);
        
        let pool = match db_type {
            DatabaseType::Postgres => {
                let pg_pool = PgPoolOptions::new()
                    .max_connections(10)
                    .connect(connection_url)
                    .await
                    .context("Failed to connect to PostgreSQL database")?;
                
                Arc::new(DbPool::Postgres(pg_pool))
            },
            DatabaseType::MySQL => {
                let mysql_pool = MySqlPoolOptions::new()
                    .max_connections(10)
                    .connect(connection_url)
                    .await
                    .context("Failed to connect to MySQL database")?;
                
                Arc::new(DbPool::MySQL(mysql_pool))
            },
            DatabaseType::SQLite => {
                let sqlite_pool = SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect(connection_url)
                    .await
                    .context("Failed to connect to SQLite database")?;
                
                Arc::new(DbPool::SQLite(sqlite_pool))
            },
        };
        
        Ok(Self {
            db_type,
            pool,
        })
    }
    
    pub async fn load_full_configuration(&self) -> Result<Configuration> {
        info!("Loading full configuration from database");
        
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::load_full_configuration(pool).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::MySQL => {
                if let DbPool::MySQL(ref pool) = *self.pool {
                    mysql::load_full_configuration(pool).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::SQLite => {
                if let DbPool::SQLite(ref pool) = *self.pool {
                    sqlite::load_full_configuration(pool).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
        }
    }
    
    /// Load configuration changes since a specific timestamp
    pub async fn load_configuration_delta(&self, since: DateTime<Utc>) -> Result<ConfigurationDelta> {
        info!("Loading configuration delta since {}", since);
        
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::load_configuration_delta(pool, since).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::MySQL => {
                if let DbPool::MySQL(ref pool) = *self.pool {
                    mysql::load_configuration_delta(pool, since).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::SQLite => {
                if let DbPool::SQLite(ref pool) = *self.pool {
                    sqlite::load_configuration_delta(pool, since).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
        }
    }
    
    /// Get the latest database update timestamp without fetching the data
    pub async fn get_latest_update_timestamp(&self) -> Result<DateTime<Utc>> {
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::get_latest_update_timestamp(pool).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::MySQL => {
                if let DbPool::MySQL(ref pool) = *self.pool {
                    mysql::get_latest_update_timestamp(pool).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::SQLite => {
                if let DbPool::SQLite(ref pool) = *self.pool {
                    sqlite::get_latest_update_timestamp(pool).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
        }
    }
    
    // Here we would implement specific CRUD methods for each entity type
    // These would be used by the Admin API to manage the configuration
    
    pub async fn create_proxy(&self, proxy: &Proxy) -> Result<Proxy> {
        // Implementation for creating a proxy in the database
        // Each database adapter will check for listen_path uniqueness
        
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::create_proxy(pool, proxy).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::MySQL => {
                if let DbPool::MySQL(ref pool) = *self.pool {
                    mysql::create_proxy(pool, proxy).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::SQLite => {
                if let DbPool::SQLite(ref pool) = *self.pool {
                    sqlite::create_proxy(pool, proxy).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
        }
    }
    
    /// Get a consumer by its ID from the database
    pub async fn get_consumer_by_id(&self, consumer_id: &str) -> Result<Consumer> {
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::get_consumer_by_id(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::MySQL => {
                if let DbPool::MySQL(ref pool) = *self.pool {
                    mysql::get_consumer_by_id(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::SQLite => {
                if let DbPool::SQLite(ref pool) = *self.pool {
                    sqlite::get_consumer_by_id(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
        }
    }
    
    /// Delete a consumer from the database
    pub async fn delete_consumer(&self, consumer_id: &str) -> Result<()> {
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::delete_consumer(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::MySQL => {
                if let DbPool::MySQL(ref pool) = *self.pool {
                    mysql::delete_consumer(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::SQLite => {
                if let DbPool::SQLite(ref pool) = *self.pool {
                    sqlite::delete_consumer(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
        }
    }
    
    // Create a new plugin config in the database
    // Returns the ID of the newly created plugin config
    pub async fn create_plugin_config(&self, plugin_config: &PluginConfig) -> Result<String> {
        info!("Creating plugin config in database: {}", plugin_config.id);
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::create_plugin_config(pool, plugin_config).await
                } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::MySQL => {
                 if let DbPool::MySQL(ref pool) = *self.pool {
                     mysql::create_plugin_config(pool, plugin_config).await
                 } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::SQLite => {
                 if let DbPool::SQLite(ref pool) = *self.pool {
                     sqlite::create_plugin_config(pool, plugin_config).await
                 } else { unreachable!("Pool type mismatch") }
            },
        }
    }
    
    // Update an existing plugin config in the database
    pub async fn update_plugin_config(&self, plugin_config: &PluginConfig) -> Result<()> {
        info!("Updating plugin config in database: {}", plugin_config.id);
         match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::update_plugin_config(pool, plugin_config).await
                } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::MySQL => {
                 if let DbPool::MySQL(ref pool) = *self.pool {
                     mysql::update_plugin_config(pool, plugin_config).await
                 } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::SQLite => {
                 if let DbPool::SQLite(ref pool) = *self.pool {
                     sqlite::update_plugin_config(pool, plugin_config).await
                 } else { unreachable!("Pool type mismatch") }
            },
        }
    }
    
    // Delete a plugin config from the database
    pub async fn delete_plugin_config(&self, config_id: &str) -> Result<()> {
        info!("Deleting plugin config from database: {}", config_id);
         match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::delete_plugin_config(pool, config_id).await
                } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::MySQL => {
                 if let DbPool::MySQL(ref pool) = *self.pool {
                     mysql::delete_plugin_config(pool, config_id).await
                 } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::SQLite => {
                 if let DbPool::SQLite(ref pool) = *self.pool {
                     sqlite::delete_plugin_config(pool, config_id).await
                 } else { unreachable!("Pool type mismatch") }
            },
        }
    }
     
     // Create a new consumer in the database
    // Returns the ID of the newly created consumer
    pub async fn create_consumer(&self, consumer: &Consumer) -> Result<String> {
        info!("Creating consumer in database: {}", consumer.id);
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::create_consumer(pool, consumer).await
                } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::MySQL => {
                 if let DbPool::MySQL(ref pool) = *self.pool {
                     mysql::create_consumer(pool, consumer).await
                 } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::SQLite => {
                 if let DbPool::SQLite(ref pool) = *self.pool {
                     sqlite::create_consumer(pool, consumer).await
                 } else { unreachable!("Pool type mismatch") }
            },
        }
    }
    
    // Update an existing consumer in the database
    pub async fn update_consumer(&self, consumer: &Consumer) -> Result<()> {
        info!("Updating consumer in database: {}", consumer.id);
         match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::update_consumer(pool, consumer).await
                } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::MySQL => {
                 if let DbPool::MySQL(ref pool) = *self.pool {
                     mysql::update_consumer(pool, consumer).await
                 } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::SQLite => {
                 if let DbPool::SQLite(ref pool) = *self.pool {
                     sqlite::update_consumer(pool, consumer).await
                 } else { unreachable!("Pool type mismatch") }
            },
        }
    }
    
    // Get a consumer by its ID from the database
    pub async fn get_consumer_by_id(&self, consumer_id: &str) -> Result<Consumer> {
        info!("Getting consumer from database: {}", consumer_id);
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::get_consumer_by_id(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::MySQL => {
                if let DbPool::MySQL(ref pool) = *self.pool {
                    mysql::get_consumer_by_id(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
            DatabaseType::SQLite => {
                if let DbPool::SQLite(ref pool) = *self.pool {
                    sqlite::get_consumer_by_id(pool, consumer_id).await
                } else {
                    unreachable!("Pool type mismatch with database type")
                }
            },
        }
    }
     
     // Get a proxy by its ID from the database
    pub async fn get_proxy_by_id(&self, proxy_id: &str) -> Result<Proxy> {
        info!("Getting proxy from database: {}", proxy_id);
        match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::get_proxy_by_id(pool, proxy_id).await
                } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::MySQL => {
                 if let DbPool::MySQL(ref pool) = *self.pool {
                     mysql::get_proxy_by_id(pool, proxy_id).await
                 } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::SQLite => {
                 if let DbPool::SQLite(ref pool) = *self.pool {
                     sqlite::get_proxy_by_id(pool, proxy_id).await
                 } else { unreachable!("Pool type mismatch") }
            },
        }
    }

    // Update an existing proxy in the database
    pub async fn update_proxy(&self, proxy: &Proxy) -> Result<()> {
        info!("Updating proxy in database: {}", proxy.id);
         match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::update_proxy(pool, proxy).await
                } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::MySQL => {
                 if let DbPool::MySQL(ref pool) = *self.pool {
                     mysql::update_proxy(pool, proxy).await
                 } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::SQLite => {
                 if let DbPool::SQLite(ref pool) = *self.pool {
                     sqlite::update_proxy(pool, proxy).await
                 } else { unreachable!("Pool type mismatch") }
            },
        }
    }

    // Delete a proxy by its ID from the database
    pub async fn delete_proxy(&self, proxy_id: &str) -> Result<()> {
        info!("Deleting proxy from database: {}", proxy_id);
         match self.db_type {
            DatabaseType::Postgres => {
                if let DbPool::Postgres(ref pool) = *self.pool {
                    postgres::delete_proxy(pool, proxy_id).await
                } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::MySQL => {
                 if let DbPool::MySQL(ref pool) = *self.pool {
                     mysql::delete_proxy(pool, proxy_id).await
                 } else { unreachable!("Pool type mismatch") }
            },
            DatabaseType::SQLite => {
                 if let DbPool::SQLite(ref pool) = *self.pool {
                     sqlite::delete_proxy(pool, proxy_id).await
                 } else { unreachable!("Pool type mismatch") }
            },
        }
    }
}
