use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, broadcast};
use anyhow::Result;
use tracing::{debug, warn, error, info};

use crate::proxy::router::Router;

/// Message type for router update events
#[derive(Debug, Clone)]
pub enum RouterUpdate {
    /// Configuration has changed, rebuild the routing tree
    ConfigChanged,
}

/// The UpdateManager handles notifying relevant components when configuration changes
pub struct UpdateManager {
    router: Arc<Router>,
    update_tx: broadcast::Sender<RouterUpdate>,
}

impl UpdateManager {
    pub fn new(router: Arc<Router>) -> Self {
        // Create a channel for router updates with buffer size of 32
        let (update_tx, _) = broadcast::channel(32);
        
        let manager = Self {
            router,
            update_tx,
        };
        
        // Spawn a task to handle updates
        manager.spawn_update_handler();
        
        manager
    }
    
    /// Notifies all subscribers that the configuration has changed
    pub fn notify_config_changed(&self) -> Result<()> {
        debug!("Notifying config change to update routing tree");
        
        match self.update_tx.send(RouterUpdate::ConfigChanged) {
            Ok(receivers) => {
                debug!("Config change notification sent to {} receivers", receivers);
                Ok(())
            },
            Err(e) => {
                warn!("Failed to send config change notification: {}", e);
                Ok(()) // Non-fatal error, don't propagate
            }
        }
    }
    
    /// Get a receiver for router updates
    pub fn subscribe(&self) -> broadcast::Receiver<RouterUpdate> {
        self.update_tx.subscribe()
    }
    
    /// Spawns a background task to handle update events
    fn spawn_update_handler(&self) {
        let router = Arc::clone(&self.router);
        let mut rx = self.update_tx.subscribe();
        
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(RouterUpdate::ConfigChanged) => {
                        debug!("Received config change notification, rebuilding routing tree");
                        if let Err(e) = router.rebuild_route_tree().await {
                            error!("Failed to rebuild routing tree: {}", e);
                        }
                    },
                    Err(e) => {
                        warn!("Error receiving router update: {}", e);
                        // Try to resubscribe if the channel is lagged
                        if e.is_lagged() {
                            debug!("Resubscribing to router updates due to lag");
                            // Continue the loop, which will use the current rx
                        } else {
                            // Exit the loop for other errors
                            error!("Terminating router update handler due to error: {}", e);
                            break;
                        }
                    }
                }
            }
        });
    }
}
