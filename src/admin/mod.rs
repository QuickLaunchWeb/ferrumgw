use std::sync::Arc;
use std::net::SocketAddr;
use std::collections::HashMap;
use tokio::sync::{RwLock, broadcast};
use tokio::net::TcpListener;
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use hyper::server::conn::Http;
use hyper::service::service_fn;
use hyper::{Body, Request, Response, StatusCode, Method};
use chrono::Utc;
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm, TokenData};
use serde::{Serialize, Deserialize};

use crate::config::env_config::EnvConfig;
use crate::config::data_model::{Configuration, Proxy, Consumer, PluginConfig};
use crate::database::DatabaseClient;
use crate::proxy::tls;
use crate::modes::OperationMode;
use crate::proxy::update_manager::RouterUpdate;

mod routes;
mod auth;
mod metrics;
pub mod pagination;

/// Claims structure for JWT tokens
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,  // Subject (username)
    exp: u64,     // Expiration time
    iat: u64,     // Issued at
}

/// The Admin API server
pub struct AdminServer {
    env_config: EnvConfig,
    shared_config: Arc<RwLock<Configuration>>,
    db_client: DatabaseClient,
    jwt_secret: String,
}

impl AdminServer {
    pub fn new(
        env_config: EnvConfig,
        shared_config: Arc<RwLock<Configuration>>,
        db_client: DatabaseClient,
        jwt_secret: String,
    ) -> Result<Self> {
        Ok(Self {
            env_config,
            shared_config,
            db_client,
            jwt_secret,
        })
    }
    
    pub async fn start(self) -> Result<()> {
        // Warn if neither HTTP nor HTTPS is enabled
        if self.env_config.admin_http_port.is_none() && self.env_config.admin_https_port.is_none() {
            warn!("No admin ports are enabled. Admin API will not be accessible.");
            return Ok(());
        }
        
        // Start HTTP server if enabled
        if let Some(http_port) = self.env_config.admin_http_port {
            let addr = format!("0.0.0.0:{}", http_port).parse::<SocketAddr>()?;
            let shared_config = Arc::clone(&self.shared_config);
            let db_client = self.db_client.clone();
            let jwt_secret = self.jwt_secret.clone();
            let operation_mode = self.env_config.mode;
            
            info!("Starting HTTP admin server on {}", addr);
            
            tokio::spawn(async move {
                if let Err(e) = Self::run_http_server(
                    addr, 
                    shared_config, 
                    db_client,
                    jwt_secret,
                    operation_mode,
                ).await {
                    error!("HTTP admin server error: {}", e);
                }
            });
        }
        
        // Start HTTPS server if enabled
        if let Some(https_port) = self.env_config.admin_https_port {
            if let (Some(cert_path), Some(key_path)) = (
                &self.env_config.admin_tls_cert_path,
                &self.env_config.admin_tls_key_path,
            ) {
                let addr = format!("0.0.0.0:{}", https_port).parse::<SocketAddr>()?;
                let shared_config = Arc::clone(&self.shared_config);
                let db_client = self.db_client.clone();
                let jwt_secret = self.jwt_secret.clone();
                let cert_path = cert_path.clone();
                let key_path = key_path.clone();
                let operation_mode = self.env_config.mode;
                
                info!("Starting HTTPS admin server on {}", addr);
                
                tokio::spawn(async move {
                    if let Err(e) = Self::run_https_server(
                        addr,
                        cert_path,
                        key_path,
                        shared_config,
                        db_client,
                        jwt_secret,
                        operation_mode,
                    ).await {
                        error!("HTTPS admin server error: {}", e);
                    }
                });
            } else {
                warn!("HTTPS admin port is enabled but TLS certificate and/or key path is not provided. HTTPS admin server will not start.");
            }
        }
        
        // Prevent the function from returning (server runs in background tasks)
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await; // Sleep for an hour
        }
    }
    
    async fn run_http_server(
        addr: SocketAddr,
        shared_config: Arc<RwLock<Configuration>>,
        db_client: DatabaseClient,
        jwt_secret: String,
        operation_mode: OperationMode,
    ) -> Result<()> {
        // Create TCP listener
        let listener = TcpListener::bind(addr).await?;
        
        // Create state for request handlers
        let state = Arc::new(AdminApiState {
            shared_config,
            db_client,
            jwt_secret,
            operation_mode,
            update_tx: None,
        });
        
        // Accept and serve connections
        loop {
            let (stream, remote_addr) = match listener.accept().await {
                Ok((stream, addr)) => (stream, addr),
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };
            
            // Clone state for the connection handler
            let state_clone = Arc::clone(&state);
            
            // Configure HTTP server
            let http = Http::new()
                .with_executor(tokio::runtime::Handle::current());
            
            // Spawn a task to serve the connection
            tokio::spawn(async move {
                if let Err(e) = http
                    .serve_connection(
                        stream,
                        service_fn(move |req| {
                            let state = Arc::clone(&state_clone);
                            async move {
                                handle_request(req, state).await
                            }
                        }),
                    )
                    .await
                {
                    error!("Error serving admin connection: {}", e);
                }
            });
        }
    }
    
    async fn run_https_server(
        addr: SocketAddr,
        cert_path: String,
        key_path: String,
        shared_config: Arc<RwLock<Configuration>>,
        db_client: DatabaseClient,
        jwt_secret: String,
        operation_mode: OperationMode,
    ) -> Result<()> {
        // Load TLS configuration
        let tls_config = tls::load_server_config(&cert_path, &key_path)
            .context("Failed to load TLS configuration")?;
        
        // Create TCP listener
        let listener = TcpListener::bind(addr).await?;
        
        // Create state for request handlers
        let state = Arc::new(AdminApiState {
            shared_config,
            db_client,
            jwt_secret,
            operation_mode,
            update_tx: None,
        });
        
        // Accept and serve connections
        loop {
            let (stream, remote_addr) = match listener.accept().await {
                Ok((stream, addr)) => (stream, addr),
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };
            
            // Clone necessary components for the connection handler
            let state_clone = Arc::clone(&state);
            let tls_config = tls_config.clone();
            
            // Perform TLS handshake
            let tls_stream = match tls::accept_connection(stream, tls_config).await {
                Ok(tls_stream) => tls_stream,
                Err(e) => {
                    error!("TLS handshake failed: {}", e);
                    continue;
                }
            };
            
            // Configure HTTP server
            let http = Http::new()
                .with_executor(tokio::runtime::Handle::current());
            
            // Spawn a task to serve the connection
            tokio::spawn(async move {
                if let Err(e) = http
                    .serve_connection(
                        tls_stream,
                        service_fn(move |req| {
                            let state = Arc::clone(&state_clone);
                            async move {
                                handle_request(req, state).await
                            }
                        }),
                    )
                    .await
                {
                    error!("Error serving admin TLS connection: {}", e);
                }
            });
        }
    }
}

/// Shared state for the Admin API server
pub struct AdminApiState {
    pub shared_config: Arc<RwLock<Configuration>>,
    pub db_client: DatabaseClient,
    pub jwt_secret: String,
    pub operation_mode: OperationMode,
    pub update_tx: Option<broadcast::Sender<RouterUpdate>>,
}

/// Handle an incoming request to the Admin API
async fn handle_request(
    req: Request<Body>,
    state: Arc<AdminApiState>,
) -> Result<Response<Body>, hyper::Error> {
    // Check if this is a health check (doesn't require authentication)
    if req.uri().path() == "/health" || req.uri().path() == "/status" {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(r#"{"status":"ok"}"#))
            .unwrap());
    }
    
    // Authenticate the request (except for health check)
    match authenticate_request(&req, &state.jwt_secret) {
        Ok(claims) => {
            // Request is authenticated, route it to the appropriate handler
            match route_request(req, state, claims).await {
                Ok(response) => Ok(response),
                Err(e) => {
                    error!("Error handling admin request: {}", e);
                    
                    Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from(format!("{{\"error\":\"{}\"}}", e)))
                        .unwrap())
                }
            }
        },
        Err(e) => {
            // Authentication failed
            debug!("Authentication failed: {}", e);
            
            Ok(Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("WWW-Authenticate", "Bearer")
                .body(Body::from(r#"{"error":"Unauthorized"}"#))
                .unwrap())
        }
    }
}

/// Authenticate a request using JWT
fn authenticate_request(req: &Request<Body>, jwt_secret: &str) -> Result<Claims> {
    // Get the Authorization header
    let auth_header = req.headers()
        .get("Authorization")
        .ok_or_else(|| anyhow::anyhow!("Missing Authorization header"))?;
    
    // Extract the token
    let auth_str = auth_header.to_str()?;
    if !auth_str.starts_with("Bearer ") {
        return Err(anyhow::anyhow!("Invalid Authorization header format"));
    }
    
    let token = &auth_str[7..]; // Skip "Bearer "
    
    // Validate the token
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )?;
    
    Ok(token_data.claims)
}

/// Route a request to the appropriate handler
async fn route_request(
    req: Request<Body>,
    state: Arc<AdminApiState>,
    claims: Claims,
) -> Result<Response<Body>> {
    // Extract path and method
    let path = req.uri().path();
    let method = req.method();
    
    // Route based on path and method
    match (method, path) {
        (&Method::GET, "/proxies") => {
            routes::proxies::list_proxies(req, state).await
        },
        (&Method::POST, "/proxies") => {
            routes::proxies::create_proxy(req, state).await
        },
        (&Method::GET, path) if path.starts_with("/proxies/") => {
            let proxy_id = &path[9..]; // Skip "/proxies/"
            routes::proxies::get_proxy(proxy_id, state).await
        },
        (&Method::PUT, path) if path.starts_with("/proxies/") => {
            let proxy_id = &path[9..]; // Skip "/proxies/"
            routes::proxies::update_proxy(proxy_id, req, state).await
        },
        (&Method::DELETE, path) if path.starts_with("/proxies/") => {
            let proxy_id = &path[9..]; // Skip "/proxies/"
            routes::proxies::delete_proxy(proxy_id, state).await
        },
        (&Method::GET, "/consumers") => {
            routes::consumers::list_consumers(req, state).await
        },
        (&Method::POST, "/consumers") => {
            routes::consumers::create_consumer(req, state).await
        },
        (&Method::GET, path) if path.starts_with("/consumers/") => {
            if path.contains("/credentials/") {
                // Handle credentials endpoint
                let parts: Vec<&str> = path.split('/').collect();
                if parts.len() == 4 {
                    let consumer_id = parts[2];
                    let credential_type = parts[3];
                    routes::consumers::get_consumer_credentials(consumer_id, credential_type, state).await
                } else {
                    Err(anyhow::anyhow!("Invalid path format"))
                }
            } else {
                // Handle consumer endpoint
                let consumer_id = &path[11..]; // Skip "/consumers/"
                routes::consumers::get_consumer(consumer_id, state).await
            }
        },
        (&Method::PUT, path) if path.starts_with("/consumers/") => {
            if path.contains("/credentials/") {
                // Handle credentials endpoint
                let parts: Vec<&str> = path.split('/').collect();
                if parts.len() == 4 {
                    let consumer_id = parts[2];
                    let credential_type = parts[3];
                    routes::consumers::update_consumer_credentials(consumer_id, credential_type, req, state).await
                } else {
                    Err(anyhow::anyhow!("Invalid path format"))
                }
            } else {
                // Handle consumer endpoint
                let consumer_id = &path[11..]; // Skip "/consumers/"
                routes::consumers::update_consumer(consumer_id, req, state).await
            }
        },
        (&Method::DELETE, path) if path.starts_with("/consumers/") => {
            if path.contains("/credentials/") {
                // Handle credentials endpoint
                let parts: Vec<&str> = path.split('/').collect();
                if parts.len() == 4 {
                    let consumer_id = parts[2];
                    let credential_type = parts[3];
                    routes::consumers::delete_consumer_credentials(consumer_id, credential_type, state).await
                } else {
                    Err(anyhow::anyhow!("Invalid path format"))
                }
            } else {
                // Handle consumer endpoint
                let consumer_id = &path[11..]; // Skip "/consumers/"
                routes::consumers::delete_consumer(consumer_id, state).await
            }
        },
        (&Method::GET, "/plugins") => {
            routes::plugins::list_plugin_types(req, state).await
        },
        (&Method::GET, "/plugins/config") => {
            routes::plugins::list_plugin_configs(req, state).await
        },
        (&Method::POST, "/plugins/config") => {
            routes::plugins::create_plugin_config(req, state).await
        },
        (&Method::GET, path) if path.starts_with("/plugins/config/") => {
            let config_id = &path[15..]; // Skip "/plugins/config/"
            routes::plugins::get_plugin_config(config_id, state).await
        },
        (&Method::PUT, path) if path.starts_with("/plugins/config/") => {
            let config_id = &path[15..]; // Skip "/plugins/config/"
            routes::plugins::update_plugin_config(config_id, req, state).await
        },
        (&Method::DELETE, path) if path.starts_with("/plugins/config/") => {
            let config_id = &path[15..]; // Skip "/plugins/config/"
            routes::plugins::delete_plugin_config(config_id, state).await
        },
        (&Method::GET, "/admin/metrics") => {
            metrics::get_metrics(state).await
        },
        _ => {
            // Route not found
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from(r#"{"error":"Not Found"}"#))
                .unwrap())
        }
    }
}
