use std::sync::Arc;
use anyhow::Result;
use hyper::{Body, Request, Response, StatusCode};
use tracing::{debug, error, info};

use crate::admin::AdminApiState;
use crate::config::data_model::Proxy;
use crate::modes::OperationMode;
use crate::proxy::update_manager::RouterUpdate;

/// Handler for GET /proxies endpoint - lists all proxies
pub async fn list_proxies(state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Extract pagination parameters
    let pagination = PaginationQuery::from_request(&Request::new(Body::empty()));
    
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Apply pagination to the proxies
    let (paginated_proxies, pagination_meta) = pagination.paginate(&config.proxies);
    
    // Create the paginated response
    let response = create_paginated_response(paginated_proxies, pagination_meta);
    
    // Serialize to JSON
    let json = serde_json::to_string(&response)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for POST /proxies endpoint - creates a new proxy
pub async fn create_proxy(req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Check operation mode
    if state.operation_mode == OperationMode::File {
        return Ok(Response::builder()
            .status(StatusCode::CONFLICT)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Cannot modify config — currently running in File Mode"}"#))
            .unwrap());
    }
    
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
    match state.db_client.create_proxy(&proxy).await {
        Ok(created_proxy) => {
            // Serialize the created proxy to JSON
            let json = serde_json::to_string(&created_proxy)?;
            
            // Return the response
            let response = Response::builder()
                .status(StatusCode::CREATED)
                .header("Content-Type", "application/json")
                .body(Body::from(json))
                .unwrap();
            
            // Notify the update manager about the configuration change
            if let Some(update_tx) = &state.update_tx {
                if let Err(e) = update_tx.send(RouterUpdate::ConfigChanged) {
                    debug!("Failed to notify router update: {}", e);
                }
            }
            
            Ok(response)
        },
        Err(e) => {
            error!("Failed to create proxy in database: {}", e);
            
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Body::from(format!(r#"{{"error":"Failed to create proxy: {}"}}"#, e)))
                .unwrap())
        }
    }
}

/// Handler for GET /proxies/{id} endpoint - gets a specific proxy
pub async fn get_proxy(proxy_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // First check in-memory configuration
    let proxy = config.proxies.iter().find(|p| p.id == proxy_id).cloned();
    
    // If not found in memory, try to fetch it from the database
    let proxy = if proxy.is_none() {
        debug!("Proxy not found in memory, fetching from database: {}", proxy_id);
        match state.db_client.get_proxy_by_id(proxy_id).await {
            Ok(db_proxy) => Some(db_proxy),
            Err(e) => {
                error!("Failed to fetch proxy {} from database: {}", proxy_id, e);
                None
            }
        }
    } else {
        proxy
    };
    
    // Return 404 if not found
    if proxy.is_none() {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Proxy not found"}"#))
            .unwrap());
    }
    
    let proxy = proxy.unwrap();
    
    // Serialize the proxy to JSON
    let json = serde_json::to_string(&proxy)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for PUT /proxies/{id} endpoint - updates a specific proxy
pub async fn update_proxy(proxy_id: &str, req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Check operation mode
    if state.operation_mode == OperationMode::File {
        return Ok(Response::builder()
            .status(StatusCode::CONFLICT)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Cannot modify config — currently running in File Mode"}"#))
            .unwrap());
    }
    
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
        
        // Check if the proxy exists
        if !config.proxies.iter().any(|p| p.id == proxy_id) {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"error":"Proxy not found"}"#))
                .unwrap());
        }
    }
    
    // Update timestamp
    updated_proxy.updated_at = chrono::Utc::now();
    
    // Update the proxy in the database
    match state.db_client.update_proxy(&updated_proxy).await {
        Ok(_) => {
            // Serialize the updated proxy to JSON
            let json = serde_json::to_string(&updated_proxy)?;
            
            // Return the response
            let response = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Body::from(json))
                .unwrap();
            
            // Notify the update manager about the configuration change
            if let Some(update_tx) = &state.update_tx {
                if let Err(e) = update_tx.send(RouterUpdate::ConfigChanged) {
                    debug!("Failed to notify router update: {}", e);
                }
            }
            
            Ok(response)
        },
        Err(e) => {
            error!("Failed to update proxy in database: {}", e);
            
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Body::from(format!(r#"{{"error":"Failed to update proxy: {}"}}"#, e)))
                .unwrap())
        }
    }
}

/// Handler for DELETE /proxies/{id} endpoint - deletes a specific proxy
pub async fn delete_proxy(proxy_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Check operation mode
    if state.operation_mode == OperationMode::File {
        return Ok(Response::builder()
            .status(StatusCode::CONFLICT)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Cannot modify config — currently running in File Mode"}"#))
            .unwrap());
    }
    
    // Check if the proxy exists
    {
        let config = state.shared_config.read().await;
        
        if !config.proxies.iter().any(|p| p.id == proxy_id) {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"error":"Proxy not found"}"#))
                .unwrap());
        }
    }
    
    // Delete the proxy from the database
    match state.db_client.delete_proxy(proxy_id).await {
        Ok(_) => {
            // Return the response
            let response = Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(Body::empty())
                .unwrap();
            
            // Notify the update manager about the configuration change
            if let Some(update_tx) = &state.update_tx {
                if let Err(e) = update_tx.send(RouterUpdate::ConfigChanged) {
                    debug!("Failed to notify router update: {}", e);
                }
            }
            
            Ok(response)
        },
        Err(e) => {
            error!("Failed to delete proxy from database: {}", e);
            
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(Body::from(format!(r#"{{"error":"Failed to delete proxy: {}"}}"#, e)))
                .unwrap())
        }
    }
}
