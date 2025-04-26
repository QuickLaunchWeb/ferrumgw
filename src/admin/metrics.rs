use std::sync::Arc;
use std::collections::HashMap;
use anyhow::Result;
use hyper::{Body, Response, StatusCode};
use serde::Serialize;
use chrono::{DateTime, Utc};

use crate::modes::OperationMode;
use crate::admin::AdminApiState;

/// Runtime metrics for the gateway
#[derive(Debug, Serialize)]
struct Metrics {
    /// Current operation mode
    mode: String,
    
    /// Timestamp of the last configuration update
    config_last_updated_at: DateTime<Utc>,
    
    /// Status of the configuration source (database, CP connection)
    config_source_status: ConfigSourceStatus,
    
    /// Number of loaded proxies
    proxy_count: usize,
    
    /// Number of loaded consumers
    consumer_count: usize,
    
    /// Approximate requests per second in the last second
    #[serde(rename = "requests_per_second_current")]
    rps_current: f64,
    
    /// Map of response status codes to counts observed in the last second
    status_codes_last_second: HashMap<String, usize>,
}

/// Enum representing the status of the configuration source
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum ConfigSourceStatus {
    Online,
    Offline,
    NA,
    Connecting,
    Degraded,
    Error,
    Unknown,
}

/// Handler for the /admin/metrics endpoint
pub async fn get_metrics(state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Create the metrics object
    let metrics = Metrics {
        mode: state.operation_mode.clone(),
        config_last_updated_at: config.last_updated_at,
        config_source_status: match state.metrics_collector.get_config_source_status() {
            "online" => ConfigSourceStatus::Online,
            "connecting" => ConfigSourceStatus::Connecting,
            "degraded" => ConfigSourceStatus::Degraded,
            "error" => ConfigSourceStatus::Error,
            _ => ConfigSourceStatus::Unknown,
        },
        proxy_count: config.proxies.len(),
        consumer_count: config.consumers.len(),
        rps_current: state.metrics_collector.get_requests_per_second(),
        status_codes_last_second: state.metrics_collector.get_status_code_counts().await,
    };
    
    // Serialize to JSON
    let json = serde_json::to_string(&metrics)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}
