use anyhow::Result;
use prometheus::{register_counter_vec, register_histogram_vec, register_int_counter, register_int_gauge};
use prometheus::{Counter, CounterVec, Histogram, HistogramVec, IntCounter, IntGauge};
use prometheus::Encoder;
use prometheus::TextEncoder;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::RwLock;

use crate::config::data_model::Configuration;

/// MetricsCollector manages all the metrics collection for the Ferrum Gateway
pub struct MetricsCollector {
    config: Arc<RwLock<Configuration>>,
    mode: String,
    // Store the most recent RPS value for admin API reporting
    recent_rps: Arc<RwLock<f64>>,
    // Store status code counts for the last second
    recent_status_codes: Arc<RwLock<HashMap<u16, u64>>>,
    last_operation_success: Arc<AtomicBool>,
}

lazy_static! {
    // Basic gateway metrics
    static ref PROXY_REQUESTS_TOTAL: IntCounter = register_int_counter!(
        "ferrumgw_proxy_requests_total",
        "Total number of requests processed by the proxy"
    ).unwrap();

    static ref PROXY_REQUESTS_ACTIVE: IntGauge = register_int_gauge!(
        "ferrumgw_proxy_requests_active",
        "Current number of active requests being processed"
    ).unwrap();

    // Request metrics by proxy
    static ref PROXY_REQUESTS_BY_PROXY: CounterVec = register_counter_vec!(
        "ferrumgw_proxy_requests_by_proxy",
        "Number of requests processed by each proxy",
        &["proxy_id", "proxy_name"]
    ).unwrap();

    // Status code metrics
    static ref PROXY_STATUS_CODES: CounterVec = register_counter_vec!(
        "ferrumgw_proxy_status_codes",
        "HTTP status codes returned by the gateway",
        &["status_code"]
    ).unwrap();

    // Latency metrics
    static ref PROXY_REQUEST_DURATION: HistogramVec = register_histogram_vec!(
        "ferrumgw_proxy_request_duration_seconds",
        "Request duration in seconds",
        &["proxy_id"],
        vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    ).unwrap();

    static ref PROXY_BACKEND_DURATION: HistogramVec = register_histogram_vec!(
        "ferrumgw_proxy_backend_duration_seconds", 
        "Backend request duration in seconds",
        &["proxy_id"],
        vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    ).unwrap();

    // Authentication metrics
    static ref AUTH_SUCCESSES: CounterVec = register_counter_vec!(
        "ferrumgw_auth_successes",
        "Number of successful authentications",
        &["plugin_name"]
    ).unwrap();

    static ref AUTH_FAILURES: CounterVec = register_counter_vec!(
        "ferrumgw_auth_failures",
        "Number of failed authentications",
        &["plugin_name"]
    ).unwrap();

    // Plugin metrics
    static ref PLUGIN_EXEC_DURATION: HistogramVec = register_histogram_vec!(
        "ferrumgw_plugin_exec_duration_seconds",
        "Plugin execution duration in seconds",
        &["plugin_name", "hook_name"],
        vec![0.0001, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5]
    ).unwrap();
}

impl MetricsCollector {
    pub fn new(config: Arc<RwLock<Configuration>>, mode: &str) -> Self {
        Self {
            config,
            mode: mode.to_string(),
            recent_rps: Arc::new(RwLock::new(0.0)),
            recent_status_codes: Arc::new(RwLock::new(HashMap::new())),
            last_operation_success: Arc::new(AtomicBool::new(true)),
        }
    }

    // Track a new request
    pub fn track_request_start(&self) {
        PROXY_REQUESTS_TOTAL.inc();
        PROXY_REQUESTS_ACTIVE.inc();
    }

    // Track the end of a request with its final status
    pub async fn track_request_end(&self, proxy_id: &str, status_code: u16, duration_ms: u64) {
        PROXY_REQUESTS_ACTIVE.dec();
        
        // Track by status code
        let status_code_str = status_code.to_string();
        PROXY_STATUS_CODES.with_label_values(&[&status_code_str]).inc();
        
        // Update recent status codes map for admin API metrics
        let mut status_codes = self.recent_status_codes.write().await;
        *status_codes.entry(status_code).or_insert(0) += 1;
        
        // Track by proxy
        let config = self.config.read().await;
        if let Some(proxy) = config.proxies.iter().find(|p| p.id == proxy_id) {
            let proxy_name = proxy.name.as_deref().unwrap_or("unnamed");
            PROXY_REQUESTS_BY_PROXY.with_label_values(&[proxy_id, proxy_name]).inc();
            
            // Track request duration
            let duration_seconds = duration_ms as f64 / 1000.0;
            PROXY_REQUEST_DURATION.with_label_values(&[proxy_id]).observe(duration_seconds);
        }
    }
    
    // Track backend request duration
    pub fn track_backend_duration(&self, proxy_id: &str, duration_ms: u64) {
        let duration_seconds = duration_ms as f64 / 1000.0;
        PROXY_BACKEND_DURATION.with_label_values(&[proxy_id]).observe(duration_seconds);
    }
    
    // Track authentication result
    pub fn track_auth_result(&self, plugin_name: &str, success: bool) {
        if success {
            AUTH_SUCCESSES.with_label_values(&[plugin_name]).inc();
        } else {
            AUTH_FAILURES.with_label_values(&[plugin_name]).inc();
        }
    }
    
    // Track plugin execution duration
    pub fn track_plugin_execution(&self, plugin_name: &str, hook_name: &str, duration_ns: u64) {
        let duration_seconds = duration_ns as f64 / 1_000_000_000.0;
        PLUGIN_EXEC_DURATION.with_label_values(&[plugin_name, hook_name]).observe(duration_seconds);
    }
    
    // Update RPS (requests per second) value based on recent metrics
    // This should be called periodically (e.g., every second) from a background task
    pub async fn update_rps(&self) {
        let one_second_ago = std::time::Instant::now() - Duration::from_secs(1);
        
        // Here we would calculate the actual RPS based on the requests in the last second
        // For this example, we'll use a simple rolling average
        let current_total = PROXY_REQUESTS_TOTAL.get();
        static mut LAST_TOTAL: u64 = 0;
        static mut LAST_TIME: std::time::Instant = std::time::Instant::now();
        
        let (rps, last_total, last_time) = unsafe {
            let elapsed = LAST_TIME.elapsed().as_secs_f64();
            let requests = current_total - LAST_TOTAL;
            let rps = if elapsed > 0.0 { requests as f64 / elapsed } else { 0.0 };
            
            // Update the last values
            LAST_TOTAL = current_total;
            LAST_TIME = std::time::Instant::now();
            
            (rps, LAST_TOTAL, LAST_TIME)
        };
        
        // Update the recent RPS value
        let mut recent_rps = self.recent_rps.write().await;
        *recent_rps = rps;
    }
    
    // Get the Prometheus metrics in text format
    pub fn get_metrics_text(&self) -> Result<String> {
        // Gather all registered metrics
        let metric_families = prometheus::gather();
        
        // Encode metrics as text
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        
        Ok(String::from_utf8(buffer)?)
    }
    
    // Get metrics for the admin API
    pub async fn get_admin_metrics(&self) -> serde_json::Value {
        let config = self.config.read().await;
        let rps = *self.recent_rps.read().await;
        let status_codes = self.recent_status_codes.read().await;
        
        // Convert status codes to the expected format
        let mut status_codes_json = serde_json::Map::new();
        for (&code, &count) in status_codes.iter() {
            status_codes_json.insert(code.to_string(), serde_json::Value::Number(count.into()));
        }
        
        serde_json::json!({
            "mode": self.mode,
            "config_last_updated_at": config.last_updated_at,
            "config_source_status": self.get_config_source_status(),
            "proxy_count": config.proxies.len(),
            "consumer_count": config.consumers.len(),
            "requests_per_second_current": rps,
            "status_codes_last_second": status_codes_json
        })
    }
    
    // Determine the status of the configuration source
    fn get_config_source_status(&self) -> &'static str {
        // Check the status of the configuration source based on mode
        match self.mode.as_str() {
            "file" => {
                // For file mode, check the last file read status
                let status = self.last_operation_success.load(Ordering::Relaxed);
                if status {
                    "online"
                } else {
                    "error"
                }
            },
            "database" => {
                // For database mode, check the last database operation status
                let status = self.last_operation_success.load(Ordering::Relaxed);
                if status {
                    "online"
                } else {
                    "degraded"
                }
            },
            "data_plane" => {
                // For data plane mode, check the gRPC connection status
                let status = self.last_operation_success.load(Ordering::Relaxed);
                if status {
                    "online"
                } else {
                    "connecting"
                }
            },
            "control_plane" => {
                // For control plane mode, always return online as it's the source of truth
                "online"
            },
            _ => "unknown"
        }
    }
    
    // Reset metrics for testing
    #[cfg(test)]
    pub fn reset(&self) {
        // This would reset all metrics for testing
    }
}

// Start a background task to periodically update RPS metrics
pub async fn start_metrics_updater(metrics: Arc<MetricsCollector>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        
        loop {
            interval.tick().await;
            metrics.update_rps().await;
        }
    });
}

// Create a new metrics endpoint handler for Prometheus metrics
// This can be used with hyper for the /metrics endpoint
pub async fn metrics_handler(_req: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>> {
    // Gather and encode metrics
    let metric_families = prometheus::gather();
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    
    // Create response
    let response = hyper::Response::builder()
        .status(200)
        .header("Content-Type", encoder.format_type())
        .body(hyper::Body::from(buffer))?;
    
    Ok(response)
}
