use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, StatusCode, header};
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use std::time::{Duration, Instant};
use dashmap::DashMap;

use crate::plugins::Plugin;
use crate::proxy::handler::RequestContext;

/// Configuration for the rate limiting plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    /// Whether to limit by consumer or IP address
    #[serde(default)]
    pub limit_by: LimitBy,
    
    /// Maximum requests per second (0 means no limit)
    #[serde(default)]
    pub requests_per_second: u32,
    
    /// Maximum requests per minute (0 means no limit)
    #[serde(default)]
    pub requests_per_minute: u32,
    
    /// Maximum requests per hour (0 means no limit)
    #[serde(default)]
    pub requests_per_hour: u32,
    
    /// Whether to add X-RateLimit headers to responses
    #[serde(default = "default_true")]
    pub add_headers: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LimitBy {
    /// Limit by consumer identity
    Consumer,
    /// Limit by client IP address
    Ip,
}

impl Default for LimitBy {
    fn default() -> Self {
        LimitBy::Consumer
    }
}

fn default_true() -> bool {
    true
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            limit_by: LimitBy::default(),
            requests_per_second: 0,
            requests_per_minute: 0,
            requests_per_hour: 0,
            add_headers: true,
        }
    }
}

/// Tracks request counters for a specific time window
#[derive(Debug)]
struct WindowCounter {
    /// The time window start
    window_start: Instant,
    /// The counter value
    count: u32,
}

impl WindowCounter {
    fn new() -> Self {
        Self {
            window_start: Instant::now(),
            count: 0,
        }
    }
    
    /// Checks if the window has expired
    fn is_expired(&self, window_duration: Duration) -> bool {
        self.window_start.elapsed() >= window_duration
    }
    
    /// Increments the counter
    fn increment(&mut self) {
        self.count += 1;
    }
    
    /// Resets the counter and window start time
    fn reset(&mut self) {
        self.window_start = Instant::now();
        self.count = 0;
    }
}

/// Stores rate limiting state for all consumers/IPs
#[derive(Debug)]
struct RateLimitState {
    /// Second window counters
    second_counters: DashMap<String, WindowCounter>,
    /// Minute window counters
    minute_counters: DashMap<String, WindowCounter>,
    /// Hour window counters
    hour_counters: DashMap<String, WindowCounter>,
}

impl RateLimitState {
    fn new() -> Self {
        Self {
            second_counters: DashMap::new(),
            minute_counters: DashMap::new(),
            hour_counters: DashMap::new(),
        }
    }
    
    /// Check if a key has exceeded any of the rate limits
    fn check_limits(
        &self,
        key: &str,
        requests_per_second: u32,
        requests_per_minute: u32,
        requests_per_hour: u32,
    ) -> RateLimitCheckResult {
        let mut result = RateLimitCheckResult {
            limit_exceeded: false,
            window_unit: None,
            remaining: HashMap::new(),
        };
        
        // Check second limit
        if requests_per_second > 0 {
            if let Some(counter) = self.second_counters.get(key) {
                let remaining = if counter.is_expired(Duration::from_secs(1)) {
                    requests_per_second
                } else {
                    requests_per_second.saturating_sub(counter.count)
                };
                
                result.remaining.insert("second".to_string(), remaining);
                
                if remaining == 0 {
                    result.limit_exceeded = true;
                    result.window_unit = Some("second".to_string());
                }
            } else {
                result.remaining.insert("second".to_string(), requests_per_second);
            }
        }
        
        // Check minute limit
        if requests_per_minute > 0 {
            if let Some(counter) = self.minute_counters.get(key) {
                let remaining = if counter.is_expired(Duration::from_secs(60)) {
                    requests_per_minute
                } else {
                    requests_per_minute.saturating_sub(counter.count)
                };
                
                result.remaining.insert("minute".to_string(), remaining);
                
                if remaining == 0 && !result.limit_exceeded {
                    result.limit_exceeded = true;
                    result.window_unit = Some("minute".to_string());
                }
            } else {
                result.remaining.insert("minute".to_string(), requests_per_minute);
            }
        }
        
        // Check hour limit
        if requests_per_hour > 0 {
            if let Some(counter) = self.hour_counters.get(key) {
                let remaining = if counter.is_expired(Duration::from_secs(3600)) {
                    requests_per_hour
                } else {
                    requests_per_hour.saturating_sub(counter.count)
                };
                
                result.remaining.insert("hour".to_string(), remaining);
                
                if remaining == 0 && !result.limit_exceeded {
                    result.limit_exceeded = true;
                    result.window_unit = Some("hour".to_string());
                }
            } else {
                result.remaining.insert("hour".to_string(), requests_per_hour);
            }
        }
        
        result
    }
    
    /// Record a request for a key
    fn record_request(&self, key: &str, requests_per_second: u32, requests_per_minute: u32, requests_per_hour: u32) {
        // Update second counter
        if requests_per_second > 0 {
            let mut counter = self.second_counters.entry(key.to_string()).or_insert_with(WindowCounter::new);
            if counter.is_expired(Duration::from_secs(1)) {
                counter.reset();
            }
            counter.increment();
        }
        
        // Update minute counter
        if requests_per_minute > 0 {
            let mut counter = self.minute_counters.entry(key.to_string()).or_insert_with(WindowCounter::new);
            if counter.is_expired(Duration::from_secs(60)) {
                counter.reset();
            }
            counter.increment();
        }
        
        // Update hour counter
        if requests_per_hour > 0 {
            let mut counter = self.hour_counters.entry(key.to_string()).or_insert_with(WindowCounter::new);
            if counter.is_expired(Duration::from_secs(3600)) {
                counter.reset();
            }
            counter.increment();
        }
    }
}

/// The result of a rate limit check
#[derive(Debug)]
struct RateLimitCheckResult {
    /// Whether any limit was exceeded
    limit_exceeded: bool,
    /// The time window unit that was exceeded (second, minute, hour)
    window_unit: Option<String>,
    /// Remaining requests for each window
    remaining: HashMap<String, u32>,
}

/// Rate limiting plugin
pub struct RateLimitingPlugin {
    config: RateLimitingConfig,
    state: Arc<RateLimitState>,
}

impl RateLimitingPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| RateLimitingConfig::default());
        
        // Validate that at least one limit is set
        if config.requests_per_second == 0 && config.requests_per_minute == 0 && config.requests_per_hour == 0 {
            warn!("Rate limiting plugin configured without any actual limits");
        }
        
        Ok(Self {
            config,
            state: Arc::new(RateLimitState::new()),
        })
    }
    
    /// Determine the rate limit key based on configuration
    fn get_rate_limit_key(&self, ctx: &RequestContext) -> String {
        match self.config.limit_by {
            LimitBy::Consumer => {
                if let Some(ref consumer) = ctx.consumer {
                    format!("consumer:{}", consumer.username)
                } else {
                    format!("ip:{}", ctx.client_addr)
                }
            },
            LimitBy::Ip => format!("ip:{}", ctx.client_addr),
        }
    }
    
    /// Add rate limit headers to the response
    fn add_rate_limit_headers(&self, resp: &mut Response<Body>, check_result: &RateLimitCheckResult) {
        if !self.config.add_headers {
            return;
        }
        
        // Add X-RateLimit-Limit headers
        if self.config.requests_per_second > 0 {
            resp.headers_mut().insert(
                header::HeaderName::from_static("x-ratelimit-limit-second"),
                header::HeaderValue::from_str(&self.config.requests_per_second.to_string()).unwrap()
            );
        }
        
        if self.config.requests_per_minute > 0 {
            resp.headers_mut().insert(
                header::HeaderName::from_static("x-ratelimit-limit-minute"),
                header::HeaderValue::from_str(&self.config.requests_per_minute.to_string()).unwrap()
            );
        }
        
        if self.config.requests_per_hour > 0 {
            resp.headers_mut().insert(
                header::HeaderName::from_static("x-ratelimit-limit-hour"),
                header::HeaderValue::from_str(&self.config.requests_per_hour.to_string()).unwrap()
            );
        }
        
        // Add X-RateLimit-Remaining headers
        for (window, remaining) in &check_result.remaining {
            let header_name = format!("x-ratelimit-remaining-{}", window);
            resp.headers_mut().insert(
                header::HeaderName::from_bytes(header_name.as_bytes()).unwrap(),
                header::HeaderValue::from_str(&remaining.to_string()).unwrap()
            );
        }
        
        // Add Retry-After header if rate limit was exceeded
        if check_result.limit_exceeded {
            if let Some(ref window_unit) = check_result.window_unit {
                let retry_after = match window_unit.as_str() {
                    "second" => "1",
                    "minute" => "60",
                    "hour" => "3600",
                    _ => "60",
                };
                
                resp.headers_mut().insert(
                    header::RETRY_AFTER,
                    header::HeaderValue::from_str(retry_after).unwrap()
                );
            }
        }
    }
}

#[async_trait]
impl Plugin for RateLimitingPlugin {
    fn name(&self) -> &'static str {
        "rate_limiting"
    }
    
    async fn authenticate(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        // If limiting by consumer, we need to ensure the consumer is identified
        if self.config.limit_by == LimitBy::Consumer && ctx.consumer.is_none() {
            debug!("Rate limiting by consumer, but no consumer identified");
            return Ok(true); // Allow the request to continue to other authentication plugins
        }
        
        // Get the rate limit key
        let key = self.get_rate_limit_key(ctx);
        
        // Check if rate limit is exceeded
        let check_result = self.state.check_limits(
            &key,
            self.config.requests_per_second,
            self.config.requests_per_minute,
            self.config.requests_per_hour,
        );
        
        if check_result.limit_exceeded {
            // Rate limit exceeded
            info!(
                "Rate limit exceeded for {}: {} limit",
                key,
                check_result.window_unit.as_deref().unwrap_or("unknown")
            );
            
            // Create a 429 response
            let mut response = Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .body(Body::from("Rate limit exceeded"))
                .unwrap();
            
            // Add rate limit headers
            self.add_rate_limit_headers(&mut response, &check_result);
            
            // Store the response in the request extensions for later retrieval
            req.extensions_mut().insert(response);
            
            return Ok(false); // Do not continue processing
        }
        
        // Record the request
        self.state.record_request(
            &key,
            self.config.requests_per_second,
            self.config.requests_per_minute,
            self.config.requests_per_hour,
        );
        
        Ok(true)
    }
}
