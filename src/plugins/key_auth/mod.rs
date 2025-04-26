use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, header, StatusCode};
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use bcrypt::verify;

use crate::plugins::Plugin;
use crate::proxy::handler::{RequestContext, Consumer};

/// Configuration for the API key authentication plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyAuthConfig {
    /// Where to look for the API key
    #[serde(default = "default_key_location")]
    pub key_location: KeyLocation,
    
    /// Custom header name if using a header
    #[serde(default = "default_header_name")]
    pub header_name: String,
    
    /// Query parameter name if using a query parameter
    #[serde(default = "default_query_name")]
    pub query_name: String,
    
    /// Whether to cache consumer lookups
    #[serde(default = "default_true")]
    pub use_cache: bool,
    
    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_seconds: u64,
    
    /// Whether to hash keys
    #[serde(default = "default_hash_keys")]
    pub hash_keys: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum KeyLocation {
    /// Look for key in a header
    Header,
    /// Look for key in a query parameter
    Query,
}

fn default_key_location() -> KeyLocation {
    KeyLocation::Header
}

fn default_header_name() -> String {
    "X-API-Key".to_string()
}

fn default_query_name() -> String {
    "apikey".to_string()
}

fn default_true() -> bool {
    true
}

fn default_cache_ttl() -> u64 {
    300
}

fn default_hash_keys() -> bool {
    false
}

impl Default for KeyAuthConfig {
    fn default() -> Self {
        Self {
            key_location: default_key_location(),
            header_name: default_header_name(),
            query_name: default_query_name(),
            use_cache: default_true(),
            cache_ttl_seconds: default_cache_ttl(),
            hash_keys: default_hash_keys(),
        }
    }
}

/// API key authentication plugin
pub struct KeyAuthPlugin {
    config: KeyAuthConfig,
}

impl KeyAuthPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| KeyAuthConfig::default());
        
        Ok(Self { config })
    }
    
    /// Extract the API key from the request based on the configuration
    fn extract_key(&self, req: &Request<Body>) -> Option<String> {
        match self.config.key_location {
            KeyLocation::Header => {
                // Extract from header
                let header_value = req.headers().get(&self.config.header_name)?;
                header_value.to_str().ok().map(|s| s.to_string())
            },
            KeyLocation::Query => {
                // Extract from query parameter
                let uri = req.uri();
                let query = uri.query()?;
                
                // Find the API key parameter
                for pair in query.split('&') {
                    let mut parts = pair.splitn(2, '=');
                    if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                        if name == self.config.query_name {
                            return Some(value.to_string());
                        }
                    }
                }
                
                debug!("No API key query parameter found");
                None
            }
        }
    }
    
    /// Find a consumer based on the API key
    async fn find_consumer_by_key(&self, api_key: &str, ctx: &RequestContext) -> Option<Consumer> {
        // Look up the API key in the shared configuration
        if let Some(active_config) = &ctx.proxy.active_config {
            // Iterate through all consumers to find one with matching API key
            for consumer in &active_config.consumers {
                // Check if the consumer has an API key credential
                if let Some(credentials) = &consumer.credentials {
                    // Check for API key entries
                    if let Some(keys) = credentials.get("api_keys").and_then(|v| v.as_array()) {
                        // Look through all keys
                        for key in keys {
                            if let Some(key_str) = key.as_str() {
                                // If using hashed keys, we would verify the hash here instead of direct comparison
                                if key_str == api_key || (self.config.hash_keys && verify_key_hash(api_key, key_str)) {
                                    debug!("Found consumer {} using API key authentication", consumer.username);
                                    return Some(consumer.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        debug!("No consumer found with the provided API key");
        None
    }
}

#[async_trait]
impl Plugin for KeyAuthPlugin {
    fn name(&self) -> &'static str {
        "key_auth"
    }
    
    async fn authenticate(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        // Skip if a consumer is already identified (multi-auth mode)
        if ctx.consumer.is_some() {
            debug!("Consumer already identified, skipping API key authentication");
            return Ok(true);
        }
        
        // Extract the API key from the request
        let api_key = match self.extract_key(req) {
            Some(key) => key,
            None => {
                debug!("No API key found in request");
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Find the consumer based on the API key
        let consumer = match self.find_consumer_by_key(&api_key, ctx).await {
            Some(consumer) => consumer,
            None => {
                warn!("No consumer found for API key");
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Set the consumer in the context
        ctx.consumer = Some(consumer);
        debug!("Consumer identified by API key: {}", ctx.consumer.as_ref().unwrap().username);
        
        Ok(true)
    }
}

/// Verify a key against a hash (for implementations that hash API keys)
fn verify_key_hash(plain_key: &str, hash: &str) -> bool {
    // In a production system, we would use bcrypt, argon2, or similar
    // Here's a simplified example using a bcrypt-like approach:
    
    #[cfg(feature = "argon2")]
    {
        use argon2::{Argon2, PasswordHash, PasswordVerifier};
        
        match PasswordHash::new(hash) {
            Ok(parsed_hash) => {
                Argon2::default()
                    .verify_password(plain_key.as_bytes(), &parsed_hash)
                    .is_ok()
            }
            Err(_) => false,
        }
    }
    
    #[cfg(not(feature = "argon2"))]
    {
        // Fallback for when argon2 feature is not enabled
        // This would be a simplified and insecure comparison 
        // (should only be used for development/testing)
        hash.starts_with(&format!("hash_{}", plain_key))
    }
}
