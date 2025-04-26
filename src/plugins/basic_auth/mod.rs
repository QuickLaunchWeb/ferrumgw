use anyhow::Result;
use async_trait::async_trait;
use hyper::{Body, Request, Response, header, StatusCode};
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bcrypt::verify;

use crate::plugins::Plugin;
use crate::proxy::handler::{RequestContext, Consumer};

/// Configuration for the Basic Authentication plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicAuthConfig {
    /// Realm to use in WWW-Authenticate header
    #[serde(default = "default_realm")]
    pub realm: String,
    
    /// Whether to cache consumer lookups
    #[serde(default = "default_true")]
    pub use_cache: bool,
    
    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_seconds: u64,
}

fn default_realm() -> String {
    "API Gateway".to_string()
}

fn default_true() -> bool {
    true
}

fn default_cache_ttl() -> u64 {
    300
}

impl Default for BasicAuthConfig {
    fn default() -> Self {
        Self {
            realm: default_realm(),
            use_cache: default_true(),
            cache_ttl_seconds: default_cache_ttl(),
        }
    }
}

/// HTTP Basic Authentication plugin
pub struct BasicAuthPlugin {
    config: BasicAuthConfig,
}

impl BasicAuthPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| BasicAuthConfig::default());
        
        Ok(Self { config })
    }
    
    /// Extract username and password from the Authorization header
    fn extract_credentials(&self, req: &Request<Body>) -> Option<(String, String)> {
        // Get the Authorization header
        let auth_header = req.headers().get(header::AUTHORIZATION)?;
        let auth_value = auth_header.to_str().ok()?;
        
        // Check if it's Basic authentication
        if !auth_value.starts_with("Basic ") {
            debug!("Authorization header does not use Basic scheme");
            return None;
        }
        
        // Decode the Base64 value
        let credentials = auth_value[6..].trim(); // Skip "Basic "
        let decoded = BASE64.decode(credentials).ok()?;
        let credentials_str = String::from_utf8(decoded).ok()?;
        
        // Split username and password
        let mut parts = credentials_str.splitn(2, ':');
        match (parts.next(), parts.next()) {
            (Some(username), Some(password)) if !username.is_empty() => {
                Some((username.to_string(), password.to_string()))
            },
            _ => {
                debug!("Invalid Basic authentication format");
                None
            }
        }
    }
    
    /// Authenticate a user based on username and password
    async fn authenticate_user(&self, username: &str, password: &str, ctx: &RequestContext) -> Option<Consumer> {
        // Look up the user in the shared configuration
        if let Some(active_config) = &ctx.proxy.active_config {
            // Try to find the consumer by username
            if let Some(consumer) = active_config.consumers.iter().find(|c| c.username == username) {
                // Check if the consumer has password credentials
                if let Some(credentials) = &consumer.credentials {
                    // Look for password in credentials
                    if let Some(stored_password) = credentials.get("password").and_then(|p| p.as_str()) {
                        // Verify the password
                        if verify_password(password, stored_password) {
                            debug!("Authentication successful for user: {}", username);
                            return Some(consumer.clone());
                        }
                    }
                    
                    // Look for hashed password in credentials
                    if let Some(hashed_password) = credentials.get("hashed_password").and_then(|p| p.as_str()) {
                        // Verify the password against the hash
                        if verify_password_hash(password, hashed_password) {
                            debug!("Authentication successful for user: {} (using hashed password)", username);
                            return Some(consumer.clone());
                        }
                    }
                }
            }
        }
        
        debug!("Authentication failed for user: {}", username);
        None
    }
}

#[async_trait]
impl Plugin for BasicAuthPlugin {
    fn name(&self) -> &'static str {
        "basic_auth"
    }
    
    async fn authenticate(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        // Skip if a consumer is already identified (multi-auth mode)
        if ctx.consumer.is_some() {
            debug!("Consumer already identified, skipping Basic authentication");
            return Ok(true);
        }
        
        // Extract credentials from the request
        let (username, password) = match self.extract_credentials(req) {
            Some(creds) => creds,
            None => {
                debug!("No Basic authentication credentials found in request");
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Authenticate the user
        let consumer = match self.authenticate_user(&username, &password, ctx).await {
            Some(consumer) => consumer,
            None => {
                warn!("Basic authentication failed for user '{}'", username);
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Set the consumer in the context
        ctx.consumer = Some(consumer);
        debug!("Consumer identified by Basic authentication: {}", username);
        
        Ok(true)
    }
}

// Verify a plaintext password against another plaintext password
// This is only for development/testing and should be removed in production
fn verify_password(provided: &str, stored: &str) -> bool {
    provided == stored
}

// Verify a password against a hash
fn verify_password_hash(password: &str, hash: &str) -> bool {
    #[cfg(feature = "bcrypt")]
    {
        bcrypt::verify(password, hash).unwrap_or(false)
    }
    
    #[cfg(all(feature = "argon2", not(feature = "bcrypt")))]
    {
        use argon2::{Argon2, PasswordHash, PasswordVerifier};
        
        match PasswordHash::new(hash) {
            Ok(parsed_hash) => {
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed_hash)
                    .is_ok()
            }
            Err(_) => false,
        }
    }
    
    #[cfg(not(any(feature = "bcrypt", feature = "argon2")))]
    {
        // Fallback implementation when neither bcrypt nor argon2 features are enabled
        // This is insecure and should only be used for development/testing
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash_slice(password.as_bytes(), &mut hasher);
        let simple_hash = format!("{:x}", std::hash::Hasher::finish(&hasher));
        
        // Only use this for testing!
        hash == simple_hash
    }
}
