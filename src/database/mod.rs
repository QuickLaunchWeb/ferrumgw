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
    
    pub async fn create_proxy(&self, proxy: Proxy) -> Result<Proxy> {
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
    
    // Similar methods would be implemented for other CRUD operations
    // and other entity types (Consumer, PluginConfig)
}
