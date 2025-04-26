-- MySQL migration for Ferrum Gateway
-- Initial schema creation

-- Proxies table
CREATE TABLE IF NOT EXISTS proxies (
    id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255),
    listen_path VARCHAR(255) NOT NULL,
    backend_protocol VARCHAR(10) NOT NULL,
    backend_host VARCHAR(255) NOT NULL,
    backend_port INT UNSIGNED NOT NULL,
    backend_path VARCHAR(255),
    strip_listen_path BOOLEAN NOT NULL DEFAULT TRUE,
    preserve_host_header BOOLEAN NOT NULL DEFAULT FALSE,
    backend_connect_timeout_ms BIGINT UNSIGNED NOT NULL,
    backend_read_timeout_ms BIGINT UNSIGNED NOT NULL,
    backend_write_timeout_ms BIGINT UNSIGNED NOT NULL,
    backend_tls_client_cert_path VARCHAR(255),
    backend_tls_client_key_path VARCHAR(255),
    backend_tls_verify_server_cert BOOLEAN NOT NULL DEFAULT TRUE,
    backend_tls_server_ca_cert_path VARCHAR(255),
    dns_override VARCHAR(45),
    dns_cache_ttl_seconds BIGINT UNSIGNED,
    auth_mode VARCHAR(10) NOT NULL DEFAULT 'single',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY (listen_path),
    CONSTRAINT chk_backend_protocol CHECK (backend_protocol IN ('http', 'https', 'ws', 'wss', 'grpc')),
    CONSTRAINT chk_auth_mode CHECK (auth_mode IN ('single', 'multi'))
);

CREATE INDEX idx_proxies_listen_path ON proxies(listen_path);

-- Consumers table
CREATE TABLE IF NOT EXISTS consumers (
    id VARCHAR(64) PRIMARY KEY,
    username VARCHAR(255) NOT NULL,
    custom_id VARCHAR(255),
    credentials JSON,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY (username)
);

CREATE INDEX idx_consumers_username ON consumers(username);
CREATE INDEX idx_consumers_custom_id ON consumers(custom_id);

-- Plugin configurations table
CREATE TABLE IF NOT EXISTS plugin_configs (
    id VARCHAR(64) PRIMARY KEY,
    plugin_name VARCHAR(64) NOT NULL,
    config JSON NOT NULL,
    scope VARCHAR(20) NOT NULL,
    proxy_id VARCHAR(64),
    consumer_id VARCHAR(64),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    CONSTRAINT chk_scope CHECK (scope IN ('global', 'proxy', 'consumer')),
    CONSTRAINT fk_plugin_configs_proxy FOREIGN KEY (proxy_id) REFERENCES proxies(id) ON DELETE CASCADE,
    CONSTRAINT fk_plugin_configs_consumer FOREIGN KEY (consumer_id) REFERENCES consumers(id) ON DELETE CASCADE,
    -- Enforce that at least one of proxy_id or consumer_id is set when scope isn't global
    CONSTRAINT chk_scope_references CHECK (
        (scope = 'global') OR
        (scope = 'proxy' AND proxy_id IS NOT NULL) OR
        (scope = 'consumer' AND consumer_id IS NOT NULL)
    )
);

CREATE INDEX idx_plugin_configs_plugin_name ON plugin_configs(plugin_name);
CREATE INDEX idx_plugin_configs_proxy_id ON plugin_configs(proxy_id);
CREATE INDEX idx_plugin_configs_consumer_id ON plugin_configs(consumer_id);

-- Association table for proxies and plugin configurations
CREATE TABLE IF NOT EXISTS proxy_plugin_associations (
    proxy_id VARCHAR(64) NOT NULL,
    plugin_config_id VARCHAR(64) NOT NULL,
    PRIMARY KEY (proxy_id, plugin_config_id),
    CONSTRAINT fk_associations_proxy FOREIGN KEY (proxy_id) REFERENCES proxies(id) ON DELETE CASCADE,
    CONSTRAINT fk_associations_plugin_config FOREIGN KEY (plugin_config_id) REFERENCES plugin_configs(id) ON DELETE CASCADE
);
