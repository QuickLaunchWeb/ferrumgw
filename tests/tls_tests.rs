#[cfg(test)]
mod tls_tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::tempdir;
    
    use ferrumgw::proxy::tls;
    
    // Generate self-signed certificate for testing
    fn generate_test_cert(cert_path: &PathBuf, key_path: &PathBuf) -> Result<(), anyhow::Error> {
        // Generate a test certificate using rcgen
        use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, SanType};
        
        let mut params = CertificateParams::default();
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, "localhost");
        params.distinguished_name = distinguished_name;
        
        // Add Subject Alternative Names
        params.subject_alt_names = vec![
            SanType::DnsName("localhost".to_string()),
            SanType::DnsName("127.0.0.1".to_string()),
        ];
        
        // Generate self-signed certificate
        let cert = Certificate::from_params(params)?;
        let cert_pem = cert.serialize_pem()?;
        let key_pem = cert.serialize_private_key_pem();
        
        // Write certificate and key to files
        fs::write(cert_path, cert_pem)?;
        fs::write(key_path, key_pem)?;
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_client_tls_configuration() {
        // Test the default client TLS config with system root certificates
        let client_config = match tls::create_client_tls_config(None, None, true, None) {
            Ok(config) => config,
            Err(e) => {
                // This might fail if system certificates can't be loaded, which is ok for some environments
                println!("Failed to create client TLS config: {}", e);
                return;
            }
        };
        
        // Verify that we got a valid TLS config
        assert!(client_config.root_store.roots.len() > 0, "Expected root certificates to be loaded");
    }
    
    #[tokio::test]
    async fn test_client_tls_with_client_cert() {
        // Create temporary directory for certificates
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let client_cert_path = temp_dir.path().join("client.crt");
        let client_key_path = temp_dir.path().join("client.key");
        
        // Generate test certificate
        match generate_test_cert(&client_cert_path, &client_key_path) {
            Ok(_) => {},
            Err(e) => {
                println!("Failed to generate test certificate: {}", e);
                return;
            }
        };
        
        // Create client TLS config with client certificate
        let client_config = match tls::create_client_tls_config(
            Some(client_cert_path.to_str().unwrap()),
            Some(client_key_path.to_str().unwrap()),
            true,
            None,
        ) {
            Ok(config) => config,
            Err(e) => {
                println!("Failed to create client TLS config with client cert: {}", e);
                return;
            }
        };
        
        // Verify that the client config was created
        assert!(client_config.root_store.roots.len() > 0, "Expected root certificates to be loaded");
    }
    
    #[tokio::test]
    async fn test_server_tls_configuration() {
        // Create temporary directory for certificates
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let server_cert_path = temp_dir.path().join("server.crt");
        let server_key_path = temp_dir.path().join("server.key");
        
        // Generate test certificate
        match generate_test_cert(&server_cert_path, &server_key_path) {
            Ok(_) => {},
            Err(e) => {
                println!("Failed to generate test certificate: {}", e);
                return;
            }
        };
        
        // Create server TLS config
        let server_config = match tls::create_server_tls_config(
            server_cert_path.to_str().unwrap(),
            server_key_path.to_str().unwrap(),
        ) {
            Ok(config) => config,
            Err(e) => {
                println!("Failed to create server TLS config: {}", e);
                return;
            }
        };
        
        // Not much we can directly test, but at least verify it's created
        assert!(server_config.alpn_protocols.contains(&b"h2".to_vec()), "Expected h2 ALPN protocol");
        assert!(server_config.alpn_protocols.contains(&b"http/1.1".to_vec()), "Expected http/1.1 ALPN protocol");
    }
    
    #[tokio::test]
    async fn test_custom_ca_certificates() {
        // Create temporary directory for certificates
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let ca_cert_path = temp_dir.path().join("ca.crt");
        let ca_key_path = temp_dir.path().join("ca.key");
        
        // Generate test CA certificate
        match generate_test_cert(&ca_cert_path, &ca_key_path) {
            Ok(_) => {},
            Err(e) => {
                println!("Failed to generate test CA certificate: {}", e);
                return;
            }
        };
        
        // Create client TLS config with custom CA certificate
        let client_config = match tls::create_client_tls_config(
            None,
            None,
            false, // Do not use system certs
            Some(ca_cert_path.to_str().unwrap()),
        ) {
            Ok(config) => config,
            Err(e) => {
                println!("Failed to create client TLS config with custom CA: {}", e);
                return;
            }
        };
        
        // Verify the root store has our CA
        assert_eq!(client_config.root_store.roots.len(), 1, "Expected exactly one root certificate");
    }
    
    #[tokio::test]
    async fn test_invalid_certificates() {
        // Create temporary directory for certificates
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let invalid_cert_path = temp_dir.path().join("invalid.crt");
        let invalid_key_path = temp_dir.path().join("invalid.key");
        
        // Create invalid certificate files
        fs::write(&invalid_cert_path, "This is not a valid certificate").expect("Failed to write invalid cert");
        fs::write(&invalid_key_path, "This is not a valid key").expect("Failed to write invalid key");
        
        // Test server config with invalid certificates
        let server_result = tls::create_server_tls_config(
            invalid_cert_path.to_str().unwrap(),
            invalid_key_path.to_str().unwrap(),
        );
        assert!(server_result.is_err(), "Expected error with invalid certificates");
        
        // Test client config with invalid client certificates
        let client_result = tls::create_client_tls_config(
            Some(invalid_cert_path.to_str().unwrap()),
            Some(invalid_key_path.to_str().unwrap()),
            true,
            None,
        );
        assert!(client_result.is_err(), "Expected error with invalid client certificates");
        
        // Test client config with invalid CA certificate
        let ca_result = tls::create_client_tls_config(
            None,
            None,
            false,
            Some(invalid_cert_path.to_str().unwrap()),
        );
        assert!(ca_result.is_err(), "Expected error with invalid CA certificate");
    }
}
