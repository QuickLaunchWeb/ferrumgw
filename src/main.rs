use std::env;
use std::process::exit;
use tracing::{error, info};

mod config;
mod database;
mod modes;
mod plugins;
mod proxy;
mod admin;
mod utils;
mod metrics;

use config::env_config::EnvConfig;
use modes::OperationMode;

#[tokio::main]
async fn main() {
    // Initialize logging
    initialize_logging();
    
    // Load environment configuration
    let env_config = match EnvConfig::from_env() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load environment configuration: {}", e);
            exit(1);
        }
    };
    
    info!("Starting Ferrum Gateway v{}", env!("CARGO_PKG_VERSION"));
    info!("Operation mode: {}", env_config.mode);
    
    // Initialize the gateway based on operation mode
    let result = match env_config.mode {
        OperationMode::Database => modes::database::run(env_config).await,
        OperationMode::File => modes::file::run(env_config).await,
        OperationMode::ControlPlane => modes::control_plane::run(env_config).await,
        OperationMode::DataPlane => modes::data_plane::run(env_config).await,
    };
    
    // Handle result
    if let Err(e) = result {
        error!("Gateway operation failed: {}", e);
        exit(1);
    }
    
    info!("Ferrum Gateway shut down successfully");
}

fn initialize_logging() {
    let log_level = env::var("FERRUM_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(format!("ferrumgw={}", log_level))
        });
    
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .init();
}
