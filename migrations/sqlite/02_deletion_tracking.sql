-- Migration for deletion tracking to support incremental/delta configuration updates
-- Create tables to track resource deletions

-- Table to track deleted proxies
CREATE TABLE IF NOT EXISTS proxy_deletions (
    id TEXT PRIMARY KEY,
    deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Table to track deleted consumers
CREATE TABLE IF NOT EXISTS consumer_deletions (
    id TEXT PRIMARY KEY,
    deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Table to track deleted plugin configs
CREATE TABLE IF NOT EXISTS plugin_config_deletions (
    id TEXT PRIMARY KEY,
    deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create triggers to automatically track deletions

-- Proxy deletion trigger
CREATE TRIGGER IF NOT EXISTS proxy_deletion_trigger
AFTER DELETE ON proxies
FOR EACH ROW
BEGIN
    -- Insert deleted proxy ID into the deletions table
    INSERT INTO proxy_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON CONFLICT(id) DO UPDATE SET deleted_at = CURRENT_TIMESTAMP;
END;

-- Consumer deletion trigger
CREATE TRIGGER IF NOT EXISTS consumer_deletion_trigger
AFTER DELETE ON consumers
FOR EACH ROW
BEGIN
    -- Insert deleted consumer ID into the deletions table
    INSERT INTO consumer_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON CONFLICT(id) DO UPDATE SET deleted_at = CURRENT_TIMESTAMP;
END;

-- Plugin config deletion trigger
CREATE TRIGGER IF NOT EXISTS plugin_config_deletion_trigger
AFTER DELETE ON plugin_configs
FOR EACH ROW
BEGIN
    -- Insert deleted plugin config ID into the deletions table
    INSERT INTO plugin_config_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON CONFLICT(id) DO UPDATE SET deleted_at = CURRENT_TIMESTAMP;
END;

-- Create indexes for efficient querying by timestamp
CREATE INDEX IF NOT EXISTS proxy_deletions_timestamp_idx ON proxy_deletions(deleted_at);
CREATE INDEX IF NOT EXISTS consumer_deletions_timestamp_idx ON consumer_deletions(deleted_at);
CREATE INDEX IF NOT EXISTS plugin_config_deletions_timestamp_idx ON plugin_config_deletions(deleted_at);

-- SQLite doesn't support stored procedures, so we'll need to run cleanup from application code
-- or use a SQL function that can be called directly:

-- SQL to clean up old deletion records (to be executed from application):
-- DELETE FROM proxy_deletions WHERE deleted_at < datetime('now', '-30 days');
-- DELETE FROM consumer_deletions WHERE deleted_at < datetime('now', '-30 days');
-- DELETE FROM plugin_config_deletions WHERE deleted_at < datetime('now', '-30 days');
