use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, StatusCode};
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use std::collections::HashSet;

use crate::plugins::Plugin;
use crate::proxy::handler::RequestContext;

/// Configuration for the access control plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessControlConfig {
    /// List of allowed consumer usernames (empty means all are allowed unless in disallowed list)
    #[serde(default)]
    pub allowed_consumers: Vec<String>,
    
    /// List of disallowed consumer usernames
    #[serde(default)]
    pub disallowed_consumers: Vec<String>,
    
    /// Whether anonymous access is allowed (no consumer identified)
    #[serde(default = "default_false")]
    pub allow_anonymous: bool,
}

fn default_false() -> bool {
    false
}

impl Default for AccessControlConfig {
    fn default() -> Self {
        Self {
            allowed_consumers: Vec::new(),
            disallowed_consumers: Vec::new(),
            allow_anonymous: false,
        }
    }
}

/// Access control plugin for authorizing requests based on consumer identity
pub struct AccessControlPlugin {
    config: AccessControlConfig,
    allowed_set: HashSet<String>,
    disallowed_set: HashSet<String>,
}

impl AccessControlPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| AccessControlConfig::default());
        
        // Create HashSets for efficient lookups
        let allowed_set = config.allowed_consumers.iter().cloned().collect();
        let disallowed_set = config.disallowed_consumers.iter().cloned().collect();
        
        Ok(Self {
            config,
            allowed_set,
            disallowed_set,
        })
    }
}

#[async_trait]
impl Plugin for AccessControlPlugin {
    fn name(&self) -> &'static str {
        "access_control"
    }
    
    async fn authorize(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        // Check if a consumer has been identified
        if let Some(ref consumer) = ctx.consumer {
            // Check if the consumer is explicitly disallowed
            if self.disallowed_set.contains(&consumer.username) {
                info!(
                    "Access denied for consumer '{}' - explicitly disallowed",
                    consumer.username
                );
                return Ok(false);
            }
            
            // If there's an allowed list, check if the consumer is in it
            if !self.allowed_set.is_empty() && !self.allowed_set.contains(&consumer.username) {
                info!(
                    "Access denied for consumer '{}' - not in allowed list",
                    consumer.username
                );
                return Ok(false);
            }
            
            // Consumer is authorized
            debug!("Access granted for consumer '{}'", consumer.username);
            return Ok(true);
        } else {
            // No consumer identified - check if anonymous access is allowed
            if self.config.allow_anonymous {
                debug!("Anonymous access granted");
                return Ok(true);
            } else {
                // For multi-auth mode, this plugin acts as the final gatekeeper
                // after all authentication plugins have run
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    info!("Access denied - no consumer identified in multi-auth mode");
                } else {
                    debug!("Access denied - no consumer identified and anonymous access not allowed");
                }
                
                return Ok(false);
            }
        }
    }
}
