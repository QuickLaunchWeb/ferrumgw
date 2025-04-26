-- PostgreSQL migration for Ferrum Gateway
-- Initial schema creation

-- Proxies table
CREATE TABLE IF NOT EXISTS proxies (
    id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255),
    listen_path VARCHAR(255) NOT NULL UNIQUE,
    backend_protocol VARCHAR(10) NOT NULL CHECK (backend_protocol IN ('http', 'https', 'ws', 'wss', 'grpc')),
    backend_host VARCHAR(255) NOT NULL,
    backend_port INTEGER NOT NULL CHECK (backend_port > 0 AND backend_port < 65536),
    backend_path VARCHAR(255),
    strip_listen_path BOOLEAN NOT NULL DEFAULT TRUE,
    preserve_host_header BOOLEAN NOT NULL DEFAULT FALSE,
    backend_connect_timeout_ms BIGINT NOT NULL,
    backend_read_timeout_ms BIGINT NOT NULL,
    backend_write_timeout_ms BIGINT NOT NULL,
    backend_tls_client_cert_path VARCHAR(255),
    backend_tls_client_key_path VARCHAR(255),
    backend_tls_verify_server_cert BOOLEAN NOT NULL DEFAULT TRUE,
    backend_tls_server_ca_cert_path VARCHAR(255),
    dns_override VARCHAR(45),
    dns_cache_ttl_seconds BIGINT,
    auth_mode VARCHAR(10) NOT NULL DEFAULT 'single' CHECK (auth_mode IN ('single', 'multi')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_proxies_listen_path ON proxies(listen_path);

-- Consumers table
CREATE TABLE IF NOT EXISTS consumers (
    id VARCHAR(64) PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    custom_id VARCHAR(255),
    credentials JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_consumers_username ON consumers(username);
CREATE INDEX IF NOT EXISTS idx_consumers_custom_id ON consumers(custom_id);

-- Plugin configurations table
CREATE TABLE IF NOT EXISTS plugin_configs (
    id VARCHAR(64) PRIMARY KEY,
    plugin_name VARCHAR(64) NOT NULL,
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    scope VARCHAR(20) NOT NULL CHECK (scope IN ('global', 'proxy', 'consumer')),
    proxy_id VARCHAR(64) REFERENCES proxies(id) ON DELETE CASCADE,
    consumer_id VARCHAR(64) REFERENCES consumers(id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Enforce that at least one of proxy_id or consumer_id is set when scope isn't global
    CONSTRAINT check_scope_references CHECK (
        (scope = 'global') OR
        (scope = 'proxy' AND proxy_id IS NOT NULL) OR
        (scope = 'consumer' AND consumer_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_plugin_configs_plugin_name ON plugin_configs(plugin_name);
CREATE INDEX IF NOT EXISTS idx_plugin_configs_proxy_id ON plugin_configs(proxy_id);
CREATE INDEX IF NOT EXISTS idx_plugin_configs_consumer_id ON plugin_configs(consumer_id);

-- Association table for proxies and plugin configurations
CREATE TABLE IF NOT EXISTS proxy_plugin_associations (
    proxy_id VARCHAR(64) NOT NULL REFERENCES proxies(id) ON DELETE CASCADE,
    plugin_config_id VARCHAR(64) NOT NULL REFERENCES plugin_configs(id) ON DELETE CASCADE,
    PRIMARY KEY (proxy_id, plugin_config_id)
);

-- Function to update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Triggers to automatically update the updated_at column
CREATE TRIGGER update_proxies_updated_at
BEFORE UPDATE ON proxies
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_consumers_updated_at
BEFORE UPDATE ON consumers
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_plugin_configs_updated_at
BEFORE UPDATE ON plugin_configs
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
