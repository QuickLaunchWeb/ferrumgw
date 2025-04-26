use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use anyhow::{Result, Context};
use tokio::net::TcpStream;
use tokio_rustls::rustls::{self, Certificate, PrivateKey, ServerConfig};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use tracing::{debug, error};
use rustls_native_certs;

/// Loads a server TLS configuration from certificate and key files
pub fn load_server_config(cert_path: &str, key_path: &str) -> Result<Arc<ServerConfig>> {
    debug!("Loading TLS certificate from {} and key from {}", cert_path, key_path);
    
    // Load and parse the certificate chain
    let cert_file = File::open(cert_path)
        .context(format!("Failed to open certificate file: {}", cert_path))?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain = rustls_pemfile::certs(&mut cert_reader)
        .context("Failed to parse certificate chain")?
        .into_iter()
        .map(Certificate)
        .collect();
    
    // Load and parse the private key
    let key_file = File::open(key_path)
        .context(format!("Failed to open key file: {}", key_path))?;
    let mut key_reader = BufReader::new(key_file);
    
    // Try PKCS8 format first, then RSA/EC if that fails
    let private_key = match rustls_pemfile::pkcs8_private_keys(&mut key_reader) {
        Ok(keys) if !keys.is_empty() => PrivateKey(keys[0].clone()),
        _ => {
            // Reset the reader
            let key_file = File::open(key_path)?;
            let mut key_reader = BufReader::new(key_file);
            
            // Try RSA key format
            match rustls_pemfile::rsa_private_keys(&mut key_reader) {
                Ok(keys) if !keys.is_empty() => PrivateKey(keys[0].clone()),
                _ => {
                    // Reset the reader again
                    let key_file = File::open(key_path)?;
                    let mut key_reader = BufReader::new(key_file);
                    
                    // Try EC key format as a last resort
                    match rustls_pemfile::ec_private_keys(&mut key_reader) {
                        Ok(keys) if !keys.is_empty() => PrivateKey(keys[0].clone()),
                        _ => anyhow::bail!("No supported private key found in {}", key_path),
                    }
                }
            }
        }
    };
    
    // Create a server config
    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .context("Failed to create TLS server config")?;
    
    Ok(Arc::new(config))
}

/// Accepts a TLS connection by performing the handshake
pub async fn accept_connection(
    tcp_stream: TcpStream,
    tls_config: Arc<ServerConfig>,
) -> Result<TlsStream<TcpStream>> {
    let acceptor = TlsAcceptor::from(tls_config);
    acceptor.accept(tcp_stream).await.context("TLS handshake failed")
}

/// Loads a client TLS configuration for connecting to backends with mTLS
pub fn load_client_config(
    client_cert_path: &str,
    client_key_path: &str,
    verify_server: bool,
    server_ca_path: Option<&str>,
) -> Result<Arc<rustls::ClientConfig>> {
    // Load and parse the client certificate
    let cert_file = File::open(client_cert_path)
        .context(format!("Failed to open client certificate file: {}", client_cert_path))?;
    let mut cert_reader = BufReader::new(cert_file);
    let client_certs = rustls_pemfile::certs(&mut cert_reader)
        .context("Failed to parse client certificate chain")?
        .into_iter()
        .map(Certificate)
        .collect();
    
    // Load and parse the client private key
    let key_file = File::open(client_key_path)
        .context(format!("Failed to open client key file: {}", client_key_path))?;
    let mut key_reader = BufReader::new(key_file);
    
    // Try PKCS8 format first, then RSA/EC if that fails
    let private_key = match rustls_pemfile::pkcs8_private_keys(&mut key_reader) {
        Ok(keys) if !keys.is_empty() => PrivateKey(keys[0].clone()),
        _ => {
            // Reset the reader
            let key_file = File::open(client_key_path)?;
            let mut key_reader = BufReader::new(key_file);
            
            // Try RSA key format
            match rustls_pemfile::rsa_private_keys(&mut key_reader) {
                Ok(keys) if !keys.is_empty() => PrivateKey(keys[0].clone()),
                _ => {
                    // Reset the reader again
                    let key_file = File::open(client_key_path)?;
                    let mut key_reader = BufReader::new(key_file);
                    
                    // Try EC key format as a last resort
                    match rustls_pemfile::ec_private_keys(&mut key_reader) {
                        Ok(keys) if !keys.is_empty() => PrivateKey(keys[0].clone()),
                        _ => anyhow::bail!("No supported private key found in {}", client_key_path),
                    }
                }
            }
        }
    };
    
    // Configure the certificate verifier
    let mut root_cert_store = rustls::RootCertStore::empty();
    
    if verify_server {
        if let Some(ca_path) = server_ca_path {
            // Load specific CA certificate
            let ca_file = File::open(ca_path)
                .context(format!("Failed to open server CA certificate file: {}", ca_path))?;
            let mut ca_reader = BufReader::new(ca_file);
            let ca_certs = rustls_pemfile::certs(&mut ca_reader)
                .context("Failed to parse server CA certificate")?;
            
            for cert in ca_certs {
                root_cert_store.add(&Certificate(cert))?;
            }
        } else {
            // Use system root certificates
            root_cert_store = rustls::RootCertStore::empty();
            let roots = rustls_native_certs::load_native_certs()
                .context("Failed to load native certificates")?;
            
            for cert in roots {
                root_cert_store.add(&rustls::Certificate(cert.0))
                    .map_err(|e| anyhow!("Failed to add certificate: {}", e))?;
            }
            
            debug!("Added {} native root certificates to the store", root_cert_store.len());
        }
    }
    
    // Create a client config
    let config = if verify_server {
        rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_cert_store)
            .with_client_auth_cert(client_certs, private_key)
            .context("Failed to create TLS client config")?
    } else {
        rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(Arc::new(NoVerifier {}))
            .with_client_auth_cert(client_certs, private_key)
            .context("Failed to create TLS client config")?
    };
    
    Ok(Arc::new(config))
}

// A certificate verifier that accepts any certificate
struct NoVerifier {}

impl rustls::client::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}
