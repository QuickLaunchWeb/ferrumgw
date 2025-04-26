#[cfg(test)]
mod simple_tests {
    use ferrumgw::config::data_model::{Proxy, Protocol, AuthMode};
    use std::collections::HashMap;
    use chrono::Utc;

    #[test]
    fn test_proxy_creation() {
        let proxy = Proxy {
            id: "test1".to_string(),
            name: Some("Test Proxy".to_string()),
            listen_path: "/api/test".to_string(),
            backend_protocol: Protocol::Http,
            backend_host: "example.com".to_string(),
            backend_port: 80,
            backend_path: Some("/backend".to_string()),
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
        };

        assert_eq!(proxy.id, "test1");
        assert_eq!(proxy.listen_path, "/api/test");
        assert_eq!(proxy.backend_port, 80);
        assert_eq!(proxy.backend_protocol, Protocol::Http);
        assert_eq!(proxy.auth_mode, AuthMode::Single);
        println!("Simple proxy test passed!");
    }
}
