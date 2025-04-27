use std::env;
use std::collections::HashMap;
use std::time::Duration;
use std::net::SocketAddr;
use serde_json;
use thiserror::Error;

use super::data_model::DatabaseType;
use crate::modes::OperationMode;

#[derive(Error, Debug)]
pub enum EnvConfigError {
    #[error("Missing required environment variable: {0}")]
    MissingEnv(String),
    
    #[error("Invalid environment variable value for {0}: {1}")]
    InvalidEnvValue(String, String),
    
    #[error("Failed to parse DNS overrides: {0}")]
    DnsOverridesParseError(String),
}

#[derive(Debug, Clone)]
pub struct EnvConfig {
    // Core settings
    pub mode: OperationMode,
    pub log_level: String,
    
    // Network ports & TLS settings
    pub proxy_http_port: Option<u16>,
    pub proxy_https_port: Option<u16>,
    pub proxy_http3_port: Option<u16>,
    pub proxy_tls_cert_path: Option<String>,
    pub proxy_tls_key_path: Option<String>,
    
    pub admin_http_port: Option<u16>,
    pub admin_https_port: Option<u16>,
    pub admin_http3_port: Option<u16>,
    pub admin_tls_cert_path: Option<String>,
    pub admin_tls_key_path: Option<String>,
    
    // Security settings
    pub admin_jwt_secret: Option<String>,
    pub cp_grpc_jwt_secret: Option<String>,
    pub dp_grpc_auth_token: Option<String>,
    
    // Database settings
    pub db_type: Option<DatabaseType>,
    pub db_url: Option<String>,
    pub db_poll_interval: Duration,
    pub db_incremental_polling: bool,
    pub db_poll_check_interval: Duration,
    
    // File mode settings
    pub file_config_path: Option<String>,
    
    // CP/DP communication
    pub cp_grpc_listen_addr: Option<SocketAddr>,
    pub dp_cp_grpc_url: Option<String>,
    
    // Request handling limits
    pub max_header_size_bytes: usize,
    pub max_body_size_bytes: usize,
    
    // DNS caching
    pub dns_cache_ttl_seconds: u64,
    pub dns_overrides: HashMap<String, String>,
    
    // Pagination settings
    pub default_pagination_limit: usize,
}

impl EnvConfig {
    pub fn from_env() -> Result<Self, EnvConfigError> {
        // Parse mode (required)
        let mode_str = env::var("FERRUM_MODE")
            .map_err(|_| EnvConfigError::MissingEnv("FERRUM_MODE".to_string()))?;
        
        let mode = match mode_str.as_str() {
            "database" => OperationMode::Database,
            "file" => OperationMode::File,
            "cp" => OperationMode::ControlPlane,
            "dp" => OperationMode::DataPlane,
            _ => return Err(EnvConfigError::InvalidEnvValue(
                "FERRUM_MODE".to_string(), 
                format!("Expected one of: database, file, cp, dp. Got: {}", mode_str)
            )),
        };
        
        // Parse log level (with default)
        let log_level = env::var("FERRUM_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
        
        // Network ports (with defaults)
        let proxy_http_port = Self::parse_optional_port("FERRUM_PROXY_HTTP_PORT", Some(8000))?;
        let proxy_https_port = Self::parse_optional_port("FERRUM_PROXY_HTTPS_PORT", Some(8443))?;
        let proxy_http3_port = Self::parse_optional_port("FERRUM_PROXY_HTTP3_PORT", Some(8444))?;
        let admin_http_port = Self::parse_optional_port("FERRUM_ADMIN_HTTP_PORT", Some(9000))?;
        let admin_https_port = Self::parse_optional_port("FERRUM_ADMIN_HTTPS_PORT", Some(9443))?;
        let admin_http3_port = Self::parse_optional_port("FERRUM_ADMIN_HTTP3_PORT", Some(9444))?;
        
        // TLS paths
        let proxy_tls_cert_path = env::var("FERRUM_PROXY_TLS_CERT_PATH").ok();
        let proxy_tls_key_path = env::var("FERRUM_PROXY_TLS_KEY_PATH").ok();
        let admin_tls_cert_path = env::var("FERRUM_ADMIN_TLS_CERT_PATH").ok();
        let admin_tls_key_path = env::var("FERRUM_ADMIN_TLS_KEY_PATH").ok();
        
        // JWT secrets
        let admin_jwt_secret = env::var("FERRUM_ADMIN_JWT_SECRET").ok();
        let cp_grpc_jwt_secret = env::var("FERRUM_CP_GRPC_JWT_SECRET").ok();
        let dp_grpc_auth_token = env::var("FERRUM_DP_GRPC_AUTH_TOKEN").ok();
        
        // Validate mode-specific configurations
        let mut config = EnvConfig {
            mode,
            log_level,
            proxy_http_port,
            proxy_https_port,
            proxy_http3_port,
            proxy_tls_cert_path,
            proxy_tls_key_path,
            admin_http_port,
            admin_https_port,
            admin_http3_port,
            admin_tls_cert_path,
            admin_tls_key_path,
            admin_jwt_secret,
            cp_grpc_jwt_secret,
            dp_grpc_auth_token,
            db_type: None,
            db_url: None,
            db_poll_interval: Duration::from_secs(30),
            db_incremental_polling: true,
            db_poll_check_interval: Duration::from_secs(5),
            file_config_path: None,
            cp_grpc_listen_addr: None,
            dp_cp_grpc_url: None,
            max_header_size_bytes: 16384,
            max_body_size_bytes: 10485760,
            dns_cache_ttl_seconds: 300,
            dns_overrides: HashMap::new(),
            default_pagination_limit: 500,
        };
        
        match config.mode {
            OperationMode::Database => {
                // For database mode, we need database connection info
                if config.db_type.is_none() {
                    return Err(anyhow!("FERRUM_DB_TYPE is required for database mode"));
                }
                if config.db_url.is_none() {
                    return Err(anyhow!("FERRUM_DB_URL is required for database mode"));
                }
                // Admin JWT secret is required for admin API
                if config.admin_jwt_secret.is_none() {
                    return Err(anyhow!("FERRUM_ADMIN_JWT_SECRET is required for database mode"));
                }
            }
            OperationMode::File => {
                // For file mode, we need the file config path
                if config.file_config_path.is_none() {
                    return Err(anyhow!("FERRUM_FILE_CONFIG_PATH is required for file mode"));
                }
            }
            OperationMode::ControlPlane => {
                // For CP mode, we need database connection info and gRPC config
                if config.db_type.is_none() {
                    return Err(anyhow!("FERRUM_DB_TYPE is required for control plane mode"));
                }
                if config.db_url.is_none() {
                    return Err(anyhow!("FERRUM_DB_URL is required for control plane mode"));
                }
                if config.cp_grpc_listen_addr.is_none() {
                    return Err(anyhow!("FERRUM_CP_GRPC_LISTEN_ADDR is required for control plane mode"));
                }
                if config.cp_grpc_jwt_secret.is_none() {
                    return Err(anyhow!("FERRUM_CP_GRPC_JWT_SECRET is required for control plane mode"));
                }
                // Admin JWT secret is required for admin API
                if config.admin_jwt_secret.is_none() {
                    return Err(anyhow!("FERRUM_ADMIN_JWT_SECRET is required for control plane mode"));
                }
            }
            OperationMode::DataPlane => {
                // For DP mode, we need the CP gRPC URL and auth token
                if config.dp_cp_grpc_url.is_none() {
                    return Err(anyhow!("FERRUM_DP_CP_GRPC_URL is required for data plane mode"));
                }
                if config.dp_grpc_auth_token.is_none() {
                    return Err(anyhow!("FERRUM_DP_GRPC_AUTH_TOKEN is required for data plane mode"));
                }
            }
        }
        
        // Database settings
        let db_poll_interval = Self::parse_duration_with_default("FERRUM_DB_POLL_INTERVAL", 30)?;
        let db_incremental_polling = env::var("FERRUM_DB_INCREMENTAL_POLLING")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(true); // Default to true for better performance
        let db_poll_check_interval = Self::parse_duration_with_default("FERRUM_DB_POLL_CHECK_INTERVAL", 5)?;
        
        config.db_poll_interval = db_poll_interval;
        config.db_incremental_polling = db_incremental_polling;
        config.db_poll_check_interval = db_poll_check_interval;
        
        let (db_type, db_url) = match config.mode {
            OperationMode::Database | OperationMode::ControlPlane => {
                let db_type_str = env::var("FERRUM_DB_TYPE")
                    .map_err(|_| EnvConfigError::MissingEnv("FERRUM_DB_TYPE".to_string()))?;
                
                let db_type = match db_type_str.as_str() {
                    "postgres" => DatabaseType::Postgres,
                    "mysql" => DatabaseType::MySQL,
                    "sqlite" => DatabaseType::SQLite,
                    _ => return Err(EnvConfigError::InvalidEnvValue(
                        "FERRUM_DB_TYPE".to_string(), 
                        format!("Expected one of: postgres, mysql, sqlite. Got: {}", db_type_str)
                    )),
                };
                
                let db_url = env::var("FERRUM_DB_URL")
                    .map_err(|_| EnvConfigError::MissingEnv("FERRUM_DB_URL".to_string()))?;
                
                (Some(db_type), Some(db_url))
            },
            _ => (None, None)
        };
        
        config.db_type = db_type;
        config.db_url = db_url;
        
        // File mode settings
        config.file_config_path = match config.mode {
            OperationMode::File => {
                let path = env::var("FERRUM_FILE_CONFIG_PATH")
                    .map_err(|_| EnvConfigError::MissingEnv("FERRUM_FILE_CONFIG_PATH".to_string()))?;
                Some(path)
            },
            _ => None
        };
        
        // CP/DP communication
        config.cp_grpc_listen_addr = match config.mode {
            OperationMode::ControlPlane => {
                let addr_str = env::var("FERRUM_CP_GRPC_LISTEN_ADDR")
                    .map_err(|_| EnvConfigError::MissingEnv("FERRUM_CP_GRPC_LISTEN_ADDR".to_string()))?;
                
                let addr = addr_str.parse::<SocketAddr>()
                    .map_err(|_| EnvConfigError::InvalidEnvValue(
                        "FERRUM_CP_GRPC_LISTEN_ADDR".to_string(),
                        format!("Invalid socket address: {}", addr_str)
                    ))?;
                
                Some(addr)
            },
            _ => None
        };
        
        config.dp_cp_grpc_url = match config.mode {
            OperationMode::DataPlane => {
                let url = env::var("FERRUM_DP_CP_GRPC_URL")
                    .map_err(|_| EnvConfigError::MissingEnv("FERRUM_DP_CP_GRPC_URL".to_string()))?;
                Some(url)
            },
            _ => None
        };
        
        // Request handling limits
        config.max_header_size_bytes = Self::parse_usize_with_default(
            "FERRUM_MAX_HEADER_SIZE_BYTES", 
            16384
        )?;
        
        config.max_body_size_bytes = Self::parse_usize_with_default(
            "FERRUM_MAX_BODY_SIZE_BYTES", 
            10485760
        )?;
        
        // DNS caching
        config.dns_cache_ttl_seconds = Self::parse_u64_with_default(
            "FERRUM_DNS_CACHE_TTL_SECONDS", 
            300
        )?;
        
        // DNS overrides
        config.dns_overrides = match env::var("FERRUM_DNS_OVERRIDES") {
            Ok(json_str) => {
                serde_json::from_str::<HashMap<String, String>>(&json_str)
                    .map_err(|e| EnvConfigError::DnsOverridesParseError(e.to_string()))?
            },
            Err(_) => HashMap::new()
        };
        
        // Pagination settings
        config.default_pagination_limit = Self::parse_usize_with_default(
            "FERRUM_DEFAULT_PAGINATION_LIMIT", 
            500
        )?;
        
        Ok(config)
    }
    
    fn parse_optional_port(var_name: &str, default: Option<u16>) -> Result<Option<u16>, EnvConfigError> {
        match env::var(var_name) {
            Ok(val) => {
                let port = val.parse::<u16>()
                    .map_err(|_| EnvConfigError::InvalidEnvValue(
                        var_name.to_string(),
                        format!("Expected a valid port number (0-65535). Got: {}", val)
                    ))?;
                Ok(Some(port))
            },
            Err(_) => Ok(default)
        }
    }
    
    fn parse_duration_with_default(var_name: &str, default_secs: u64) -> Result<Duration, EnvConfigError> {
        match env::var(var_name) {
            Ok(val) => {
                let secs = val.parse::<u64>()
                    .map_err(|_| EnvConfigError::InvalidEnvValue(
                        var_name.to_string(),
                        format!("Expected a positive integer. Got: {}", val)
                    ))?;
                Ok(Duration::from_secs(secs))
            },
            Err(_) => Ok(Duration::from_secs(default_secs))
        }
    }
    
    fn parse_usize_with_default(var_name: &str, default: usize) -> Result<usize, EnvConfigError> {
        match env::var(var_name) {
            Ok(val) => {
                let num = val.parse::<usize>()
                    .map_err(|_| EnvConfigError::InvalidEnvValue(
                        var_name.to_string(),
                        format!("Expected a positive integer. Got: {}", val)
                    ))?;
                Ok(num)
            },
            Err(_) => Ok(default)
        }
    }
    
    fn parse_u64_with_default(var_name: &str, default: u64) -> Result<u64, EnvConfigError> {
        match env::var(var_name) {
            Ok(val) => {
                let num = val.parse::<u64>()
                    .map_err(|_| EnvConfigError::InvalidEnvValue(
                        var_name.to_string(),
                        format!("Expected a positive integer. Got: {}", val)
                    ))?;
                Ok(num)
            },
            Err(_) => Ok(default)
        }
    }
}
