#[cfg(test)]
mod integration_tests {
    use std::collections::HashMap;
    use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    use std::sync::Arc;
    use std::time::Duration;
    use chrono::Utc;
    use http::{HeaderMap, Method, Request, Response, StatusCode};
    use hyper::{Body, Client, Server};
    use hyper::service::{make_service_fn, service_fn};
    use tokio::sync::RwLock;
    use tokio::time::timeout;
    use serde_json::json;
    
    use ferrumgw::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode};
    use ferrumgw::proxy::handler::ProxyHandler;
    use ferrumgw::proxy::router::Router;
    use ferrumgw::plugins::PluginManager;
    
    // Helper to create a test proxy
    fn create_test_proxy(id: &str, listen_path: &str, backend_host: &str, backend_port: u16) -> Proxy {
        Proxy {
            id: id.to_string(),
            name: Some(format!("Test Proxy {}", id)),
            listen_path: listen_path.to_string(),
            backend_protocol: Protocol::Http,
            backend_host: backend_host.to_string(),
            backend_port,
            backend_path: None,
            strip_listen_path: true,
            preserve_host_header: false,
            backend_connect_timeout_ms: 5000,
            backend_read_timeout_ms: 30000,
            backend_write_timeout_ms: 30000,
            backend_tls_client_cert_path: None,
            backend_tls_client_key_path: None,
            backend_tls_verify_server_cert: true,
            backend_tls_server_ca_cert_path: None,
            dns_override: None,
            dns_cache_ttl_seconds: None,
            auth_mode: AuthMode::Single,
            plugins: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
    
    // Create a simple mock backend server
    async fn start_mock_backend() -> SocketAddr {
        // Define a simple handler that returns different responses based on the path
        async fn mock_handler(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
            let path = req.uri().path();
            let method = req.method().clone();
            
            let response = match (method, path) {
                (Method::GET, "/echo") => {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(json!({
                            "message": "Echo response",
                            "path": path
                        }).to_string()))
                        .unwrap()
                },
                (Method::GET, "/error") => {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Internal server error"))
                        .unwrap()
                },
                (Method::POST, "/users") => {
                    // Echo back the request body
                    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
                    Response::builder()
                        .status(StatusCode::CREATED)
                        .header("content-type", "application/json")
                        .body(Body::from(body_bytes))
                        .unwrap()
                },
                _ => {
                    Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::from("Not found"))
                        .unwrap()
                }
            };
            
            Ok(response)
        }
        
        // Create a service from the handler
        let make_svc = make_service_fn(|_| {
            async { Ok::<_, hyper::Error>(service_fn(mock_handler)) }
        });
        
        // Bind to a random port
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
        let server = Server::bind(&addr).serve(make_svc);
        let server_addr = server.local_addr();
        
        // Spawn the server in the background
        tokio::spawn(async move {
            if let Err(e) = server.await {
                eprintln!("Server error: {}", e);
            }
        });
        
        // Return the server's address
        server_addr
    }
    
    #[tokio::test]
    async fn test_proxy_request_forwarding() {
        // Start a mock backend server
        let backend_addr = start_mock_backend().await;
        
        // Create test configuration with a proxy pointing to our mock backend
        let proxy = create_test_proxy(
            "test",
            "/api",
            "127.0.0.1",
            backend_addr.port()
        );
        
        let config = Configuration {
            proxies: vec![proxy],
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        
        let config_store = Arc::new(RwLock::new(config));
        let router = Router::new(config_store.clone());
        let plugin_manager = PluginManager::new();
        
        // Create a ProxyHandler
        let handler = ProxyHandler::new(router, Arc::new(plugin_manager), Arc::new(None));
        
        // Create a test request to the proxy
        let request = Request::builder()
            .method(Method::GET)
            .uri("http://gateway.example.com/api/echo")
            .header("host", "gateway.example.com")
            .body(Body::empty())
            .unwrap();
        
        // Process the request through the proxy handler
        let response = handler.handle_request(
            request,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345)
        ).await.unwrap();
        
        // Verify the response
        assert_eq!(response.status(), StatusCode::OK);
        
        // Read the response body
        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        
        // Check that the response contains the expected data
        assert_eq!(json["message"], "Echo response");
        assert_eq!(json["path"], "/echo");
    }
    
    #[tokio::test]
    async fn test_proxy_error_handling() {
        // Start a mock backend server
        let backend_addr = start_mock_backend().await;
        
        // Create test configuration with a proxy pointing to our mock backend
        let proxy = create_test_proxy(
            "test",
            "/api",
            "127.0.0.1",
            backend_addr.port()
        );
        
        let config = Configuration {
            proxies: vec![proxy],
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        
        let config_store = Arc::new(RwLock::new(config));
        let router = Router::new(config_store.clone());
        let plugin_manager = PluginManager::new();
        
        // Create a ProxyHandler
        let handler = ProxyHandler::new(router, Arc::new(plugin_manager), Arc::new(None));
        
        // Create a test request to the error endpoint
        let request = Request::builder()
            .method(Method::GET)
            .uri("http://gateway.example.com/api/error")
            .header("host", "gateway.example.com")
            .body(Body::empty())
            .unwrap();
        
        // Process the request through the proxy handler
        let response = handler.handle_request(
            request,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345)
        ).await.unwrap();
        
        // Verify the response
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        
        // Read the response body
        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        
        // Check that the response contains the expected error message
        assert_eq!(body_str, "Internal server error");
    }
    
    #[tokio::test]
    async fn test_proxy_not_found() {
        // Start a mock backend server
        let backend_addr = start_mock_backend().await;
        
        // Create test configuration with a proxy pointing to our mock backend
        let proxy = create_test_proxy(
            "test",
            "/api",
            "127.0.0.1",
            backend_addr.port()
        );
        
        let config = Configuration {
            proxies: vec![proxy],
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        
        let config_store = Arc::new(RwLock::new(config));
        let router = Router::new(config_store.clone());
        let plugin_manager = PluginManager::new();
        
        // Create a ProxyHandler
        let handler = ProxyHandler::new(router, Arc::new(plugin_manager), Arc::new(None));
        
        // Create a test request with a path that doesn't match any proxy
        let request = Request::builder()
            .method(Method::GET)
            .uri("http://gateway.example.com/non-existent")
            .header("host", "gateway.example.com")
            .body(Body::empty())
            .unwrap();
        
        // Process the request through the proxy handler
        let response = handler.handle_request(
            request,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345)
        ).await.unwrap();
        
        // Verify the response
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    
    #[tokio::test]
    async fn test_post_request_with_body() {
        // Start a mock backend server
        let backend_addr = start_mock_backend().await;
        
        // Create test configuration with a proxy pointing to our mock backend
        let proxy = create_test_proxy(
            "test",
            "/api",
            "127.0.0.1",
            backend_addr.port()
        );
        
        let config = Configuration {
            proxies: vec![proxy],
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        
        let config_store = Arc::new(RwLock::new(config));
        let router = Router::new(config_store.clone());
        let plugin_manager = PluginManager::new();
        
        // Create a ProxyHandler
        let handler = ProxyHandler::new(router, Arc::new(plugin_manager), Arc::new(None));
        
        // Create a test POST request with a JSON body
        let request_body = json!({
            "name": "Test User",
            "email": "test@example.com"
        }).to_string();
        
        let request = Request::builder()
            .method(Method::POST)
            .uri("http://gateway.example.com/api/users")
            .header("host", "gateway.example.com")
            .header("content-type", "application/json")
            .body(Body::from(request_body.clone()))
            .unwrap();
        
        // Process the request through the proxy handler
        let response = handler.handle_request(
            request,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345)
        ).await.unwrap();
        
        // Verify the response
        assert_eq!(response.status(), StatusCode::CREATED);
        
        // Read the response body
        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        
        // Check that the response body matches our request body (echo)
        assert_eq!(body_str, request_body);
    }
    
    // Additional tests would go here:
    // 1. Test WebSocket proxying
    // 2. Test gRPC proxying
    // 3. Test with authentication plugins
    // 4. Test with request/response transformation plugins
    // 5. Test rate limiting
}
