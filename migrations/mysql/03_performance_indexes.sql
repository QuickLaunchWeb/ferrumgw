-- Migration to add performance indexes for efficient incremental updates
-- These indexes improve performance for timestamp-based queries

-- Add indexes on updated_at columns for the main tables
CREATE INDEX proxies_updated_at_idx ON proxies(updated_at);
CREATE INDEX consumers_updated_at_idx ON consumers(updated_at);
CREATE INDEX plugin_configs_updated_at_idx ON plugin_configs(updated_at);

-- Add indexes for foreign key columns used in joins
CREATE INDEX proxy_plugin_assoc_proxy_id_idx ON proxy_plugin_associations(proxy_id);
CREATE INDEX proxy_plugin_assoc_plugin_config_id_idx ON proxy_plugin_associations(plugin_config_id);
CREATE INDEX plugin_configs_proxy_id_idx ON plugin_configs(proxy_id);

-- Add unique index for efficient path lookup
CREATE UNIQUE INDEX proxies_listen_path_idx ON proxies(listen_path);
