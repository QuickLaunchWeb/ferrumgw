use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;
use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};
use serde::{Serialize, Deserialize};

/// Claims structure for JWT tokens
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // Subject (username)
    pub exp: u64,     // Expiration time
    pub iat: u64,     // Issued at
}

/// Generates a JWT token for admin authentication
pub fn generate_admin_token(username: &str, secret: &str, expiry_seconds: u64) -> Result<String> {
    // Get current time
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    
    // Create claims
    let claims = Claims {
        sub: username.to_string(),
        iat: now,
        exp: now + expiry_seconds,
    };
    
    // Generate token
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    
    Ok(token)
}
