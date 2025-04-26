use std::sync::Arc;
use anyhow::Result;
use hyper::{Body, Request, Response, StatusCode};
use tracing::{debug, error, info};

use crate::admin::AdminApiState;
use crate::config::data_model::Proxy;

/// Handler for GET /proxies endpoint - lists all proxies
pub async fn list_proxies(state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Serialize the proxies to JSON
    let json = serde_json::to_string(&config.proxies)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for POST /proxies endpoint - creates a new proxy
pub async fn create_proxy(req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Read the request body
    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
    
    // Deserialize the proxy from JSON
    let mut proxy = serde_json::from_slice::<Proxy>(&body_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid proxy data: {}", e))?;
    
    // Verify listen_path uniqueness (in memory check)
    {
        let config = state.shared_config.read().await;
        for existing_proxy in &config.proxies {
            if existing_proxy.listen_path == proxy.listen_path {
                return Ok(Response::builder()
                    .status(StatusCode::CONFLICT)
                    .header("Content-Type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"error":"A proxy with listen_path '{}' already exists"}}"#,
                        proxy.listen_path
                    )))
                    .unwrap());
            }
        }
    }
    
    // Add timestamp
    let now = chrono::Utc::now();
    proxy.created_at = now;
    proxy.updated_at = now;
    
    // Create the proxy in the database
    let created_proxy = state.db_client.create_proxy(proxy).await?;
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        config.proxies.push(created_proxy.clone());
        config.last_updated_at = now;
    }
    
    // Serialize the created proxy to JSON
    let json = serde_json::to_string(&created_proxy)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for GET /proxies/{id} endpoint - gets a specific proxy
pub async fn get_proxy(proxy_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Find the proxy with the specified ID
    let proxy = config.proxies
        .iter()
        .find(|p| p.id == proxy_id)
        .ok_or_else(|| anyhow::anyhow!("Proxy not found"))?;
    
    // Serialize the proxy to JSON
    let json = serde_json::to_string(proxy)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for PUT /proxies/{id} endpoint - updates a specific proxy
pub async fn update_proxy(proxy_id: &str, req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Read the request body
    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
    
    // Deserialize the proxy from JSON
    let mut updated_proxy = serde_json::from_slice::<Proxy>(&body_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid proxy data: {}", e))?;
    
    // Ensure the ID in the path matches the ID in the body
    if updated_proxy.id != proxy_id {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Proxy ID in path does not match ID in body"}"#))
            .unwrap());
    }
    
    // Verify listen_path uniqueness (in memory check)
    {
        let config = state.shared_config.read().await;
        for existing_proxy in &config.proxies {
            if existing_proxy.id != proxy_id && existing_proxy.listen_path == updated_proxy.listen_path {
                return Ok(Response::builder()
                    .status(StatusCode::CONFLICT)
                    .header("Content-Type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"error":"A proxy with listen_path '{}' already exists"}}"#,
                        updated_proxy.listen_path
                    )))
                    .unwrap());
            }
        }
    }
    
    // Update timestamp
    updated_proxy.updated_at = chrono::Utc::now();
    
    // Update the proxy in the database
    match &state.db_client {
        Some(db) => {
            // Use the database client to update the proxy
            db.update_proxy(&updated_proxy).await?;
        },
        None => {
            // If no database client is available, update the in-memory configuration only
            debug!("No database client available, updating proxy in-memory only");
        }
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        
        // Find the index of the proxy with the specified ID
        let index = config.proxies
            .iter()
            .position(|p| p.id == proxy_id)
            .ok_or_else(|| anyhow::anyhow!("Proxy not found"))?;
        
        // Replace the proxy
        config.proxies[index] = updated_proxy.clone();
        
        // Update the last updated timestamp
        config.last_updated_at = updated_proxy.updated_at;
    }
    
    // Serialize the updated proxy to JSON
    let json = serde_json::to_string(&updated_proxy)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for DELETE /proxies/{id} endpoint - deletes a specific proxy
pub async fn delete_proxy(proxy_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Delete the proxy from the database
    match &state.db_client {
        Some(db) => {
            // Use the database client to delete the proxy
            db.delete_proxy(proxy_id).await?;
        },
        None => {
            // If no database client is available, update the in-memory configuration only
            debug!("No database client available, deleting proxy in-memory only");
        }
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        
        // Find the index of the proxy with the specified ID
        let index = config.proxies
            .iter()
            .position(|p| p.id == proxy_id)
            .ok_or_else(|| anyhow::anyhow!("Proxy not found"))?;
        
        // Remove the proxy
        config.proxies.remove(index);
        
        // Update the last updated timestamp
        config.last_updated_at = chrono::Utc::now();
    }
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}
