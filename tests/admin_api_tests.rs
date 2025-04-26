#[cfg(test)]
mod admin_api_tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use chrono::Utc;
    use http::{Request, Response, StatusCode};
    use hyper::Body;
    use serde_json::{json, Value};
    use tokio::sync::RwLock;
    
    use ferrumgw::config::data_model::{Configuration, Proxy, Consumer, PluginConfig, Protocol, AuthMode};
    use ferrumgw::admin::routes::{proxies, consumers, plugins, metrics};
    use ferrumgw::database::memory::InMemoryAdapter;
    
    // Helper function to create a test environment
    async fn create_test_env() -> (Arc<RwLock<Configuration>>, Arc<dyn ferrumgw::database::DatabaseAdapter>) {
        let config = Configuration {
            proxies: Vec::new(),
            consumers: Vec::new(),
            plugin_configs: Vec::new(),
            last_updated_at: Utc::now(),
        };
        
        let shared_config = Arc::new(RwLock::new(config));
        let db_client = Arc::new(InMemoryAdapter::new(shared_config.clone()));
        
        (shared_config, db_client)
    }
    
    // Helper to convert a response to JSON
    async fn response_to_json(response: Response<Body>) -> anyhow::Result<Value> {
        let status = response.status();
        let body_bytes = hyper::body::to_bytes(response.into_body()).await?;
        let json: Value = match serde_json::from_slice(&body_bytes) {
            Ok(j) => j,
            Err(e) => anyhow::bail!("Failed to parse JSON: {}, status: {}, body: {:?}", 
                                    e, status, String::from_utf8_lossy(&body_bytes))
        };
        Ok(json)
    }
    
    #[tokio::test]
    async fn test_proxy_routes() {
        let (shared_config, db_client) = create_test_env().await;
        
        // Test GET /proxies (empty)
        let req = Request::builder()
            .method("GET")
            .uri("/proxies")
            .body(Body::empty())
            .unwrap();
        
        let resp = proxies::handle_get_proxies(req, db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"], json!([]));
        
        // Test POST /proxies
        let proxy_json = json!({
            "name": "Test API",
            "listen_path": "/api/test",
            "backend_protocol": "http",
            "backend_host": "example.com",
            "backend_port": 80,
            "strip_listen_path": true
        });
        
        let req = Request::builder()
            .method("POST")
            .uri("/proxies")
            .header("content-type", "application/json")
            .body(Body::from(proxy_json.to_string()))
            .unwrap();
        
        let resp = proxies::handle_create_proxy(req, db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        
        let json = response_to_json(resp).await.unwrap();
        let proxy_id = json["data"]["id"].as_str().unwrap().to_string();
        
        // Test GET /proxies (now with one proxy)
        let req = Request::builder()
            .method("GET")
            .uri("/proxies")
            .body(Body::empty())
            .unwrap();
        
        let resp = proxies::handle_get_proxies(req, db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
        
        // Test GET /proxies/{id}
        let req = Request::builder()
            .method("GET")
            .uri(format!("/proxies/{}", proxy_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = proxies::handle_get_proxy(req, proxy_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"]["listen_path"], "/api/test");
        
        // Test PUT /proxies/{id}
        let update_json = json!({
            "name": "Updated API",
            "listen_path": "/api/test",
            "backend_protocol": "http",
            "backend_host": "updated.example.com",
            "backend_port": 8080,
            "strip_listen_path": false
        });
        
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/proxies/{}", proxy_id))
            .header("content-type", "application/json")
            .body(Body::from(update_json.to_string()))
            .unwrap();
        
        let resp = proxies::handle_update_proxy(req, proxy_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        // Verify the update
        let req = Request::builder()
            .method("GET")
            .uri(format!("/proxies/{}", proxy_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = proxies::handle_get_proxy(req, proxy_id.clone(), db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"]["name"], "Updated API");
        assert_eq!(json["data"]["backend_host"], "updated.example.com");
        assert_eq!(json["data"]["backend_port"], 8080);
        
        // Test DELETE /proxies/{id}
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/proxies/{}", proxy_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = proxies::handle_delete_proxy(req, proxy_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        
        // Verify deletion
        let req = Request::builder()
            .method("GET")
            .uri(format!("/proxies/{}", proxy_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = proxies::handle_get_proxy(req, proxy_id, db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    
    #[tokio::test]
    async fn test_consumer_routes() {
        let (shared_config, db_client) = create_test_env().await;
        
        // Test POST /consumers
        let consumer_json = json!({
            "username": "testuser",
            "custom_id": "cust123"
        });
        
        let req = Request::builder()
            .method("POST")
            .uri("/consumers")
            .header("content-type", "application/json")
            .body(Body::from(consumer_json.to_string()))
            .unwrap();
        
        let resp = consumers::handle_create_consumer(req, db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        
        let json = response_to_json(resp).await.unwrap();
        let consumer_id = json["data"]["id"].as_str().unwrap().to_string();
        
        // Test GET /consumers
        let req = Request::builder()
            .method("GET")
            .uri("/consumers")
            .body(Body::empty())
            .unwrap();
        
        let resp = consumers::handle_get_consumers(req, db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
        
        // Test GET /consumers/{id}
        let req = Request::builder()
            .method("GET")
            .uri(format!("/consumers/{}", consumer_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = consumers::handle_get_consumer(req, consumer_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"]["username"], "testuser");
        
        // Test PUT /consumers/{id}
        let update_json = json!({
            "username": "updateduser",
            "custom_id": "cust456"
        });
        
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/consumers/{}", consumer_id))
            .header("content-type", "application/json")
            .body(Body::from(update_json.to_string()))
            .unwrap();
        
        let resp = consumers::handle_update_consumer(req, consumer_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        // Verify the update
        let req = Request::builder()
            .method("GET")
            .uri(format!("/consumers/{}", consumer_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = consumers::handle_get_consumer(req, consumer_id.clone(), db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"]["username"], "updateduser");
        
        // Test DELETE /consumers/{id}
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/consumers/{}", consumer_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = consumers::handle_delete_consumer(req, consumer_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        
        // Verify deletion
        let req = Request::builder()
            .method("GET")
            .uri(format!("/consumers/{}", consumer_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = consumers::handle_get_consumer(req, consumer_id, db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    
    #[tokio::test]
    async fn test_consumer_credential_routes() {
        let (shared_config, db_client) = create_test_env().await;
        
        // First create a consumer
        let consumer_json = json!({
            "username": "testuser"
        });
        
        let req = Request::builder()
            .method("POST")
            .uri("/consumers")
            .header("content-type", "application/json")
            .body(Body::from(consumer_json.to_string()))
            .unwrap();
        
        let resp = consumers::handle_create_consumer(req, db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        let consumer_id = json["data"]["id"].as_str().unwrap().to_string();
        
        // Test PUT /consumers/{id}/credentials/keyauth
        let cred_json = json!({
            "key": "test-api-key"
        });
        
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/consumers/{}/credentials/keyauth", consumer_id))
            .header("content-type", "application/json")
            .body(Body::from(cred_json.to_string()))
            .unwrap();
        
        let resp = consumers::handle_update_consumer_credentials(
            req, consumer_id.clone(), "keyauth".to_string(), db_client.clone()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        // Verify credentials were set
        let req = Request::builder()
            .method("GET")
            .uri(format!("/consumers/{}", consumer_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = consumers::handle_get_consumer(req, consumer_id.clone(), db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        assert!(json["data"]["credentials"]["keyauth"].is_object());
        
        // Test DELETE /consumers/{id}/credentials/keyauth
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/consumers/{}/credentials/keyauth", consumer_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = consumers::handle_delete_consumer_credentials(
            req, consumer_id.clone(), "keyauth".to_string(), db_client.clone()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        
        // Verify credentials were removed
        let req = Request::builder()
            .method("GET")
            .uri(format!("/consumers/{}", consumer_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = consumers::handle_get_consumer(req, consumer_id, db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        assert!(!json["data"]["credentials"].as_object().unwrap().contains_key("keyauth"));
    }
    
    #[tokio::test]
    async fn test_plugin_routes() {
        let (shared_config, db_client) = create_test_env().await;
        
        // First create a proxy
        let proxy_json = json!({
            "name": "Test API",
            "listen_path": "/api/test",
            "backend_protocol": "http",
            "backend_host": "example.com",
            "backend_port": 80
        });
        
        let req = Request::builder()
            .method("POST")
            .uri("/proxies")
            .header("content-type", "application/json")
            .body(Body::from(proxy_json.to_string()))
            .unwrap();
        
        let resp = proxies::handle_create_proxy(req, db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        let proxy_id = json["data"]["id"].as_str().unwrap().to_string();
        
        // Test GET /plugins
        let req = Request::builder()
            .method("GET")
            .uri("/plugins")
            .body(Body::empty())
            .unwrap();
        
        let resp = plugins::handle_get_plugins(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        let json = response_to_json(resp).await.unwrap();
        assert!(json["data"].as_array().unwrap().len() > 0);
        
        // Test POST /plugins/config
        let plugin_json = json!({
            "plugin_name": "key_auth",
            "config": {
                "key_location": "header",
                "header_name": "X-API-Key"
            },
            "scope": "proxy",
            "proxy_id": proxy_id,
            "enabled": true
        });
        
        let req = Request::builder()
            .method("POST")
            .uri("/plugins/config")
            .header("content-type", "application/json")
            .body(Body::from(plugin_json.to_string()))
            .unwrap();
        
        let resp = plugins::handle_create_plugin_config(req, db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        
        let json = response_to_json(resp).await.unwrap();
        let config_id = json["data"]["id"].as_str().unwrap().to_string();
        
        // Test GET /plugins/config
        let req = Request::builder()
            .method("GET")
            .uri("/plugins/config")
            .body(Body::empty())
            .unwrap();
        
        let resp = plugins::handle_get_plugin_configs(req, db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
        
        // Test GET /plugins/config/{id}
        let req = Request::builder()
            .method("GET")
            .uri(format!("/plugins/config/{}", config_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = plugins::handle_get_plugin_config(req, config_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"]["plugin_name"], "key_auth");
        
        // Test PUT /plugins/config/{id}
        let update_json = json!({
            "plugin_name": "key_auth",
            "config": {
                "key_location": "query",
                "query_param": "api_key"
            },
            "scope": "proxy",
            "proxy_id": proxy_id,
            "enabled": false
        });
        
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/plugins/config/{}", config_id))
            .header("content-type", "application/json")
            .body(Body::from(update_json.to_string()))
            .unwrap();
        
        let resp = plugins::handle_update_plugin_config(req, config_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        // Verify the update
        let req = Request::builder()
            .method("GET")
            .uri(format!("/plugins/config/{}", config_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = plugins::handle_get_plugin_config(req, config_id.clone(), db_client.clone()).await.unwrap();
        let json = response_to_json(resp).await.unwrap();
        assert_eq!(json["data"]["enabled"], false);
        assert_eq!(json["data"]["config"]["key_location"], "query");
        
        // Test DELETE /plugins/config/{id}
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/plugins/config/{}", config_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = plugins::handle_delete_plugin_config(req, config_id.clone(), db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        
        // Verify deletion
        let req = Request::builder()
            .method("GET")
            .uri(format!("/plugins/config/{}", config_id))
            .body(Body::empty())
            .unwrap();
        
        let resp = plugins::handle_get_plugin_config(req, config_id, db_client.clone()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    
    #[tokio::test]
    async fn test_metrics_endpoint() {
        let (shared_config, db_client) = create_test_env().await;
        
        // Add some test data to the configuration
        {
            let mut config = shared_config.write().await;
            config.proxies.push(Proxy {
                id: "proxy1".to_string(),
                name: Some("Test Proxy".to_string()),
                listen_path: "/api/test".to_string(),
                backend_protocol: Protocol::Http,
                backend_host: "example.com".to_string(),
                backend_port: 80,
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
            });
            
            config.consumers.push(Consumer {
                id: "consumer1".to_string(),
                username: "testuser".to_string(),
                custom_id: None,
                credentials: HashMap::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        }
        
        // Create and initialize a metrics collector
        let metrics_collector = Arc::new(ferrumgw::metrics::MetricsCollector::new());
        metrics_collector.record_status_code(200);
        metrics_collector.record_status_code(200);
        metrics_collector.record_status_code(404);
        
        // Test GET /admin/metrics
        let req = Request::builder()
            .method("GET")
            .uri("/admin/metrics")
            .body(Body::empty())
            .unwrap();
        
        let resp = metrics::handle_get_metrics(
            req, shared_config.clone(), metrics_collector, "memory"
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        let json = response_to_json(resp).await.unwrap();
        
        // Basic assertions about the metrics
        assert_eq!(json["mode"], "memory");
        assert_eq!(json["proxy_count"], 1);
        assert_eq!(json["consumer_count"], 1);
        
        // Check the status codes counts
        assert!(json["status_codes_last_second"].is_object());
        let status_codes = json["status_codes_last_second"].as_object().unwrap();
        assert_eq!(status_codes["200"], 2);
        assert_eq!(status_codes["404"], 1);
    }
}
