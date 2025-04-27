-- Migration for deletion tracking to support incremental/delta configuration updates
-- Create tables to track resource deletions

-- Table to track deleted proxies
CREATE TABLE IF NOT EXISTS proxy_deletions (
    id VARCHAR(255) PRIMARY KEY,
    deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Table to track deleted consumers
CREATE TABLE IF NOT EXISTS consumer_deletions (
    id VARCHAR(255) PRIMARY KEY,
    deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Table to track deleted plugin configs
CREATE TABLE IF NOT EXISTS plugin_config_deletions (
    id VARCHAR(255) PRIMARY KEY,
    deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create triggers to automatically track deletions

-- Proxy deletion trigger
DELIMITER //
CREATE TRIGGER proxy_deletion_trigger
AFTER DELETE ON proxies
FOR EACH ROW
BEGIN
    -- Insert deleted proxy ID into the deletions table
    INSERT INTO proxy_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON DUPLICATE KEY UPDATE deleted_at = CURRENT_TIMESTAMP;
END //
DELIMITER ;

-- Consumer deletion trigger
DELIMITER //
CREATE TRIGGER consumer_deletion_trigger
AFTER DELETE ON consumers
FOR EACH ROW
BEGIN
    -- Insert deleted consumer ID into the deletions table
    INSERT INTO consumer_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON DUPLICATE KEY UPDATE deleted_at = CURRENT_TIMESTAMP;
END //
DELIMITER ;

-- Plugin config deletion trigger
DELIMITER //
CREATE TRIGGER plugin_config_deletion_trigger
AFTER DELETE ON plugin_configs
FOR EACH ROW
BEGIN
    -- Insert deleted plugin config ID into the deletions table
    INSERT INTO plugin_config_deletions (id, deleted_at) 
    VALUES (OLD.id, CURRENT_TIMESTAMP)
    ON DUPLICATE KEY UPDATE deleted_at = CURRENT_TIMESTAMP;
END //
DELIMITER ;

-- Create indexes for efficient querying by timestamp
CREATE INDEX proxy_deletions_timestamp_idx ON proxy_deletions(deleted_at);
CREATE INDEX consumer_deletions_timestamp_idx ON consumer_deletions(deleted_at);
CREATE INDEX plugin_config_deletions_timestamp_idx ON plugin_config_deletions(deleted_at);

-- Create maintenance procedure to periodically clean up old deletion records
DELIMITER //
CREATE PROCEDURE cleanup_deletion_records(IN older_than_days INT)
BEGIN
    DECLARE cutoff_date TIMESTAMP;
    SET cutoff_date = DATE_SUB(CURRENT_TIMESTAMP, INTERVAL older_than_days DAY);
    
    -- Delete old records from deletion tracking tables
    DELETE FROM proxy_deletions WHERE deleted_at < cutoff_date;
    DELETE FROM consumer_deletions WHERE deleted_at < cutoff_date;
    DELETE FROM plugin_config_deletions WHERE deleted_at < cutoff_date;
END //
DELIMITER ;

-- Note: You can run the cleanup procedure with:
-- CALL cleanup_deletion_records(30); -- Removes records older than 30 days
