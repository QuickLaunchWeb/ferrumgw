// Include the generated protobuf code
pub mod ferrumgw {
    pub mod config {
        tonic::include_proto!("ferrumgw.config");
    }
}

// Re-export all the important types from the generated code
pub use self::ferrumgw::config::{
    // Messages
    ConfigSnapshot, ConfigUpdate, ConfigDelta,
    Proxy, Consumer, PluginConfig,
    SubscribeRequest, SnapshotRequest,
    HealthReport, HealthAck,
    
    // Enums
    UpdateType, Protocol, AuthMode,
    
    // Service traits
    config_service_server::{ConfigService, ConfigServiceServer},
    config_service_client::ConfigServiceClient,
};
