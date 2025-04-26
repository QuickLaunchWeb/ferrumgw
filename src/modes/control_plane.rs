use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time;
use anyhow::{Result, Context};
use tracing::{info, warn, error};

use crate::config::env_config::EnvConfig;
use crate::config::data_model::Configuration;
use crate::database::DatabaseClient;
use crate::admin::AdminServer;

pub async fn run(config: EnvConfig) -> Result<()> {
    info!("Starting Ferrum Gateway in Control Plane mode");
    
    // Get database configuration
    let db_type = config.db_type.clone().context("Database type must be set in Control Plane mode")?;
    let db_url = config.db_url.clone().context("Database URL must be set in Control Plane mode")?;
    
    // Set up database client
    let db_client = DatabaseClient::new(db_type, &db_url)
        .await
        .context("Failed to create database client")?;
    
    // Create shared configuration
    let shared_config = Arc::new(RwLock::new(Configuration {
        proxies: Vec::new(),
        consumers: Vec::new(),
        plugin_configs: Vec::new(),
        last_updated_at: Utc::now(),
    }));
    
    // Load initial configuration from database
    let initial_config = db_client.load_full_configuration()
        .await
        .context("Failed to load initial configuration from database")?;
    
    *shared_config.write().await = initial_config.clone();
    
    // Start admin server
    info!("Starting admin server");
    let admin_jwt_secret = config.admin_jwt_secret.clone()
        .context("Admin JWT secret must be set in Control Plane mode")?;
        
    let admin_server = AdminServer::new(
        config.clone(),
        Arc::clone(&shared_config),
        db_client.clone(),
        admin_jwt_secret,
    )?;
    
    let admin_handle = tokio::spawn(async move {
        if let Err(e) = admin_server.start().await {
            error!("Admin server error: {}", e);
        }
    });
    
    // Start gRPC server for Data Plane nodes
    info!("Starting gRPC server for Data Plane nodes");
    let cp_grpc_jwt_secret = config.cp_grpc_jwt_secret.clone()
        .context("CP gRPC JWT secret must be set in Control Plane mode")?;
    
    let cp_grpc_listen_addr = config.cp_grpc_listen_addr
        .context("CP gRPC listen address must be set in Control Plane mode")?;
    
    let grpc_server = crate::modes::control_plane::grpc::GrpcServer::new(
        cp_grpc_listen_addr,
        cp_grpc_jwt_secret,
        Arc::clone(&shared_config),
    )?;
    
    let grpc_handle = tokio::spawn(async move {
        if let Err(e) = grpc_server.start().await {
            error!("gRPC server error: {}", e);
        }
    });
    
    // Start configuration polling
    let poll_interval = config.db_poll_interval;
    let db_client_clone = db_client.clone();
    let shared_config_clone = Arc::clone(&shared_config);
    
    let polling_handle = tokio::spawn(async move {
        let mut interval = time::interval(poll_interval);
        
        loop {
            interval.tick().await;
            
            match db_client_clone.load_full_configuration().await {
                Ok(new_config) => {
                    // Compare timestamps to see if there are updates
                    let update_needed = {
                        let current_config = shared_config_clone.read().await;
                        new_config.last_updated_at > current_config.last_updated_at
                    };
                    
                    if update_needed {
                        info!("Configuration update detected, applying changes");
                        let mut writable_config = shared_config_clone.write().await;
                        *writable_config = new_config;
                        
                        // The gRPC server will automatically push updates to connected
                        // Data Plane nodes when the shared configuration changes
                    }
                },
                Err(e) => {
                    warn!("Failed to poll configuration from database: {}", e);
                    // Continue operation with existing configuration
                }
            }
        }
    });
    
    // Wait for shutdown signal
    tokio::spawn(async {
        let (_tx, _rx) = tokio::sync::oneshot::channel::<()>();
        let _ = _rx.await;
        info!("Received shutdown signal, stopping Control Plane");
        // Implement graceful shutdown logic here
    });
    
    info!("Shutdown signal received, stopping services");
    
    // The rest of the tasks will be cleaned up when the main function returns
    Ok(())
}

pub mod grpc {
    use std::sync::{Arc, Mutex};
    use std::net::SocketAddr;
    use std::collections::HashMap;
    use tokio::sync::{RwLock, mpsc};
    use tokio_stream::{Stream, wrappers::ReceiverStream};
    use anyhow::{Result, anyhow};
    use tonic::{transport::Server, Request, Response, Status};
    use tracing::{info, warn, error, debug};
    use chrono::Utc;
    use std::time::Duration;
    
    use crate::config::data_model::{Configuration, Proxy, Consumer, PluginConfig};
    use crate::grpc::proto::{
        config_service_server::{ConfigService, ConfigServiceServer},
        SubscribeRequest, ConfigUpdate, ConfigSnapshot, GetConfigSnapshotRequest,
    };
    use crate::grpc::conversions;
    
    /// State shared between all connected DP clients
    #[derive(Debug)]
    struct SharedState {
        /// The current configuration version (incremented on each change)
        version: std::sync::atomic::AtomicU64,
        /// The shared configuration that all nodes access
        shared_config: Arc<RwLock<Configuration>>,
        /// Connected Data Plane clients (client_id -> sender)
        clients: Mutex<HashMap<String, mpsc::Sender<Result<ConfigUpdate, Status>>>>,
        /// JWT secret for authenticating Data Plane nodes
        jwt_secret: String,
    }
    
    impl SharedState {
        /// Broadcasts a configuration update to all connected Data Plane nodes
        async fn broadcast_update(&self) -> Result<()> {
            let config = self.shared_config.read().await;
            let version = self.version.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            
            // Create a ConfigUpdate from the current configuration
            let update = self.create_config_update(&config, version)?;
            
            // Get a copy of the clients hashmap
            let clients = self.clients.lock().unwrap().clone();
            
            // Drop the lock to avoid holding it during potentially slow broadcast
            drop(config);
            
            if clients.is_empty() {
                debug!("No connected Data Plane nodes to broadcast configuration update to");
                return Ok(());
            }
            
            info!("Broadcasting configuration update (v{}) to {} Data Plane nodes", 
                version, clients.len());
            
            let mut disconnected_clients = Vec::new();
            
            // Send the update to each client
            for (client_id, sender) in clients.iter() {
                if let Err(_) = sender.send(Ok(update.clone())).await {
                    // Client is disconnected, mark for removal
                    warn!("Data Plane node {} is disconnected, will remove", client_id);
                    disconnected_clients.push(client_id.clone());
                } else {
                    debug!("Sent configuration update to Data Plane node {}", client_id);
                }
            }
            
            // Remove disconnected clients
            if !disconnected_clients.is_empty() {
                let mut clients = self.clients.lock().unwrap();
                for client_id in disconnected_clients {
                    clients.remove(&client_id);
                }
                info!("Removed {} disconnected Data Plane nodes, {} nodes remain connected", 
                    disconnected_clients.len(), clients.len());
            }
            
            Ok(())
        }
        
        /// Creates a ConfigUpdate message from the current configuration
        fn create_config_update(&self, config: &Configuration, version: u64) -> Result<ConfigUpdate> {
            // Convert domain models to proto messages
            let mut proxies = Vec::new();
            let mut consumers = Vec::new();
            let mut plugin_configs = Vec::new();
            
            for proxy in &config.proxies {
                proxies.push(conversions::From::from(proxy));
            }
            
            for consumer in &config.consumers {
                consumers.push(conversions::From::from(consumer));
            }
            
            for plugin_config in &config.plugin_configs {
                plugin_configs.push(conversions::From::from(plugin_config));
            }
            
            Ok(ConfigUpdate {
                version,
                proxies,
                consumers,
                plugin_configs,
                timestamp: Utc::now().to_rfc3339(),
                update_type: 0, // 0 = Full update
                deleted_ids: Vec::new(), // None for full update
            })
        }
        
        /// Creates a ConfigSnapshot message from the current configuration
        async fn create_config_snapshot(&self) -> Result<ConfigSnapshot> {
            let config = self.shared_config.read().await;
            let version = self.version.load(std::sync::atomic::Ordering::SeqCst);
            
            // Convert domain models to proto messages
            let mut proxies = Vec::new();
            let mut consumers = Vec::new();
            let mut plugin_configs = Vec::new();
            
            for proxy in &config.proxies {
                proxies.push(conversions::From::from(proxy));
            }
            
            for consumer in &config.consumers {
                consumers.push(conversions::From::from(consumer));
            }
            
            for plugin_config in &config.plugin_configs {
                plugin_configs.push(conversions::From::from(plugin_config));
            }
            
            Ok(ConfigSnapshot {
                version,
                proxies,
                consumers,
                plugin_configs,
                timestamp: Utc::now().to_rfc3339(),
            })
        }
        
        /// Registers a new Data Plane client
        async fn register_client(&self, client_id: String, sender: mpsc::Sender<Result<ConfigUpdate, Status>>) {
            info!("Registering new Data Plane node with ID {}", client_id);
            
            // Add the client to the map
            self.clients.lock().unwrap().insert(client_id, sender);
            
            // Log the total number of connected clients
            let client_count = self.clients.lock().unwrap().len();
            info!("Total connected Data Plane nodes: {}", client_count);
        }
        
        /// Validates a JWT token from a Data Plane node
        fn validate_jwt(&self, auth_token: &str) -> Result<(), Status> {
            // First check if token has the Bearer prefix
            let token = if let Some(stripped) = auth_token.strip_prefix("Bearer ") {
                stripped
            } else {
                return Err(Status::unauthenticated("Invalid authorization format"));
            };
            
            // Validate JWT token
            match validate_jwt_token(token, &self.jwt_secret) {
                Ok(_) => {
                    // Token is valid, allow the request to proceed
                    debug!("Successfully validated JWT token for data plane connection");
                },
                Err(e) => {
                    warn!("JWT validation failed: {}", e);
                    return Err(Status::unauthenticated(format!("Invalid token: {}", e)));
                }
            }
            
            Ok(())
        }
    }
    
    /// Implementation of the ConfigService gRPC service
    #[derive(Debug)]
    struct ConfigServiceImpl {
        state: Arc<SharedState>,
    }
    
    #[tonic::async_trait]
    impl ConfigService for ConfigServiceImpl {
        /// Subscribe to configuration updates
        type SubscribeConfigUpdatesStream = ReceiverStream<Result<ConfigUpdate, Status>>;
        
        async fn subscribe_config_updates(
            &self,
            request: Request<SubscribeRequest>,
        ) -> Result<Response<Self::SubscribeConfigUpdatesStream>, Status> {
            let req = request.into_inner();
            let node_id = req.node_id;
            
            // Get auth token from metadata
            let auth_token = match request.metadata().get("authorization") {
                Some(t) => match t.to_str() {
                    Ok(s) => s,
                    Err(_) => return Err(Status::unauthenticated("Invalid authorization header")),
                },
                None => return Err(Status::unauthenticated("Missing authorization header")),
            };
            
            // Validate the JWT
            self.state.validate_jwt(auth_token)?;
            
            // Create channel for configuration updates
            let (tx, rx) = mpsc::channel(10);
            
            // Register this client
            self.state.register_client(node_id.clone(), tx.clone()).await;
            
            // Send initial full configuration update
            let config = self.state.shared_config.read().await;
            let version = self.state.version.load(std::sync::atomic::Ordering::SeqCst);
            
            match self.state.create_config_update(&config, version) {
                Ok(update) => {
                    // Send initial update
                    if let Err(e) = tx.send(Ok(update)).await {
                        error!("Failed to send initial configuration update to Data Plane node {}: {}", node_id, e);
                        return Err(Status::internal("Failed to send initial configuration"));
                    }
                },
                Err(e) => {
                    error!("Failed to create initial configuration update: {}", e);
                    return Err(Status::internal("Failed to create initial configuration update"));
                }
            }
            
            // Return the stream
            Ok(Response::new(ReceiverStream::new(rx)))
        }
        
        /// Get a configuration snapshot
        async fn get_config_snapshot(
            &self,
            request: Request<GetConfigSnapshotRequest>,
        ) -> Result<Response<ConfigSnapshot>, Status> {
            let req = request.into_inner();
            let node_id = req.node_id;
            
            // Get auth token from metadata
            let auth_token = match request.metadata().get("authorization") {
                Some(t) => match t.to_str() {
                    Ok(s) => s,
                    Err(_) => return Err(Status::unauthenticated("Invalid authorization header")),
                },
                None => return Err(Status::unauthenticated("Missing authorization header")),
            };
            
            // Validate the JWT
            self.state.validate_jwt(auth_token)?;
            
            info!("Data Plane node {} requested configuration snapshot", node_id);
            
            // Create and return the snapshot
            let snapshot = match self.state.create_config_snapshot().await {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    error!("Failed to create configuration snapshot: {}", e);
                    return Err(Status::internal("Failed to create configuration snapshot"));
                }
            };
            
            info!("Sending configuration snapshot to Data Plane node {} (v{})", 
                node_id, snapshot.version);
            
            Ok(Response::new(snapshot))
        }
    }
    
    /// Validate a JWT token against the provided secret
    fn validate_jwt_token(token: &str, secret: &str) -> Result<(), String> {
        use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
        use serde::{Serialize, Deserialize};
        
        // Define the claims we expect in the JWT token
        #[derive(Debug, Serialize, Deserialize)]
        struct Claims {
            sub: String,          // Subject (typically the data plane node ID)
            exp: usize,           // Expiration time (as UTC timestamp)
            iat: Option<usize>,   // Issued at (as UTC timestamp)
            #[serde(default)]
            role: String,         // Optional role claim
        }
        
        // Create a decoding key from the secret
        let decoding_key = DecodingKey::from_secret(secret.as_bytes());
        
        // Create a Validation object
        let mut validation = Validation::new(Algorithm::HS256);
        
        // We require the subject claim
        validation.set_required_spec_claims(&["sub", "exp"]);
        
        // Decode and validate the token
        match decode::<Claims>(token, &decoding_key, &validation) {
            Ok(token_data) => {
                // Additional custom validation can be performed here
                // For example, check if the role is "data_plane"
                if !token_data.claims.role.is_empty() && token_data.claims.role != "data_plane" {
                    return Err(format!("Invalid role: {}", token_data.claims.role));
                }
                
                Ok(())
            },
            Err(err) => Err(format!("Token validation error: {}", err)),
        }
    }
    
    #[derive(Debug)]
    pub struct GrpcServer {
        addr: SocketAddr,
        jwt_secret: String,
        shared_config: Arc<RwLock<Configuration>>,
    }
    
    impl GrpcServer {
        pub fn new(
            addr: SocketAddr,
            jwt_secret: String,
            shared_config: Arc<RwLock<Configuration>>,
        ) -> Result<Self> {
            Ok(Self {
                addr,
                jwt_secret,
                shared_config,
            })
        }
        
        pub async fn start(self) -> Result<()> {
            // Create the shared state
            let state = Arc::new(SharedState {
                version: std::sync::atomic::AtomicU64::new(1),
                shared_config: self.shared_config.clone(),
                clients: Mutex::new(HashMap::new()),
                jwt_secret: self.jwt_secret,
            });
            
            // Create the service implementation
            let service = ConfigServiceImpl {
                state: Arc::clone(&state),
            };
            
            // Set up config change watcher to broadcast updates
            let config_watch_state = Arc::clone(&state);
            let config_watch_handle = tokio::spawn(async move {
                let mut last_updated_at = Utc::now();
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                
                loop {
                    interval.tick().await;
                    
                    // Check if configuration has changed
                    let current_updated_at = {
                        config_watch_state.shared_config.read().await.last_updated_at
                    };
                    
                    if current_updated_at > last_updated_at {
                        info!("Configuration has changed, broadcasting update to Data Plane nodes");
                        last_updated_at = current_updated_at;
                        
                        // Broadcast the update
                        if let Err(e) = config_watch_state.broadcast_update().await {
                            error!("Failed to broadcast configuration update: {}", e);
                        }
                    }
                }
            });
            
            // Build the gRPC server
            info!("Starting gRPC server at {}", self.addr);
            let server = Server::builder()
                .add_service(ConfigServiceServer::new(service))
                .serve(self.addr);
            
            // Start the server
            server.await
                .map_err(|e| anyhow!("gRPC server error: {}", e))?;
            
            // This should never be reached
            Ok(())
        }
    }
}
