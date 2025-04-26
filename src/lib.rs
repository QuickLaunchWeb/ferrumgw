pub mod config;
pub mod database;
pub mod proxy;
pub mod plugins;
pub mod admin;
pub mod modes;
pub mod grpc;
pub mod metrics;
pub mod utils;

// Re-export important types and functions for easier access
pub use config::data_model::{
    Configuration, Proxy, Consumer, PluginConfig, 
    Protocol, AuthMode, 
};
pub use proxy::handler::ProxyHandler;
pub use proxy::router::Router;
pub use database::DatabaseClient;
pub use plugins::Plugin;
pub use plugins::PluginManager;
