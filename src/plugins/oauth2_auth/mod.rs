use anyhow::{Result, Context};
use async_trait::async_trait;
use hyper::{Body, Request, Response, header, StatusCode, client::HttpConnector};
use hyper_rustls::HttpsConnector;
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::plugins::Plugin;
use crate::proxy::handler::{RequestContext, Consumer};

/// Configuration for the OAuth2 authentication plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2AuthConfig {
    /// Validation mode for the OAuth2 tokens
    #[serde(default)]
    pub validation_mode: ValidationMode,
    
    /// URL for token introspection (required if using introspection mode)
    pub introspection_url: Option<String>,
    
    /// Client ID for introspection requests
    pub introspection_client_id: Option<String>,
    
    /// Client secret for introspection requests
    pub introspection_client_secret: Option<String>,
    
    /// URL for JWKS (required if using JWKS mode)
    pub jwks_uri: Option<String>,
    
    /// Expected issuer for JWT validation
    pub issuer: Option<String>,
    
    /// Expected audience for JWT validation
    pub audience: Option<String>,
    
    /// Claim to use for identifying the consumer
    #[serde(default = "default_consumer_claim")]
    pub consumer_claim_field: String,
    
    /// Whether to cache token validation results
    #[serde(default = "default_true")]
    pub use_cache: bool,
    
    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_seconds: u64,
    
    /// OAuth provider name
    pub provider_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationMode {
    /// Validate tokens using OAuth2 token introspection
    Introspection,
    /// Validate tokens using JSON Web Key Set
    Jwks,
}

impl Default for ValidationMode {
    fn default() -> Self {
        ValidationMode::Introspection
    }
}

fn default_consumer_claim() -> String {
    "sub".to_string()
}

fn default_true() -> bool {
    true
}

fn default_cache_ttl() -> u64 {
    300
}

impl Default for OAuth2AuthConfig {
    fn default() -> Self {
        Self {
            validation_mode: ValidationMode::default(),
            introspection_url: None,
            introspection_client_id: None,
            introspection_client_secret: None,
            jwks_uri: None,
            issuer: None,
            audience: None,
            consumer_claim_field: default_consumer_claim(),
            use_cache: default_true(),
            cache_ttl_seconds: default_cache_ttl(),
            provider_name: "oauth2".to_string(),
        }
    }
}

/// OAuth2 authentication plugin
pub struct OAuth2AuthPlugin {
    config: OAuth2AuthConfig,
    http_client: hyper::Client<HttpsConnector<HttpConnector>>,
    // In a full implementation, this would include a JWKS client and caches
}

impl OAuth2AuthPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| OAuth2AuthConfig::default());
        
        // Validate configuration
        match config.validation_mode {
            ValidationMode::Introspection => {
                if config.introspection_url.is_none() {
                    return Err(anyhow::anyhow!(
                        "OAuth2 plugin configuration error: introspection_url is required for introspection mode"
                    ));
                }
            },
            ValidationMode::Jwks => {
                if config.jwks_uri.is_none() {
                    return Err(anyhow::anyhow!(
                        "OAuth2 plugin configuration error: jwks_uri is required for JWKS mode"
                    ));
                }
            },
        }
        
        // Create HTTP client for introspection and JWKS requests
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_only()
            .enable_http1()
            .build();
        
        let http_client = hyper::Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .build(https);
        
        Ok(Self {
            config,
            http_client,
        })
    }
    
    /// Extract the token from the request
    fn extract_token(&self, req: &Request<Body>) -> Option<String> {
        // Get the Authorization header
        let auth_header = req.headers().get(header::AUTHORIZATION)?;
        let auth_value = auth_header.to_str().ok()?;
        
        // Check if it's Bearer authentication
        if !auth_value.starts_with("Bearer ") {
            debug!("Authorization header does not use Bearer scheme");
            return None;
        }
        
        // Extract the token
        Some(auth_value[7..].trim().to_string())
    }
    
    /// Validate a token using the configured validation mode
    async fn validate_token(&self, token: &str) -> Result<Option<HashMap<String, serde_json::Value>>> {
        match self.config.validation_mode {
            ValidationMode::Introspection => self.validate_token_introspection(token).await,
            ValidationMode::Jwks => self.validate_token_jwks(token).await,
        }
    }
    
    /// Validate a token using OAuth2 token introspection
    async fn validate_token_introspection(&self, token: &str) -> Result<Option<HashMap<String, serde_json::Value>>> {
        let introspection_url = self.config.introspection_url.as_ref()
            .context("Introspection URL is required")?;
        
        // Create form params for introspection request
        let mut form = Vec::new();
        form.push(("token", token));
        form.push(("token_type_hint", "access_token"));
        
        // Add client credentials if provided
        if let Some(client_id) = &self.config.introspection_client_id {
            form.push(("client_id", client_id));
        }
        
        if let Some(client_secret) = &self.config.introspection_client_secret {
            form.push(("client_secret", client_secret));
        }
        
        // Create the request body
        let form_body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish();
        
        // Create the request
        let request = Request::builder()
            .method("POST")
            .uri(introspection_url)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::ACCEPT, "application/json")
            .body(Body::from(form_body))?;
        
        // Send the request
        let response = self.http_client.request(request).await?;
        
        // Check status code
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Introspection request failed with status: {}",
                response.status()
            ));
        }
        
        // Parse the response
        let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
        let introspection_response: serde_json::Value = serde_json::from_slice(&body_bytes)?;
        
        // Check if the token is active
        if introspection_response.get("active")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            // Token is active, extract claims
            let mut claims = HashMap::new();
            
            if let serde_json::Value::Object(obj) = introspection_response {
                for (key, value) in obj {
                    claims.insert(key, value);
                }
            }
            
            Ok(Some(claims))
        } else {
            // Token is not active
            debug!("OAuth2 token is not active");
            Ok(None)
        }
    }
    
    /// Validate a token using JWKS
    async fn validate_token_jwks(&self, token: &str) -> Result<Option<HashMap<String, serde_json::Value>>> {
        // NOTE: In a full implementation, this would:
        // 1. Parse the token header to get the key ID (kid)
        // 2. Fetch the JWKS from the jwks_uri if not cached
        // 3. Find the key with the matching kid
        // 4. Validate the token signature using the key
        // 5. Validate the token claims (exp, iat, iss, aud)
        // 6. Return the claims if valid
        
        // For this simplified implementation, we'll simulate token validation
        debug!("JWKS validation would occur here");
        
        // Parse the token without validation to extract claims
        let token_parts: Vec<&str> = token.split('.').collect();
        if token_parts.len() != 3 {
            return Err(anyhow::anyhow!("Invalid JWT format"));
        }
        
        // Decode the payload part (without validation)
        let payload = token_parts[1];
        let padding = match payload.len() % 4 {
            0 => "",
            1 => "===",
            2 => "==",
            3 => "=",
            _ => panic!("Impossible padding calculation"),
        };
        
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            format!("{}{}", payload, padding)
        )?;
        
        let claims: serde_json::Value = serde_json::from_slice(&decoded)?;
        
        // Check expiration
        if let Some(exp) = claims.get("exp").and_then(|v| v.as_u64()) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            
            if exp < now {
                debug!("OAuth2 token has expired");
                return Ok(None);
            }
        }
        
        // Check issuer if configured
        if let Some(expected_issuer) = &self.config.issuer {
            if claims.get("iss")
                .and_then(|v| v.as_str())
                .map(|s| s != expected_issuer)
                .unwrap_or(true)
            {
                debug!("OAuth2 token has incorrect issuer");
                return Ok(None);
            }
        }
        
        // Check audience if configured
        if let Some(expected_audience) = &self.config.audience {
            let audience_matched = match claims.get("aud") {
                Some(serde_json::Value::String(s)) => s == expected_audience,
                Some(serde_json::Value::Array(a)) => a.iter()
                    .any(|v| v.as_str().map(|s| s == expected_audience).unwrap_or(false)),
                _ => false,
            };
            
            if !audience_matched {
                debug!("OAuth2 token has incorrect audience");
                return Ok(None);
            }
        }
        
        // Extract claims as a HashMap
        let mut result = HashMap::new();
        if let serde_json::Value::Object(obj) = claims {
            for (key, value) in obj {
                result.insert(key, value);
            }
        }
        
        Ok(Some(result))
    }
    
    /// Find or create a consumer based on token claims
    async fn find_consumer_from_claims(&self, claims: &HashMap<String, serde_json::Value>) -> Option<Consumer> {
        // Extract consumer identifier from claims
        let consumer_id = claims.get(&self.config.consumer_claim_field)
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })?;
        
        // Look up the consumer in the shared configuration
        let consumer_id_str = consumer_id.to_string();
        
        // Find the consumer in the proxy's active configuration
        if let Some(consumers) = ctx.proxy.active_config.as_ref().map(|cfg| &cfg.consumers) {
            // Try to find the consumer by ID first
            if let Some(consumer) = consumers.iter().find(|c| c.id == consumer_id_str) {
                return Some(consumer.clone());
            }
            
            // If not found by ID, try by custom_id
            if let Some(consumer) = consumers.iter().find(|c| c.custom_id.as_ref().map_or(false, |cid| cid == &consumer_id_str)) {
                return Some(consumer.clone());
            }
            
            // If not found by custom_id, try by username
            if let Some(consumer) = consumers.iter().find(|c| c.username == consumer_id_str) {
                return Some(consumer.clone());
            }
            
            // Special case for OAuth - look for OAuth provider associations
            if let Some(consumer) = consumers.iter().find(|c| {
                if let Some(credentials) = &c.credentials {
                    if let Some(oauth_ids) = credentials.get("oauth_provider_ids").and_then(|v| v.as_object()) {
                        if let Some(provider_id) = oauth_ids.get(&self.config.provider_name) {
                            return provider_id.as_str().map_or(false, |id| id == consumer_id_str);
                        }
                    }
                }
                false
            }) {
                return Some(consumer.clone());
            }
        }
        
        debug!("Consumer with OAuth ID '{}' not found in active configuration", consumer_id);
        None
    }
}

#[async_trait]
impl Plugin for OAuth2AuthPlugin {
    fn name(&self) -> &'static str {
        "oauth2_auth"
    }
    
    async fn authenticate(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        // Skip if a consumer is already identified (multi-auth mode)
        if ctx.consumer.is_some() {
            debug!("Consumer already identified, skipping OAuth2 authentication");
            return Ok(true);
        }
        
        // Extract the token from the request
        let token = match self.extract_token(req) {
            Some(token) => token,
            None => {
                debug!("No OAuth2 token found in request");
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Validate the token
        let claims = match self.validate_token(&token).await {
            Ok(Some(claims)) => claims,
            Ok(None) => {
                debug!("OAuth2 token validation failed");
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            },
            Err(e) => {
                warn!("OAuth2 token validation error: {}", e);
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Find the consumer based on the token claims
        let consumer = match self.find_consumer_from_claims(&claims).await {
            Some(consumer) => consumer,
            None => {
                warn!("No consumer found for OAuth2 token");
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Set the consumer in the context
        ctx.consumer = Some(consumer);
        debug!("Consumer identified by OAuth2 token: {}", ctx.consumer.as_ref().unwrap().username);
        
        Ok(true)
    }
}
