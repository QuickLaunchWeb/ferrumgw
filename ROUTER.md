# Ferrum Gateway Router Implementation

This document explains the implementation of the high-performance path router in Ferrum Gateway, which uses a Radix Tree data structure for efficient path matching.

## Overview

The router is responsible for matching incoming client requests to the appropriate proxy configuration based on URL paths. The Ferrum Gateway router is now implemented using a Radix Tree (also known as a Patricia Trie) for optimal performance.

## Radix Tree Routing

### What is a Radix Tree?

A Radix Tree is a compressed prefix tree specialized for storing strings. It's particularly efficient for:

- Longest prefix matching (exactly what an API gateway needs)
- Sharing common prefixes to minimize memory usage
- Fast lookups regardless of the number of routes

### Performance Characteristics

| Operation | Time Complexity | Space Complexity |
|-----------|----------------|------------------|
| Lookup    | O(k) where k is the length of the path | O(n) where n is the total number of nodes |
| Insert    | O(k) where k is the length of the path | O(n) where n is the total number of nodes |
| Delete    | O(k) where k is the length of the path | O(n) where n is the total number of nodes |

Unlike the previous linear search implementation (O(n) where n is the number of routes), the Radix Tree lookup time is dependent only on the path length, not the number of routes.

## Implementation Details

The implementation uses the `matchit` crate, a high-performance path router for Rust that implements a Radix Tree internally.

### Core Components

1. **Router Struct**: Manages the Radix Tree and provides route matching functionality
   ```rust
   pub struct Router {
       shared_config: Arc<RwLock<Configuration>>,
       route_tree: Arc<RwLock<MatchitRouter<String>>>, // Stores proxy IDs in the tree
   }
   ```

2. **UpdateManager**: Manages updating the routing tree when configuration changes
   ```rust
   pub struct UpdateManager {
       router: Arc<Router>,
       update_tx: broadcast::Sender<RouterUpdate>,
   }
   ```

3. **Route Matching Process**:
   ```rust
   pub async fn route(&self, req: &Request<Body>) -> Option<Proxy> {
       let path = req.uri().path();
       let route_tree = self.route_tree.read().await;
       
       match route_tree.at(path) {
           Ok(route_match) => {
               let proxy_id = route_match.value;
               // Retrieve the full proxy configuration
               // ...
           },
           Err(_) => None
       }
   }
   ```

### How Path Matching Works

1. When a request arrives, the path is extracted from the URI
2. The path is looked up in the Radix Tree
3. If a match is found, the associated proxy ID is retrieved
4. The full proxy configuration is then obtained using the ID
5. The request is forwarded to the appropriate backend

### Route Pattern Format

Routes in the router are stored in a specific format:

- All paths end with `/*` to capture subpaths
- Example: `/api/v1/` becomes `/api/v1/*`
- This ensures proper longest-prefix matching behavior

### Automatic Route Tree Updates

The router is automatically updated when proxy configurations change:

1. **Initialization**: The route tree is built asynchronously when the server starts
2. **Admin API Changes**: When proxies are added, updated, or deleted through the Admin API, the update manager is notified
3. **Tree Rebuilding**: The route tree is rebuilt with the new configuration

## Dynamic Updating Mechanism

The router uses a publish/subscribe pattern to handle configuration changes:

1. The `UpdateManager` maintains a broadcast channel for notifications
2. Admin API handlers send notifications when proxy configurations change
3. Background tasks subscribe to these notifications and rebuild the route tree
4. The router always uses the latest routing table

## Comparison with Previous Implementation

| Aspect | Previous (Linear Search) | New (Radix Tree) |
|--------|--------------------------|------------------|
| Time Complexity | O(n) - Linear with number of routes | O(k) - Linear with path length |
| Memory Usage | Lower (simpler data structure) | Slightly higher (trie nodes) |
| Scalability | Poor with many routes | Excellent even with thousands of routes |
| Path Matching | Simple prefix checking | Full pattern matching with wildcards |

## Concurrency and Threading

The router is designed for concurrent access:

- The route tree is wrapped in `Arc<RwLock<>>` for thread-safe access
- Read-heavy workload is optimized with RwLock (multiple readers, single writer)
- Updates are processed asynchronously to minimize blocking

## Ensuring Correct Longest Path Matching

The Radix Tree implementation guarantees that the longest matching path is selected:

1. All routes are indexed in the tree by their full path pattern
2. The `matchit` library ensures the most specific (longest) match wins
3. Path conflicts are detected at insertion time, preventing ambiguous routes

This preserves the same matching behavior as before, but with much better performance characteristics.

## Practical Examples

| Request Path | Routes in Tree | Match Result |
|--------------|----------------|-------------|
| `/api/v1/users` | `/api/*`, `/api/v1/*`, `/api/v2/*` | `/api/v1/*` |
| `/api/v2/products` | `/api/*`, `/api/v1/*`, `/api/v2/*` | `/api/v2/*` |
| `/api/v3/test` | `/api/*`, `/api/v1/*`, `/api/v2/*` | `/api/*` |
| `/public/docs` | `/api/*`, `/public/*` | `/public/*` |
| `/unknown` | `/api/*`, `/public/*` | No match |
