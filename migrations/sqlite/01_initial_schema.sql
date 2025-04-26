-- SQLite migration for Ferrum Gateway
-- Initial schema creation

-- Enable foreign key support
PRAGMA foreign_keys = ON;

-- Proxies table
CREATE TABLE IF NOT EXISTS proxies (
    id TEXT PRIMARY KEY,
    name TEXT,
    listen_path TEXT NOT NULL UNIQUE,
    backend_protocol TEXT NOT NULL CHECK (backend_protocol IN ('http', 'https', 'ws', 'wss', 'grpc')),
    backend_host TEXT NOT NULL,
    backend_port INTEGER NOT NULL CHECK (backend_port > 0 AND backend_port < 65536),
    backend_path TEXT,
    strip_listen_path INTEGER NOT NULL DEFAULT 1, -- Boolean as INTEGER (1 = true, 0 = false)
    preserve_host_header INTEGER NOT NULL DEFAULT 0, -- Boolean as INTEGER
    backend_connect_timeout_ms INTEGER NOT NULL,
    backend_read_timeout_ms INTEGER NOT NULL,
    backend_write_timeout_ms INTEGER NOT NULL,
    backend_tls_client_cert_path TEXT,
    backend_tls_client_key_path TEXT,
    backend_tls_verify_server_cert INTEGER NOT NULL DEFAULT 1, -- Boolean as INTEGER
    backend_tls_server_ca_cert_path TEXT,
    dns_override TEXT,
    dns_cache_ttl_seconds INTEGER,
    auth_mode TEXT NOT NULL DEFAULT 'single' CHECK (auth_mode IN ('single', 'multi')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_proxies_listen_path ON proxies(listen_path);

-- Consumers table
CREATE TABLE IF NOT EXISTS consumers (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    custom_id TEXT,
    credentials TEXT, -- JSON as TEXT
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_consumers_username ON consumers(username);
CREATE INDEX IF NOT EXISTS idx_consumers_custom_id ON consumers(custom_id) WHERE custom_id IS NOT NULL;

-- Plugin configurations table
CREATE TABLE IF NOT EXISTS plugin_configs (
    id TEXT PRIMARY KEY,
    plugin_name TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}', -- JSON as TEXT
    scope TEXT NOT NULL CHECK (scope IN ('global', 'proxy', 'consumer')),
    proxy_id TEXT,
    consumer_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1, -- Boolean as INTEGER
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (proxy_id) REFERENCES proxies(id) ON DELETE CASCADE,
    FOREIGN KEY (consumer_id) REFERENCES consumers(id) ON DELETE CASCADE,
    -- SQLite doesn't support CHECK constraints that reference multiple columns,
    -- so we'll enforce this through application logic instead
    CHECK (
        (scope = 'global') OR
        (scope = 'proxy' AND proxy_id IS NOT NULL) OR
        (scope = 'consumer' AND consumer_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_plugin_configs_plugin_name ON plugin_configs(plugin_name);
CREATE INDEX IF NOT EXISTS idx_plugin_configs_proxy_id ON plugin_configs(proxy_id) WHERE proxy_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_plugin_configs_consumer_id ON plugin_configs(consumer_id) WHERE consumer_id IS NOT NULL;

-- Association table for proxies and plugin configurations
CREATE TABLE IF NOT EXISTS proxy_plugin_associations (
    proxy_id TEXT NOT NULL,
    plugin_config_id TEXT NOT NULL,
    PRIMARY KEY (proxy_id, plugin_config_id),
    FOREIGN KEY (proxy_id) REFERENCES proxies(id) ON DELETE CASCADE,
    FOREIGN KEY (plugin_config_id) REFERENCES plugin_configs(id) ON DELETE CASCADE
);

-- SQLite triggers to update updated_at column
CREATE TRIGGER IF NOT EXISTS update_proxies_updated_at
AFTER UPDATE ON proxies
BEGIN
    UPDATE proxies SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS update_consumers_updated_at
AFTER UPDATE ON consumers
BEGIN
    UPDATE consumers SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS update_plugin_configs_updated_at
AFTER UPDATE ON plugin_configs
BEGIN
    UPDATE plugin_configs SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;
