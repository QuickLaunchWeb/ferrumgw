use anyhow::{Result, Context};
use async_trait::async_trait;
use hyper::{Body, Request, Response, header, StatusCode};
use serde::{Serialize, Deserialize};
use tracing::{debug, warn, info};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm, TokenData};

use crate::plugins::Plugin;
use crate::proxy::handler::{RequestContext, Consumer};

/// Configuration for the JWT authentication plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtAuthConfig {
    /// Where to find the JWT token (header name or query parameter)
    #[serde(default = "default_token_lookup")]
    pub token_lookup: TokenLookup,
    
    /// Field in the JWT claims that identifies the consumer
    #[serde(default = "default_consumer_claim")]
    pub consumer_claim_field: String,
    
    /// Algorithm to use for validating JWT (default: HS256)
    #[serde(default)]
    pub algorithm: JwtAlgorithm,
    
    /// Secret key for HS256/HS384/HS512 (required if using HMAC algorithms)
    pub secret: Option<String>,
    
    /// Public key for RS256/RS384/RS512/ES256/ES384/ES512 (required if using RSA/ECDSA algorithms)
    pub public_key: Option<String>,
    
    /// Whether to allow tokens that don't have an expiration claim
    #[serde(default = "default_false")]
    pub allow_tokens_without_exp: bool,
    
    /// Optional issuer to validate in the token
    pub issuer: Option<String>,
    
    /// Optional audience to validate in the token
    pub audience: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TokenLookup {
    /// Look for token in the Authorization header (Bearer scheme)
    Header,
    /// Look for token in a query parameter
    Query,
    /// Look for token in a cookie
    Cookie,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "uppercase")]
pub enum JwtAlgorithm {
    /// HMAC using SHA-256
    HS256,
    /// HMAC using SHA-384
    HS384,
    /// HMAC using SHA-512
    HS512,
    /// RSA using SHA-256
    RS256,
    /// RSA using SHA-384
    RS384,
    /// RSA using SHA-512
    RS512,
    /// ECDSA using SHA-256
    ES256,
    /// ECDSA using SHA-384
    ES384,
    /// ECDSA using SHA-512
    ES512,
}

impl Default for JwtAlgorithm {
    fn default() -> Self {
        JwtAlgorithm::HS256
    }
}

fn default_token_lookup() -> TokenLookup {
    TokenLookup::Header
}

fn default_consumer_claim_field() -> String {
    "sub".to_string()
}

fn default_false() -> bool {
    false
}

impl Default for JwtAuthConfig {
    fn default() -> Self {
        Self {
            token_lookup: default_token_lookup(),
            consumer_claim_field: default_consumer_claim_field(),
            algorithm: JwtAlgorithm::default(),
            secret: None,
            public_key: None,
            allow_tokens_without_exp: default_false(),
            issuer: None,
            audience: None,
        }
    }
}

/// JWT authentication plugin
pub struct JwtAuthPlugin {
    config: JwtAuthConfig,
}

impl JwtAuthPlugin {
    pub fn new(config_json: serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json)
            .unwrap_or_else(|_| JwtAuthConfig::default());
        
        // Validate configuration
        match config.algorithm {
            JwtAlgorithm::HS256 | JwtAlgorithm::HS384 | JwtAlgorithm::HS512 => {
                if config.secret.is_none() {
                    return Err(anyhow::anyhow!(
                        "JWT plugin configuration error: HMAC algorithms require a secret key"
                    ));
                }
            },
            JwtAlgorithm::RS256 | JwtAlgorithm::RS384 | JwtAlgorithm::RS512 |
            JwtAlgorithm::ES256 | JwtAlgorithm::ES384 | JwtAlgorithm::ES512 => {
                if config.public_key.is_none() {
                    return Err(anyhow::anyhow!(
                        "JWT plugin configuration error: RSA/ECDSA algorithms require a public key"
                    ));
                }
            },
        }
        
        Ok(Self { config })
    }
    
    /// Extract the token from the request based on the configuration
    fn extract_token(&self, req: &Request<Body>) -> Option<String> {
        match self.config.token_lookup {
            TokenLookup::Header => {
                // Extract from Authorization header
                let auth_header = req.headers().get(header::AUTHORIZATION)?;
                let auth_value = auth_header.to_str().ok()?;
                
                if auth_value.starts_with("Bearer ") {
                    Some(auth_value[7..].to_string())
                } else {
                    debug!("Authorization header does not use Bearer scheme");
                    None
                }
            },
            TokenLookup::Query => {
                // Extract from query parameter
                let uri = req.uri();
                let query = uri.query()?;
                
                // Find the access_token parameter
                for pair in query.split('&') {
                    let mut parts = pair.splitn(2, '=');
                    if let (Some("access_token"), Some(token)) = (parts.next(), parts.next()) {
                        return Some(token.to_string());
                    }
                }
                
                debug!("No access_token query parameter found");
                None
            },
            TokenLookup::Cookie => {
                // Extract from cookie
                let cookie_header = req.headers().get(header::COOKIE)?;
                let cookie_str = cookie_header.to_str().ok()?;
                
                // Find the access_token cookie
                for cookie in cookie_str.split(';') {
                    let cookie = cookie.trim();
                    if let Some(pos) = cookie.find('=') {
                        let (name, value) = cookie.split_at(pos);
                        if name == "access_token" && value.len() > 1 {
                            return Some(value[1..].to_string());
                        }
                    }
                }
                
                debug!("No access_token cookie found");
                None
            }
        }
    }
    
    /// Validate and decode a JWT token
    fn validate_token(&self, token: &str) -> Result<serde_json::Value> {
        // Create validation parameters
        let mut validation = Validation::new(match self.config.algorithm {
            JwtAlgorithm::HS256 => Algorithm::HS256,
            JwtAlgorithm::HS384 => Algorithm::HS384,
            JwtAlgorithm::HS512 => Algorithm::HS512,
            JwtAlgorithm::RS256 => Algorithm::RS256,
            JwtAlgorithm::RS384 => Algorithm::RS384,
            JwtAlgorithm::RS512 => Algorithm::RS512,
            JwtAlgorithm::ES256 => Algorithm::ES256,
            JwtAlgorithm::ES384 => Algorithm::ES384,
            JwtAlgorithm::ES512 => Algorithm::ES512,
        });
        
        // Configure validation based on settings
        validation.validate_exp = !self.config.allow_tokens_without_exp;
        
        if let Some(ref iss) = self.config.issuer {
            validation.set_issuer(&[iss]);
        }
        
        if let Some(ref aud) = self.config.audience {
            validation.set_audience(&[aud]);
        }
        
        // Create decoding key
        let key = match self.config.algorithm {
            JwtAlgorithm::HS256 | JwtAlgorithm::HS384 | JwtAlgorithm::HS512 => {
                let secret = self.config.secret.as_ref()
                    .context("JWT plugin configuration error: Missing secret key")?;
                DecodingKey::from_secret(secret.as_bytes())
            },
            _ => {
                let public_key = self.config.public_key.as_ref()
                    .context("JWT plugin configuration error: Missing public key")?;
                DecodingKey::from_rsa_pem(public_key.as_bytes())?
            }
        };
        
        // Decode and validate the token
        let token_data = decode::<serde_json::Value>(token, &key, &validation)?;
        
        Ok(token_data.claims)
    }
    
    /// Find a consumer based on the JWT claims
    async fn find_consumer(&self, claims: &serde_json::Value, ctx: &RequestContext) -> Option<Consumer> {
        // Extract the consumer identifier from the claims
        let consumer_id = match claims.get(&self.config.consumer_claim_field) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => {
                warn!(
                    "JWT token does not contain a valid {} claim",
                    self.config.consumer_claim_field
                );
                return None;
            }
        };
        
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
        }
        
        debug!("Consumer with ID '{}' not found in active configuration", consumer_id);
        None
    }
}

#[async_trait]
impl Plugin for JwtAuthPlugin {
    fn name(&self) -> &'static str {
        "jwt_auth"
    }
    
    async fn authenticate(&self, req: &mut Request<Body>, ctx: &mut RequestContext) -> Result<bool> {
        // Skip if a consumer is already identified (multi-auth mode)
        if ctx.consumer.is_some() {
            debug!("Consumer already identified, skipping JWT authentication");
            return Ok(true);
        }
        
        // Extract the token from the request
        let token = match self.extract_token(req) {
            Some(token) => token,
            None => {
                debug!("No JWT token found in request");
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Validate the token
        let claims = match self.validate_token(&token) {
            Ok(claims) => claims,
            Err(e) => {
                warn!("JWT token validation failed: {}", e);
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Find the consumer based on the token claims
        let consumer = match self.find_consumer(&claims, ctx).await {
            Some(consumer) => consumer,
            None => {
                warn!("No consumer found for JWT token");
                
                // In multi-auth mode, we continue even if this auth method failed
                if ctx.proxy.auth_mode == crate::config::data_model::AuthMode::Multi {
                    return Ok(true);
                }
                
                return Ok(false);
            }
        };
        
        // Set the consumer in the context
        ctx.consumer = Some(consumer);
        debug!("Consumer identified by JWT token: {}", ctx.consumer.as_ref().unwrap().username);
        
        Ok(true)
    }
}
