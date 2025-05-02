use anyhow::{Result, Context};
use async_trait::async_trait;
use hyper::{Body, Request, Response, header, StatusCode, client::HttpConnector};
use hyper_rustls::HttpsConnector;
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::sync::Arc;
use tokio::sync::RwLock;

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
    // Cache for JWKS keys
    jwks_cache: Arc<RwLock<HashMap<String, JwksCache>>>,
    // Cache for validated tokens
    token_cache: Arc<RwLock<HashMap<String, TokenCacheEntry>>>,
}

/// Cache entry for JWKS
#[derive(Debug, Clone)]
struct JwksCache {
    keys: HashMap<String, JwksKey>,
    fetched_at: SystemTime,
}

/// Cached JWKS key
#[derive(Debug, Clone)]
struct JwksKey {
    key_id: String,
    algorithm: Algorithm,
    decoding_key: DecodingKey,
}

/// Cache entry for validated tokens
#[derive(Debug, Clone)]
struct TokenCacheEntry {
    claims: HashMap<String, serde_json::Value>,
    expires_at: SystemTime,
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
            jwks_cache: Arc::new(RwLock::new(HashMap::new())),
            token_cache: Arc::new(RwLock::new(HashMap::new())),
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
        // Check token cache first if enabled
        if self.config.use_cache {
            let token_cache = self.token_cache.read().await;
            if let Some(cache_entry) = token_cache.get(token) {
                let now = SystemTime::now();
                if now < cache_entry.expires_at {
                    // Token is in cache and not expired
                    debug!("OAuth2 token validation cache hit");
                    return Ok(Some(cache_entry.claims.clone()));
                }
                // Token expired, will need to validate again
                debug!("OAuth2 token in cache but expired");
            }
        }
        
        // Token not in cache or cache disabled, validate according to mode
        let validation_result = match self.config.validation_mode {
            ValidationMode::Introspection => self.validate_token_introspection(token).await?,
            ValidationMode::Jwks => self.validate_token_jwks(token).await?,
        };
        
        // If validation successful and caching is enabled, store in cache
        if let Some(claims) = &validation_result {
            if self.config.use_cache {
                // Determine expiration time
                let expires_at = if let Some(exp) = claims.get("exp").and_then(|v| v.as_u64()) {
                    // Use token's expiration if available
                    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(exp))
                        .unwrap_or_else(|| SystemTime::now().checked_add(Duration::from_secs(self.config.cache_ttl_seconds))
                            .unwrap_or_else(|| SystemTime::now()))
                } else {
                    // Otherwise use configured TTL
                    SystemTime::now().checked_add(Duration::from_secs(self.config.cache_ttl_seconds))
                        .unwrap_or_else(|| SystemTime::now())
                };
                
                // Store in cache
                let mut token_cache = self.token_cache.write().await;
                token_cache.insert(token.to_string(), TokenCacheEntry {
                    claims: claims.clone(),
                    expires_at,
                });
                
                // Cleanup expired entries occasionally
                if token_cache.len() > 100 && rand::random::<f32>() < 0.1 {
                    self.cleanup_token_cache().await;
                }
            }
        }
        
        Ok(validation_result)
    }
    
    /// Clean up expired entries in the token cache
    async fn cleanup_token_cache(&self) {
        let now = SystemTime::now();
        let mut token_cache = self.token_cache.write().await;
        token_cache.retain(|_, entry| entry.expires_at > now);
        debug!("Cleaned up token cache, {} entries remaining", token_cache.len());
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
        if self.config.jwks_uri.is_none() {
            return Err(anyhow::anyhow!("JWKS URI is not configured"));
        }
        
        let jwks_uri = self.config.jwks_uri.as_ref().unwrap();
        
        // Parse the token header to extract the key ID (kid)
        let token_parts: Vec<&str> = token.split('.').collect();
        if token_parts.len() != 3 {
            return Err(anyhow::anyhow!("Invalid JWT format"));
        }
        
        // Decode the header part
        let header = token_parts[0];
        let padding = match header.len() % 4 {
            0 => "",
            1 => "===",
            2 => "==",
            3 => "=",
            _ => panic!("Impossible padding calculation"),
        };
        
        let decoded_header = base64::Engine::decode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            format!("{}{}", header, padding)
        )?;
        
        let header_json: serde_json::Value = serde_json::from_slice(&decoded_header)?;
        
        // Extract the key ID (kid) from the token header
        let kid = match header_json.get("kid").and_then(|v| v.as_str()) {
            Some(kid) => kid.to_string(),
            None => return Err(anyhow::anyhow!("JWT header does not contain a key ID (kid)")),
        };
        
        // Extract algorithm from header
        let alg = header_json.get("alg").and_then(|v| v.as_str()).unwrap_or("RS256");
        let algorithm = match alg {
            "RS256" => Algorithm::RS256,
            "RS384" => Algorithm::RS384,
            "RS512" => Algorithm::RS512,
            _ => return Err(anyhow::anyhow!("Unsupported JWT algorithm: {}", alg)),
        };
        
        // Try to find the key in the cache first
        let decoding_key = {
            let should_fetch_jwks = {
                let jwks_cache = self.jwks_cache.read().await;
                if let Some(cache) = jwks_cache.get(jwks_uri) {
                    // Check if cache is still valid (not older than 24 hours)
                    let cache_age = SystemTime::now()
                        .duration_since(cache.fetched_at)
                        .unwrap_or_default();
                    
                    if cache_age > Duration::from_secs(86400) {
                        debug!("JWKS cache is too old, refreshing");
                        true
                    } else if cache.keys.contains_key(&kid) {
                        // Key found in valid cache
                        let jwks_key = &cache.keys[&kid];
                        if jwks_key.algorithm == algorithm {
                            debug!("JWKS key found in cache: {}", kid);
                            return self.validate_token_with_key(token, &jwks_key.decoding_key, algorithm);
                        } else {
                            debug!("JWKS key algorithm mismatch, refreshing");
                            true
                        }
                    } else {
                        // Key not found in cache, might need to refresh
                        debug!("JWKS key not found in cache, fetching from server");
                        true
                    }
                } else {
                    // No cache for this JWKS URI
                    debug!("No JWKS cache for URI: {}", jwks_uri);
                    true
                }
            };
            
            if should_fetch_jwks {
                // Fetch JWKS and update cache
                debug!("Fetching JWKS from {}", jwks_uri);
                let keys = self.fetch_jwks(jwks_uri).await?;
                
                // Find the key we need
                if let Some(jwks_key) = keys.get(&kid) {
                    // Store the fetched JWKS in cache
                    let mut jwks_cache = self.jwks_cache.write().await;
                    jwks_cache.insert(jwks_uri.to_string(), JwksCache {
                        keys,
                        fetched_at: SystemTime::now(),
                    });
                    
                    // Return the key for validation
                    jwks_key.decoding_key.clone()
                } else {
                    return Err(anyhow::anyhow!("No matching key found in JWKS for kid: {}", kid));
                }
            } else {
                // Key found in cache but we need to extract it again due to borrowing rules
                let jwks_cache = self.jwks_cache.read().await;
                let cache = jwks_cache.get(jwks_uri).unwrap();
                let jwks_key = cache.keys.get(&kid).unwrap();
                jwks_key.decoding_key.clone()
            }
        };
        
        // Validate the token with the key
        self.validate_token_with_key(token, &decoding_key, algorithm)
    }
    
    /// Fetch JWKS from the server and parse keys
    async fn fetch_jwks(&self, jwks_uri: &str) -> Result<HashMap<String, JwksKey>> {
        // Make request to JWKS endpoint
        let req = Request::builder()
            .method("GET")
            .uri(jwks_uri)
            .header("Accept", "application/json")
            .body(Body::empty())?;
        
        let resp = self.http_client.request(req).await?;
        
        if resp.status() != StatusCode::OK {
            return Err(anyhow::anyhow!("Failed to fetch JWKS: HTTP {}", resp.status()));
        }
        
        let body_bytes = hyper::body::to_bytes(resp.into_body()).await?;
        let jwks: serde_json::Value = serde_json::from_slice(&body_bytes)?;
        
        // Parse keys
        let keys = match jwks.get("keys").and_then(|v| v.as_array()) {
            Some(keys) => keys,
            None => return Err(anyhow::anyhow!("Invalid JWKS format: missing 'keys' array")),
        };
        
        let mut result = HashMap::new();
        
        // Process each key in the JWKS
        for key_value in keys {
            let kid = match key_value.get("kid").and_then(|k| k.as_str()) {
                Some(kid) => kid.to_string(),
                None => continue, // Skip keys without kid
            };
            
            let alg = key_value.get("alg").and_then(|a| a.as_str()).unwrap_or("RS256");
            let algorithm = match alg {
                "RS256" => Algorithm::RS256,
                "RS384" => Algorithm::RS384,
                "RS512" => Algorithm::RS512,
                _ => continue, // Skip unsupported algorithms
            };
            
            // Extract key components based on key type
            if key_value.get("kty").and_then(|k| k.as_str()) == Some("RSA") {
                // RSA key
                let n = match key_value.get("n").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => continue, // Skip keys without modulus
                };
                
                let e = match key_value.get("e").and_then(|v| v.as_str()) {
                    Some(e) => e,
                    None => continue, // Skip keys without exponent
                };
                
                // Create decoding key
                match DecodingKey::from_rsa_components(n, e) {
                    Ok(decoding_key) => {
                        result.insert(kid.clone(), JwksKey {
                            key_id: kid,
                            algorithm,
                            decoding_key,
                        });
                    },
                    Err(e) => {
                        debug!("Failed to create decoding key from RSA components: {}", e);
                        continue;
                    }
                }
            }
            // Support for other key types can be added here
        }
        
        if result.is_empty() {
            return Err(anyhow::anyhow!("No valid keys found in JWKS"));
        }
        
        debug!("Parsed {} keys from JWKS", result.len());
        Ok(result)
    }
    
    /// Validate a token with a specific key
    fn validate_token_with_key(&self, token: &str, decoding_key: &DecodingKey, algorithm: Algorithm) -> Result<Option<HashMap<String, serde_json::Value>>> {
        // Set up validation parameters
        let mut validation = Validation::new(algorithm);
        
        // Set expected issuer if configured
        if let Some(issuer) = &self.config.issuer {
            validation.set_issuer(&[issuer.as_str()]);
        }
        
        // Set expected audience if configured
        if let Some(audience) = &self.config.audience {
            validation.set_audience(&[audience.as_str()]);
        }
        
        // Decode and validate the token
        let token_data = match decode::<serde_json::Value>(token, decoding_key, &validation) {
            Ok(token_data) => token_data,
            Err(e) => {
                debug!("Token validation failed: {}", e);
                return Ok(None);
            }
        };
        
        // Extract claims as a HashMap
        let mut result = HashMap::new();
        if let serde_json::Value::Object(obj) = token_data.claims {
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
