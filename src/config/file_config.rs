use std::path::Path;
use std::fs;
use anyhow::{Result, Context};
use tracing::info;

use super::data_model::Configuration;

pub fn parse_json_config(content: &str) -> Result<Configuration> {
    serde_json::from_str(content)
        .context("Failed to parse JSON configuration")
}

pub fn parse_yaml_config(content: &str) -> Result<Configuration> {
    serde_yaml::from_str(content)
        .context("Failed to parse YAML configuration")
}

pub fn load_from_directory(dir_path: &Path) -> Result<Configuration> {
    if !dir_path.is_dir() {
        anyhow::bail!("Path is not a directory: {}", dir_path.display());
    }
    
    info!("Loading configuration from directory: {}", dir_path.display());
    
    let mut proxies = Vec::new();
    let mut consumers = Vec::new();
    let mut plugin_configs = Vec::new();
    let mut latest_timestamp = chrono::DateTime::<chrono::Utc>::MIN_UTC;
    
    // Walk through all files in the directory (non-recursive)
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() {
            // Only process .json, .yaml, and .yml files
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                
                if ext_str == "json" || ext_str == "yaml" || ext_str == "yml" {
                    info!("Processing configuration file: {}", path.display());
                    
                    let content = fs::read_to_string(&path)
                        .context(format!("Failed to read file: {}", path.display()))?;
                    
                    let config = if ext_str == "json" {
                        parse_json_config(&content)
                    } else {
                        parse_yaml_config(&content)
                    }?;
                    
                    // Merge the configuration
                    proxies.extend(config.proxies);
                    consumers.extend(config.consumers);
                    plugin_configs.extend(config.plugin_configs);
                    
                    // Update the latest timestamp
                    if config.last_updated_at > latest_timestamp {
                        latest_timestamp = config.last_updated_at;
                    }
                }
            }
        }
    }
    
    // Use current time if no timestamp was found
    if latest_timestamp == chrono::DateTime::<chrono::Utc>::MIN_UTC {
        latest_timestamp = chrono::Utc::now();
    }
    
    Ok(Configuration {
        proxies,
        consumers,
        plugin_configs,
        last_updated_at: latest_timestamp,
    })
}
