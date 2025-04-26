use std::sync::Arc;
use anyhow::Result;
use hyper::{Body, Request, Response, StatusCode};
use tracing::{debug, error, info};
use serde_json::Value;
use bcrypt::{hash, DEFAULT_COST};

use crate::admin::AdminApiState;
use crate::config::data_model::Consumer;

/// Handler for GET /consumers endpoint - lists all consumers
pub async fn list_consumers(state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Serialize the consumers to JSON
    let json = serde_json::to_string(&config.consumers)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for POST /consumers endpoint - creates a new consumer
pub async fn create_consumer(req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Read the request body
    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
    
    // Deserialize the consumer from JSON
    let mut consumer = serde_json::from_slice::<Consumer>(&body_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid consumer data: {}", e))?;
    
    // Verify username uniqueness (in memory check)
    {
        let config = state.shared_config.read().await;
        for existing_consumer in &config.consumers {
            if existing_consumer.username == consumer.username {
                return Ok(Response::builder()
                    .status(StatusCode::CONFLICT)
                    .header("Content-Type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"error":"A consumer with username '{}' already exists"}}"#,
                        consumer.username
                    )))
                    .unwrap());
            }
        }
    }
    
    // Hash any secrets in the credentials
    hash_sensitive_credentials(&mut consumer.credentials)?;
    
    // Add timestamp
    let now = chrono::Utc::now();
    consumer.created_at = now;
    consumer.updated_at = now;
    
    // Create the consumer in the database
    if let Some(db) = &state.db_client {
        // Use the database client to create the consumer
        let result = db.create_consumer(&consumer).await?;
        // Update the consumer ID with the one from the database
        consumer.id = result;
    } else {
        // If no database client is available, update the in-memory configuration only
        debug!("No database client available, creating consumer in-memory only");
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        config.consumers.push(consumer.clone());
        config.last_updated_at = now;
    }
    
    // Serialize the created consumer to JSON
    let json = serde_json::to_string(&consumer)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for GET /consumers/{id} endpoint - gets a specific consumer
pub async fn get_consumer(consumer_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the consumer from the database by ID
    let consumer = {
        // First try looking it up from in-memory configuration
        let config = state.shared_config.read().await;
        let consumer = config.consumers.iter().find(|c| c.id == consumer_id).cloned();
        
        // If not found in memory and we have a database client, try to fetch it from the database
        if consumer.is_none() && state.db_client.is_some() {
            // We'd need to implement a get_consumer method on the database client
            let db = state.db_client.as_ref().unwrap();
            // This would be the real implementation calling the database
            // For now we'll just return None since this method might not exist yet
            None
        } else {
            consumer
        }
    };
    
    // If no consumer was found, return 404
    let consumer = consumer.ok_or_else(|| anyhow::anyhow!("Consumer not found"))?;
    
    // Serialize the consumer to JSON
    let json = serde_json::to_string(&consumer)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for PUT /consumers/{id} endpoint - updates a specific consumer
pub async fn update_consumer(consumer_id: &str, req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Read the request body
    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
    
    // Deserialize the consumer from JSON
    let mut updated_consumer = serde_json::from_slice::<Consumer>(&body_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid consumer data: {}", e))?;
    
    // Ensure the ID in the path matches the ID in the body
    if updated_consumer.id != consumer_id {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Consumer ID in path does not match ID in body"}"#))
            .unwrap());
    }
    
    // Verify username uniqueness (in memory check)
    {
        let config = state.shared_config.read().await;
        for existing_consumer in &config.consumers {
            if existing_consumer.id != consumer_id && existing_consumer.username == updated_consumer.username {
                return Ok(Response::builder()
                    .status(StatusCode::CONFLICT)
                    .header("Content-Type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"error":"A consumer with username '{}' already exists"}}"#,
                        updated_consumer.username
                    )))
                    .unwrap());
            }
        }
    }
    
    // Get current consumer to check if we need to rehash credentials
    let current_consumer = {
        let config = state.shared_config.read().await;
        config.consumers.iter()
            .find(|c| c.id == consumer_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Consumer not found"))?
    };
    
    // Hash any new secrets in the credentials
    for (cred_type, cred_value) in updated_consumer.credentials.iter_mut() {
        // If the credential value is different from the current one, hash it
        if !current_consumer.credentials.contains_key(cred_type) || 
           current_consumer.credentials[cred_type] != *cred_value {
            hash_credential(cred_type, cred_value)?;
        }
    }
    
    // Update timestamp
    updated_consumer.updated_at = chrono::Utc::now();
    
    // Update the consumer in the database
    if let Some(db) = &state.db_client {
        // Use the database client to update the consumer
        db.update_consumer(&updated_consumer).await?;
    } else {
        // If no database client is available, update the in-memory configuration only
        debug!("No database client available, updating consumer in-memory only");
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        
        // Find the index of the consumer with the specified ID
        let index = config.consumers
            .iter()
            .position(|c| c.id == consumer_id)
            .ok_or_else(|| anyhow::anyhow!("Consumer not found"))?;
        
        // Replace the consumer
        config.consumers[index] = updated_consumer.clone();
        
        // Update the last updated timestamp
        config.last_updated_at = updated_consumer.updated_at;
    }
    
    // Serialize the updated consumer to JSON
    let json = serde_json::to_string(&updated_consumer)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for DELETE /consumers/{id} endpoint - deletes a specific consumer
pub async fn delete_consumer(consumer_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Delete the consumer from the database
    if let Some(db) = &state.db_client {
        // Use the database client to delete the consumer
        db.delete_consumer(consumer_id).await?;
    } else {
        // If no database client is available, update the in-memory configuration only
        debug!("No database client available, deleting consumer in-memory only");
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        
        // Find the index of the consumer with the specified ID
        let index = config.consumers
            .iter()
            .position(|c| c.id == consumer_id)
            .ok_or_else(|| anyhow::anyhow!("Consumer not found"))?;
        
        // Remove the consumer
        config.consumers.remove(index);
        
        // Update the last updated timestamp
        config.last_updated_at = chrono::Utc::now();
    }
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

/// Handler for GET /consumers/{id}/credentials/{credential_type} endpoint - gets specific credentials
pub async fn get_consumer_credentials(consumer_id: &str, credential_type: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Find the consumer with the specified ID
    let consumer = config.consumers
        .iter()
        .find(|c| c.id == consumer_id)
        .ok_or_else(|| anyhow::anyhow!("Consumer not found"))?;
    
    // Get the specified credential
    let credential = consumer.credentials.get(credential_type)
        .ok_or_else(|| anyhow::anyhow!("Credential type not found for this consumer"))?;
    
    // Serialize the credential to JSON
    let json = serde_json::to_string(credential)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for PUT /consumers/{id}/credentials/{credential_type} endpoint - updates specific credentials
pub async fn update_consumer_credentials(consumer_id: &str, credential_type: &str, req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Read the request body
    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
    
    // Deserialize the credential from JSON
    let mut credential = serde_json::from_slice::<Value>(&body_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid credential data: {}", e))?;
    
    // Hash the credential if necessary
    hash_credential(credential_type, &mut credential)?;
    
    // Update the consumer in the database
    if let Some(db) = &state.db_client {
        // Here we would find the consumer, update credentials, and save
        debug!("Updating consumer credentials in database");
        
        // Get current consumer data
        let mut consumer = {
            let config = state.shared_config.read().await;
            config.consumers.iter()
                .find(|c| c.id == consumer_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Consumer not found"))?
        };
        
        // Update credentials
        if consumer.credentials.is_none() {
            consumer.credentials = Some(serde_json::Map::new());
        }
        
        let credentials = consumer.credentials.as_mut().unwrap();
        
        // Add the credential
        let cred_list = match credentials.get_mut(credential_type.as_ref()) {
            Some(serde_json::Value::Array(array)) => array,
            Some(_) => {
                // If it exists but isn't an array, replace it
                let mut array = Vec::new();
                credentials.insert(credential_type.to_string(), serde_json::Value::Array(array));
                match credentials.get_mut(credential_type.as_ref()) {
                    Some(serde_json::Value::Array(array)) => array,
                    _ => unreachable!(),
                }
            },
            None => {
                // Create a new array
                let array = Vec::new();
                credentials.insert(credential_type.to_string(), serde_json::Value::Array(array));
                match credentials.get_mut(credential_type.as_ref()) {
                    Some(serde_json::Value::Array(array)) => array,
                    _ => unreachable!(),
                }
            }
        };
        
        // Add the new credential
        cred_list.push(serde_json::Value::String(credential));
        
        // Update the consumer in the database
        db.update_consumer(&consumer).await?;
    } else {
        debug!("No database client available, updating consumer credentials in-memory only");
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        
        // Find the index of the consumer with the specified ID
        let index = config.consumers
            .iter()
            .position(|c| c.id == consumer_id)
            .ok_or_else(|| anyhow::anyhow!("Consumer not found"))?;
        
        // Update the credential
        config.consumers[index].credentials.insert(credential_type.to_string(), credential.clone());
        
        // Update timestamps
        let now = chrono::Utc::now();
        config.consumers[index].updated_at = now;
        config.last_updated_at = now;
    }
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&credential)?))
        .unwrap())
}

/// Handler for DELETE /consumers/{id}/credentials/{credential_type} endpoint - deletes specific credentials
pub async fn delete_consumer_credentials(consumer_id: &str, credential_type: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        
        // Find the index of the consumer with the specified ID
        let index = config.consumers
            .iter()
            .position(|c| c.id == consumer_id)
            .ok_or_else(|| anyhow::anyhow!("Consumer not found"))?;
        
        // Remove the credential
        if config.consumers[index].credentials.remove(credential_type).is_none() {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"error":"Credential type not found for this consumer"}"#))
                .unwrap());
        }
        
        // Update timestamps
        let now = chrono::Utc::now();
        config.consumers[index].updated_at = now;
        config.last_updated_at = now;
    }
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

/// Hash all sensitive credential values in a credentials map
fn hash_sensitive_credentials(credentials: &mut std::collections::HashMap<String, Value>) -> Result<()> {
    for (cred_type, cred_value) in credentials.iter_mut() {
        hash_credential(cred_type, cred_value)?;
    }
    Ok(())
}

/// Hash a credential value based on its type
fn hash_credential(cred_type: &str, cred_value: &mut Value) -> Result<()> {
    match cred_type {
        "basicauth" => {
            // Hash the password in basic auth credentials
            if let Some(password) = cred_value.get_mut("password") {
                if let Some(pass_str) = password.as_str() {
                    let hashed = hash(pass_str, DEFAULT_COST)?;
                    *password = Value::String(hashed);
                }
            }
        },
        "keyauth" => {
            // Hash the key in key auth credentials
            if let Some(key) = cred_value.get_mut("key") {
                if let Some(key_str) = key.as_str() {
                    let hashed = hash(key_str, DEFAULT_COST)?;
                    *key = Value::String(hashed);
                }
            }
        },
        "jwt" => {
            // Hash the secret in JWT credentials
            if let Some(secret) = cred_value.get_mut("secret") {
                if let Some(secret_str) = secret.as_str() {
                    let hashed = hash(secret_str, DEFAULT_COST)?;
                    *secret = Value::String(hashed);
                }
            }
        },
        // For other credential types, no hashing is needed
        _ => {}
    }
    
    Ok(())
}
