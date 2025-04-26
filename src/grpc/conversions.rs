use std::convert::TryFrom;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::config::data_model::{
    Proxy, Consumer, PluginConfig, 
    Protocol, AuthMode, Configuration
};
use super::proto::{
    Proxy as ProtoProxy, 
    Consumer as ProtoConsumer, 
    PluginConfig as ProtoPluginConfig,
    ProxyProtocol, AuthMode as ProtoAuthMode
};

/// Conversion from protobuf Proxy to domain Proxy
impl TryFrom<&ProtoProxy> for Proxy {
    type Error = anyhow::Error;
    
    fn try_from(proto: &ProtoProxy) -> Result<Self, Self::Error> {
        // Convert the protocol enum
        let backend_protocol = match proto.backend_protocol() {
            ProtoProtocol::Http => Protocol::Http,
            ProtoProtocol::Https => Protocol::Https,
            ProtoProtocol::Ws => Protocol::Ws,
            ProtoProtocol::Wss => Protocol::Wss,
            ProtoProtocol::Grpc => Protocol::Grpc,
        };
        
        // Convert the auth mode enum
        let auth_mode = match proto.auth_mode() {
            ProtoAuthMode::Single => AuthMode::Single,
            ProtoAuthMode::Multi => AuthMode::Multi,
        };
        
        // Parse timestamps
        let created_at = if proto.created_at.is_empty() {
            Utc::now()
        } else {
            DateTime::parse_from_rfc3339(&proto.created_at)?
                .with_timezone(&Utc)
        };
        
        let updated_at = if proto.updated_at.is_empty() {
            Utc::now()
        } else {
            DateTime::parse_from_rfc3339(&proto.updated_at)?
                .with_timezone(&Utc)
        };
        
        let proxy = Proxy {
            id: proto.id.clone(),
            name: if proto.name.is_empty() { None } else { Some(proto.name.clone()) },
            listen_path: proto.listen_path.clone(),
            backend_protocol,
            backend_host: proto.backend_host.clone(),
            backend_port: proto.backend_port as u16,
            backend_path: if proto.backend_path.is_empty() { None } else { Some(proto.backend_path.clone()) },
            strip_listen_path: proto.strip_listen_path,
            preserve_host_header: proto.preserve_host_header,
            backend_connect_timeout_ms: proto.backend_connect_timeout_ms,
            backend_read_timeout_ms: proto.backend_read_timeout_ms,
            backend_write_timeout_ms: proto.backend_write_timeout_ms,
            backend_tls_client_cert_path: if proto.backend_tls_client_cert_path.is_empty() { None } else { Some(proto.backend_tls_client_cert_path.clone()) },
            backend_tls_client_key_path: if proto.backend_tls_client_key_path.is_empty() { None } else { Some(proto.backend_tls_client_key_path.clone()) },
            backend_tls_verify_server_cert: proto.backend_tls_verify_server_cert,
            backend_tls_server_ca_cert_path: if proto.backend_tls_server_ca_cert_path.is_empty() { None } else { Some(proto.backend_tls_server_ca_cert_path.clone()) },
            dns_override: if proto.dns_override.is_empty() { None } else { Some(proto.dns_override.clone()) },
            dns_cache_ttl_seconds: if proto.dns_cache_ttl_seconds == 0 { None } else { Some(proto.dns_cache_ttl_seconds) },
            auth_mode,
            plugins: Vec::new(), // Will be populated separately
            created_at,
            updated_at,
        };
        
        Ok(proxy)
    }
}

/// Conversion from domain Proxy to protobuf Proxy
impl From<&Proxy> for ProtoProxy {
    fn from(proxy: &Proxy) -> Self {
        // Convert the protocol enum
        let backend_protocol = match proxy.backend_protocol {
            Protocol::Http => ProtoProtocol::Http,
            Protocol::Https => ProtoProtocol::Https,
            Protocol::Ws => ProtoProtocol::Ws,
            Protocol::Wss => ProtoProtocol::Wss,
            Protocol::Grpc => ProtoProtocol::Grpc,
        } as i32;
        
        // Convert the auth mode enum
        let auth_mode = match proxy.auth_mode {
            AuthMode::Single => ProtoAuthMode::Single,
            AuthMode::Multi => ProtoAuthMode::Multi,
        } as i32;
        
        ProtoProxy {
            id: proxy.id.clone(),
            name: proxy.name.clone().unwrap_or_default(),
            listen_path: proxy.listen_path.clone(),
            backend_protocol,
            backend_host: proxy.backend_host.clone(),
            backend_port: proxy.backend_port as u32,
            backend_path: proxy.backend_path.clone().unwrap_or_default(),
            strip_listen_path: proxy.strip_listen_path,
            preserve_host_header: proxy.preserve_host_header,
            backend_connect_timeout_ms: proxy.backend_connect_timeout_ms,
            backend_read_timeout_ms: proxy.backend_read_timeout_ms,
            backend_write_timeout_ms: proxy.backend_write_timeout_ms,
            backend_tls_client_cert_path: proxy.backend_tls_client_cert_path.clone().unwrap_or_default(),
            backend_tls_client_key_path: proxy.backend_tls_client_key_path.clone().unwrap_or_default(),
            backend_tls_verify_server_cert: proxy.backend_tls_verify_server_cert,
            backend_tls_server_ca_cert_path: proxy.backend_tls_server_ca_cert_path.clone().unwrap_or_default(),
            dns_override: proxy.dns_override.clone().unwrap_or_default(),
            dns_cache_ttl_seconds: proxy.dns_cache_ttl_seconds.unwrap_or(0),
            auth_mode,
            created_at: proxy.created_at.to_rfc3339(),
            updated_at: proxy.updated_at.to_rfc3339(),
            plugin_ids: proxy.plugins.clone(),
        }
    }
}

/// Conversion from protobuf Consumer to domain Consumer
impl TryFrom<&ProtoConsumer> for Consumer {
    type Error = anyhow::Error;
    
    fn try_from(proto: &ProtoConsumer) -> Result<Self, Self::Error> {
        // Parse timestamps
        let created_at = if proto.created_at.is_empty() {
            Utc::now()
        } else {
            DateTime::parse_from_rfc3339(&proto.created_at)?
                .with_timezone(&Utc)
        };
        
        let updated_at = if proto.updated_at.is_empty() {
            Utc::now()
        } else {
            DateTime::parse_from_rfc3339(&proto.updated_at)?
                .with_timezone(&Utc)
        };
        
        // Parse credentials JSON
        let credentials = if proto.credentials.is_empty() {
            std::collections::HashMap::new()
        } else {
            serde_json::from_str(&proto.credentials)
                .map_err(|e| anyhow!("Failed to parse consumer credentials: {}", e))?
        };
        
        let consumer = Consumer {
            id: proto.id.clone(),
            username: proto.username.clone(),
            custom_id: if proto.custom_id.is_empty() { None } else { Some(proto.custom_id.clone()) },
            credentials,
            created_at,
            updated_at,
        };
        
        Ok(consumer)
    }
}

/// Conversion from domain Consumer to protobuf Consumer
impl From<&Consumer> for ProtoConsumer {
    fn from(consumer: &Consumer) -> Self {
        // Serialize credentials to JSON
        let credentials_json = serde_json::to_string(&consumer.credentials)
            .unwrap_or_else(|_| "{}".to_string());
        
        ProtoConsumer {
            id: consumer.id.clone(),
            username: consumer.username.clone(),
            custom_id: consumer.custom_id.clone().unwrap_or_default(),
            credentials: credentials_json,
            created_at: consumer.created_at.to_rfc3339(),
            updated_at: consumer.updated_at.to_rfc3339(),
        }
    }
}

/// Conversion from protobuf PluginConfig to domain PluginConfig
impl TryFrom<&ProtoPluginConfig> for PluginConfig {
    type Error = anyhow::Error;
    
    fn try_from(proto: &ProtoPluginConfig) -> Result<Self, Self::Error> {
        // Parse timestamps
        let created_at = if proto.created_at.is_empty() {
            Utc::now()
        } else {
            DateTime::parse_from_rfc3339(&proto.created_at)?
                .with_timezone(&Utc)
        };
        
        let updated_at = if proto.updated_at.is_empty() {
            Utc::now()
        } else {
            DateTime::parse_from_rfc3339(&proto.updated_at)?
                .with_timezone(&Utc)
        };
        
        // Parse config JSON
        let config: Value = if proto.config.is_empty() {
            json!({})
        } else {
            serde_json::from_str(&proto.config)
                .map_err(|e| anyhow!("Failed to parse plugin config: {}", e))?
        };
        
        let plugin_config = PluginConfig {
            id: proto.id.clone(),
            plugin_name: proto.plugin_name.clone(),
            config,
            scope: proto.scope.clone(),
            proxy_id: if proto.proxy_id.is_empty() { None } else { Some(proto.proxy_id.clone()) },
            consumer_id: if proto.consumer_id.is_empty() { None } else { Some(proto.consumer_id.clone()) },
            enabled: proto.enabled,
            created_at,
            updated_at,
        };
        
        Ok(plugin_config)
    }
}

/// Conversion from domain PluginConfig to protobuf PluginConfig
impl From<&PluginConfig> for ProtoPluginConfig {
    fn from(plugin_config: &PluginConfig) -> Self {
        // Serialize config to JSON
        let config_json = serde_json::to_string(&plugin_config.config)
            .unwrap_or_else(|_| "{}".to_string());
        
        ProtoPluginConfig {
            id: plugin_config.id.clone(),
            plugin_name: plugin_config.plugin_name.clone(),
            config: config_json,
            scope: plugin_config.scope.clone(),
            proxy_id: plugin_config.proxy_id.clone().unwrap_or_default(),
            consumer_id: plugin_config.consumer_id.clone().unwrap_or_default(),
            enabled: plugin_config.enabled,
            created_at: plugin_config.created_at.to_rfc3339(),
            updated_at: plugin_config.updated_at.to_rfc3339(),
        }
    }
}
