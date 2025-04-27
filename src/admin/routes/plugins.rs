use std::sync::Arc;
use anyhow::Result;
use hyper::{Body, Request, Response, StatusCode};
use tracing::{debug, error, info};

use crate::admin::AdminApiState;
use crate::config::data_model::PluginConfig;
use crate::plugins::PluginManager;
use crate::modes::OperationMode;
use crate::admin::pagination::{PaginationQuery, create_paginated_response};

/// Handler for GET /plugins endpoint - lists all available plugin types
pub async fn list_plugin_types(req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Extract pagination parameters
    let pagination = PaginationQuery::from_request(&req);
    
    // Create a plugin manager to get the list of available plugins
    let plugin_manager = PluginManager::new();
    
    // Get the list of available plugins
    let plugins = plugin_manager.available_plugins();
    
    // Apply pagination to the plugins
    let (paginated_plugins, pagination_meta) = pagination.paginate(&plugins);
    
    // Create the paginated response
    let response = create_paginated_response(paginated_plugins, pagination_meta);
    
    // Serialize to JSON
    let json = serde_json::to_string(&response)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for GET /plugins/config endpoint - lists all plugin configurations
pub async fn list_plugin_configs(req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Extract pagination parameters
    let pagination = PaginationQuery::from_request(&req);
    
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Apply pagination to the plugin configs
    let (paginated_configs, pagination_meta) = pagination.paginate(&config.plugin_configs);
    
    // Create the paginated response
    let response = create_paginated_response(paginated_configs, pagination_meta);
    
    // Serialize to JSON
    let json = serde_json::to_string(&response)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for POST /plugins/config endpoint - creates a new plugin configuration
pub async fn create_plugin_config(req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
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
    
    // Deserialize the plugin config from JSON
    let mut plugin_config = serde_json::from_slice::<PluginConfig>(&body_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid plugin config data: {}", e))?;
    
    // Validate the plugin type
    let plugin_manager = PluginManager::new();
    let available_plugins = plugin_manager.available_plugins();
    
    if !available_plugins.contains(&plugin_config.plugin_name) {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Body::from(format!(
                r#"{{"error":"Invalid plugin type: {}"}}"#,
                plugin_config.plugin_name
            )))
            .unwrap());
    }
    
    // Add timestamp
    let now = chrono::Utc::now();
    plugin_config.created_at = now;
    plugin_config.updated_at = now;
    
    // Create the plugin config in the database
    if let Some(db) = &state.db_client {
        match db.create_plugin_config(&plugin_config).await {
            Ok(id) => {
                // Update the plugin config ID with the one from the database
                plugin_config.id = id;
                
                // Serialize the created plugin config to JSON
                let json = serde_json::to_string(&plugin_config)?;
                
                // Return the response
                Ok(Response::builder()
                    .status(StatusCode::CREATED)
                    .header("Content-Type", "application/json")
                    .body(Body::from(json))
                    .unwrap())
            },
            Err(e) => {
                error!("Failed to create plugin config in database: {}", e);
                
                Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Body::from(format!(r#"{{"error":"Failed to create plugin config: {}"}}"#, e)))
                    .unwrap())
            }
        }
    } else {
        // No database client available
        Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Database is unavailable"}"#))
            .unwrap())
    }
}

/// Handler for GET /plugins/config/{id} endpoint - gets a specific plugin configuration
pub async fn get_plugin_config(config_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Find the plugin config with the specified ID
    let plugin_config = config.plugin_configs
        .iter()
        .find(|pc| pc.id == config_id)
        .ok_or_else(|| anyhow::anyhow!("Plugin config not found"))?;
    
    // Serialize the plugin config to JSON
    let json = serde_json::to_string(plugin_config)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for PUT /plugins/config/{id} endpoint - updates a specific plugin configuration
pub async fn update_plugin_config(config_id: &str, req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
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
    
    // Deserialize the plugin config from JSON
    let mut updated_config = serde_json::from_slice::<PluginConfig>(&body_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid plugin config data: {}", e))?;
    
    // Ensure the ID in the path matches the ID in the body
    if updated_config.id != config_id {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Plugin config ID in path does not match ID in body"}"#))
            .unwrap());
    }
    
    // Validate the plugin type
    let plugin_manager = PluginManager::new();
    let available_plugins = plugin_manager.available_plugins();
    
    if !available_plugins.contains(&updated_config.plugin_name) {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Body::from(format!(
                r#"{{"error":"Invalid plugin type: {}"}}"#,
                updated_config.plugin_name
            )))
            .unwrap());
    }
    
    // Check if plugin config exists
    {
        let config = state.shared_config.read().await;
        
        if !config.plugin_configs.iter().any(|pc| pc.id == config_id) {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"error":"Plugin config not found"}"#))
                .unwrap());
        }
    }
    
    // Update timestamp
    updated_config.updated_at = chrono::Utc::now();
    
    // Update the plugin config in the database
    if let Some(db) = &state.db_client {
        match db.update_plugin_config(&updated_config).await {
            Ok(_) => {
                // Serialize the updated plugin config to JSON
                let json = serde_json::to_string(&updated_config)?;
                
                // Return the response
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Body::from(json))
                    .unwrap())
            },
            Err(e) => {
                error!("Failed to update plugin config in database: {}", e);
                
                Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Body::from(format!(r#"{{"error":"Failed to update plugin config: {}"}}"#, e)))
                    .unwrap())
            }
        }
    } else {
        // No database client available
        Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Database is unavailable"}"#))
            .unwrap())
    }
}

/// Handler for DELETE /plugins/config/{id} endpoint - deletes a specific plugin configuration
pub async fn delete_plugin_config(config_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Check operation mode
    if state.operation_mode == OperationMode::File {
        return Ok(Response::builder()
            .status(StatusCode::CONFLICT)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Cannot modify config — currently running in File Mode"}"#))
            .unwrap());
    }
    
    // Check if the plugin config exists
    {
        let config = state.shared_config.read().await;
        
        if !config.plugin_configs.iter().any(|pc| pc.id == config_id) {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"error":"Plugin config not found"}"#))
                .unwrap());
        }
    }
    
    // Delete the plugin config from the database
    if let Some(db) = &state.db_client {
        match db.delete_plugin_config(config_id).await {
            Ok(_) => {
                // Return the response
                Ok(Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .body(Body::empty())
                    .unwrap())
            },
            Err(e) => {
                error!("Failed to delete plugin config from database: {}", e);
                
                Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Body::from(format!(r#"{{"error":"Failed to delete plugin config: {}"}}"#, e)))
                    .unwrap())
            }
        }
    } else {
        // No database client available
        Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"error":"Database is unavailable"}"#))
            .unwrap())
    }
}
