-- Migration for deletion tracking to support incremental/delta configuration updates
-- Create tables to track resource deletions

-- Table to track deleted proxies
CREATE TABLE IF NOT EXISTS proxy_deletions (
    id TEXT PRIMARY KEY,
    deleted_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Table to track deleted consumers
CREATE TABLE IF NOT EXISTS consumer_deletions (
    id TEXT PRIMARY KEY,
    deleted_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Table to track deleted plugin configs
CREATE TABLE IF NOT EXISTS plugin_config_deletions (
    id TEXT PRIMARY KEY,
    deleted_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create triggers to automatically track deletions

-- Proxy deletion trigger
CREATE OR REPLACE FUNCTION track_proxy_deletion() RETURNS TRIGGER AS $$
BEGIN
    -- Insert deleted proxy ID into the deletions table
    INSERT INTO proxy_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON CONFLICT (id) DO UPDATE 
    SET deleted_at = CURRENT_TIMESTAMP;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER proxy_deletion_trigger
AFTER DELETE ON proxies
FOR EACH ROW
EXECUTE FUNCTION track_proxy_deletion();

-- Consumer deletion trigger
CREATE OR REPLACE FUNCTION track_consumer_deletion() RETURNS TRIGGER AS $$
BEGIN
    -- Insert deleted consumer ID into the deletions table
    INSERT INTO consumer_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON CONFLICT (id) DO UPDATE 
    SET deleted_at = CURRENT_TIMESTAMP;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER consumer_deletion_trigger
AFTER DELETE ON consumers
FOR EACH ROW
EXECUTE FUNCTION track_consumer_deletion();

-- Plugin config deletion trigger
CREATE OR REPLACE FUNCTION track_plugin_config_deletion() RETURNS TRIGGER AS $$
BEGIN
    -- Insert deleted plugin config ID into the deletions table
    INSERT INTO plugin_config_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON CONFLICT (id) DO UPDATE 
    SET deleted_at = CURRENT_TIMESTAMP;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER plugin_config_deletion_trigger
AFTER DELETE ON plugin_configs
FOR EACH ROW
EXECUTE FUNCTION track_plugin_config_deletion();

-- Create indexes for efficient querying by timestamp
CREATE INDEX IF NOT EXISTS proxy_deletions_timestamp_idx ON proxy_deletions(deleted_at);
CREATE INDEX IF NOT EXISTS consumer_deletions_timestamp_idx ON consumer_deletions(deleted_at);
CREATE INDEX IF NOT EXISTS plugin_config_deletions_timestamp_idx ON plugin_config_deletions(deleted_at);

-- Create maintenance function to periodically clean up old deletion records
CREATE OR REPLACE FUNCTION cleanup_deletion_records(older_than_days INTEGER) RETURNS void AS $$
DECLARE
    cutoff_date TIMESTAMP WITH TIME ZONE;
BEGIN
    cutoff_date := CURRENT_TIMESTAMP - (older_than_days * INTERVAL '1 day');
    
    -- Delete old records from deletion tracking tables
    DELETE FROM proxy_deletions WHERE deleted_at < cutoff_date;
    DELETE FROM consumer_deletions WHERE deleted_at < cutoff_date;
    DELETE FROM plugin_config_deletions WHERE deleted_at < cutoff_date;
END;
$$ LANGUAGE plpgsql;

-- Note: You can run the cleanup function with:
-- SELECT cleanup_deletion_records(30); -- Removes records older than 30 days
