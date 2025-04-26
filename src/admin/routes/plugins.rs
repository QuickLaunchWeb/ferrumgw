use std::sync::Arc;
use anyhow::Result;
use hyper::{Body, Request, Response, StatusCode};
use tracing::{debug, error, info};

use crate::admin::AdminApiState;
use crate::config::data_model::PluginConfig;
use crate::plugins::PluginManager;

/// Handler for GET /plugins endpoint - lists all available plugin types
pub async fn list_plugin_types(state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Create a plugin manager to get the list of available plugins
    let plugin_manager = PluginManager::new();
    
    // Get the list of available plugins
    let plugins = plugin_manager.available_plugins();
    
    // Serialize the list to JSON
    let json = serde_json::to_string(&plugins)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for GET /plugins/config endpoint - lists all plugin configurations
pub async fn list_plugin_configs(state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Get the current configuration
    let config = state.shared_config.read().await;
    
    // Serialize the plugin configs to JSON
    let json = serde_json::to_string(&config.plugin_configs)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for POST /plugins/config endpoint - creates a new plugin configuration
pub async fn create_plugin_config(req: Request<Body>, state: Arc<AdminApiState>) -> Result<Response<Body>> {
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
        // Use the database client to create the plugin config
        let result = db.create_plugin_config(&plugin_config).await?;
        // Update the plugin config ID with the one from the database
        plugin_config.id = result;
    } else {
        // If no database client is available, update the in-memory configuration only
        debug!("No database client available, creating plugin config in-memory only");
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        config.plugin_configs.push(plugin_config.clone());
        config.last_updated_at = now;
    }
    
    // Serialize the created plugin config to JSON
    let json = serde_json::to_string(&plugin_config)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
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
    
    // Update timestamp
    updated_config.updated_at = chrono::Utc::now();
    
    // Update the plugin config in the database
    if let Some(db) = &state.db_client {
        // Use the database client to update the plugin config
        db.update_plugin_config(&updated_config).await?;
    } else {
        // If no database client is available, update the in-memory configuration only
        debug!("No database client available, updating plugin config in-memory only");
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        
        // Find the index of the plugin config with the specified ID
        let index = config.plugin_configs
            .iter()
            .position(|pc| pc.id == config_id)
            .ok_or_else(|| anyhow::anyhow!("Plugin config not found"))?;
        
        // Replace the plugin config
        config.plugin_configs[index] = updated_config.clone();
        
        // Update the last updated timestamp
        config.last_updated_at = updated_config.updated_at;
    }
    
    // Serialize the updated plugin config to JSON
    let json = serde_json::to_string(&updated_config)?;
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap())
}

/// Handler for DELETE /plugins/config/{id} endpoint - deletes a specific plugin configuration
pub async fn delete_plugin_config(config_id: &str, state: Arc<AdminApiState>) -> Result<Response<Body>> {
    // Delete the plugin config from the database
    if let Some(db) = &state.db_client {
        // Use the database client to delete the plugin config
        db.delete_plugin_config(config_id).await?;
    } else {
        // If no database client is available, update the in-memory configuration only
        debug!("No database client available, deleting plugin config in-memory only");
    }
    
    // Update the in-memory configuration
    {
        let mut config = state.shared_config.write().await;
        
        // Find the index of the plugin config with the specified ID
        let index = config.plugin_configs
            .iter()
            .position(|pc| pc.id == config_id)
            .ok_or_else(|| anyhow::anyhow!("Plugin config not found"))?;
        
        // Remove the plugin config
        config.plugin_configs.remove(index);
        
        // Update the last updated timestamp
        config.last_updated_at = chrono::Utc::now();
    }
    
    // Return the response
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}
