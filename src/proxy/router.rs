use std::sync::Arc;
use tokio::sync::RwLock;
use hyper::{Request, Body};
use tracing::{debug, trace, warn, info};
use matchit::{Router as MatchitRouter, Match};

use crate::config::data_model::{Configuration, Proxy};

/// The Router is responsible for matching incoming requests to the appropriate proxy
/// configuration using a radix tree for efficient path matching.
pub struct Router {
    shared_config: Arc<RwLock<Configuration>>,
    route_tree: Arc<RwLock<MatchitRouter<String>>>, // Stores proxy IDs in the tree
}

impl Router {
    pub fn new(shared_config: Arc<RwLock<Configuration>>) -> Self {
        let router = Self { 
            shared_config,
            route_tree: Arc::new(RwLock::new(MatchitRouter::new())),
        };
        
        // Initialize the routing tree asynchronously
        let router_clone = router.clone();
        tokio::spawn(async move {
            if let Err(e) = router_clone.rebuild_route_tree().await {
                warn!("Failed to initialize routing tree: {}", e);
            }
        });
        
        router
    }
    
    /// Clone the router
    pub fn clone(&self) -> Self {
        Self {
            shared_config: Arc::clone(&self.shared_config),
            route_tree: Arc::clone(&self.route_tree),
        }
    }
    
    /// Routes a request to the appropriate proxy configuration using
    /// the radix tree for efficient path matching.
    pub async fn route(&self, req: &Request<Body>) -> Option<Proxy> {
        let path = req.uri().path();
        trace!("Routing request for path: {}", path);
        
        // Use the radix tree to find the best match
        let route_tree = self.route_tree.read().await;
        
        match route_tree.at(path) {
            Ok(route_match) => {
                let proxy_id = route_match.value;
                debug!("Matched path '{}' to proxy ID '{}'", path, proxy_id);
                
                // Retrieve the full proxy configuration
                let config = self.shared_config.read().await;
                let proxy = config.proxies.iter()
                    .find(|p| p.id == *proxy_id)
                    .cloned();
                
                if let Some(ref p) = proxy {
                    debug!("Using proxy '{}' for path '{}'", 
                          p.name.as_deref().unwrap_or("unnamed"), path);
                } else {
                    warn!("Found proxy ID '{}' in route tree but not in configuration", proxy_id);
                }
                
                proxy
            },
            Err(_) => {
                debug!("No matching proxy found for path '{}'", path);
                None
            }
        }
    }
    
    /// Rebuilds the routing tree when configuration changes.
    /// This ensures the router always has the latest routes.
    pub async fn rebuild_route_tree(&self) -> anyhow::Result<()> {
        // Get the latest configuration
        let config = self.shared_config.read().await;
        
        // Create a new routing tree
        let mut new_tree = MatchitRouter::new();
        let mut route_count = 0;
        
        // Add all proxies to the tree
        for proxy in &config.proxies {
            let mut path = proxy.listen_path.clone();
            
            // Ensure path ends with * to capture all subpaths
            if !path.ends_with('*') {
                if path.ends_with('/') {
                    path.push('*');
                } else {
                    path.push_str("/*");
                }
            }
            
            match new_tree.insert(path, proxy.id.clone()) {
                Ok(_) => {
                    route_count += 1;
                    trace!("Added route to tree: {} -> {}", proxy.listen_path, proxy.id);
                },
                Err(e) => {
                    warn!("Failed to add route for proxy {}: {}", proxy.id, e);
                }
            }
        }
        
        // Replace the old tree with the new one
        {
            let mut tree = self.route_tree.write().await;
            *tree = new_tree;
        }
        
        info!("Rebuilt routing tree with {} routes", route_count);
        Ok(())
    }
    
    /// Constructs the backend path for a request based on the matched proxy configuration
    /// and the incoming request path.
    pub fn construct_backend_path(&self, req: &Request<Body>, proxy: &Proxy) -> String {
        let incoming_path = req.uri().path();
        let listen_path = &proxy.listen_path;
        
        // Extract the remaining path after the listen_path
        let remaining_path = if incoming_path.len() > listen_path.len() {
            &incoming_path[listen_path.len()..]
        } else {
            ""
        };
        
        // Get the backend_path (default to empty string if None)
        let backend_path = proxy.backend_path.as_deref().unwrap_or("");
        
        // Construct the final path based on the strip_listen_path flag
        if proxy.strip_listen_path {
            // Strip the listen_path and use only the remaining path
            if remaining_path.starts_with('/') || backend_path.ends_with('/') {
                // Avoid double slash
                format!("{}{}", backend_path, remaining_path)
            } else if remaining_path.is_empty() {
                // No remaining path, just use backend_path
                backend_path.to_string()
            } else {
                // Add a slash between backend_path and remaining_path
                format!("{}/{}", backend_path, remaining_path)
            }
        } else {
            // Use the full incoming path
            if incoming_path.starts_with('/') || backend_path.ends_with('/') {
                // Avoid double slash
                format!("{}{}", backend_path, incoming_path)
            } else if incoming_path.is_empty() {
                // No incoming path, just use backend_path
                backend_path.to_string()
            } else {
                // Add a slash between backend_path and incoming_path
                format!("{}/{}", backend_path, incoming_path)
            }
        }
    }
}
