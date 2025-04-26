use std::sync::Arc;
use tokio::sync::RwLock;
use hyper::{Request, Body};
use tracing::{debug, trace};

use crate::config::data_model::{Configuration, Proxy};

/// The Router is responsible for matching incoming requests to the appropriate proxy
/// configuration using a longest prefix match algorithm.
pub struct Router {
    shared_config: Arc<RwLock<Configuration>>,
}

impl Router {
    pub fn new(shared_config: Arc<RwLock<Configuration>>) -> Self {
        Self { shared_config }
    }
    
    /// Routes a request to the appropriate proxy configuration using
    /// longest prefix matching on the request path.
    pub async fn route(&self, req: &Request<Body>) -> Option<Proxy> {
        let path = req.uri().path();
        
        // Get all proxy configurations
        let config = self.shared_config.read().await;
        
        // Find the proxy with the longest matching listen_path
        let mut matched_proxy: Option<Proxy> = None;
        let mut longest_match_len = 0;
        
        for proxy in &config.proxies {
            let listen_path = &proxy.listen_path;
            
            // Check if path starts with listen_path
            if path.starts_with(listen_path) {
                // Check if this is a longer match than the current best
                let listen_path_len = listen_path.len();
                if listen_path_len > longest_match_len {
                    // This is a better match (longer prefix)
                    longest_match_len = listen_path_len;
                    matched_proxy = Some(proxy.clone());
                    
                    trace!("Found better match for path '{}': listen_path='{}', proxy='{}'", 
                          path, listen_path, proxy.name.as_deref().unwrap_or("unnamed"));
                }
            }
        }
        
        if let Some(ref proxy) = matched_proxy {
            debug!("Matched request path '{}' to proxy '{}'", 
                  path, proxy.name.as_deref().unwrap_or("unnamed"));
        } else {
            debug!("No matching proxy found for path '{}'", path);
        }
        
        matched_proxy
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
