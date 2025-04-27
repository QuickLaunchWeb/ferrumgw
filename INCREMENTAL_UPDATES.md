# Incremental Configuration Updates in Ferrum Gateway

This document describes the implementation of delta-based (incremental) configuration polling in Ferrum Gateway, which optimizes resource usage and improves performance when synchronizing configuration across modes.

## Overview

The incremental update system allows Ferrum Gateway to efficiently detect and apply only configuration changes, rather than reloading the entire configuration on each poll interval. This is particularly beneficial for:

1. Deployments with large configurations
2. High-frequency configuration changes affecting only a few resources
3. Control Plane to Data Plane synchronization with many Data Plane nodes
4. Reducing database load and network traffic

## Implementation Details

### Configuration Model

- **ConfigurationDelta**: A new data structure that contains only changed resources since a specific timestamp:
  - `updated_proxies`: New or modified proxies
  - `deleted_proxy_ids`: IDs of proxies that were deleted
  - `updated_consumers`: New or modified consumers
  - `deleted_consumer_ids`: IDs of consumers that were deleted
  - `updated_plugin_configs`: New or modified plugin configurations
  - `deleted_plugin_config_ids`: IDs of plugin configurations that were deleted
  - `last_updated_at`: Timestamp of this delta

- **Database Tracking**: Deletion tracking tables in all supported databases (PostgreSQL, MySQL, SQLite):
  - `proxy_deletions`
  - `consumer_deletions`
  - `plugin_config_deletions`

### Database Implementation

Each database client now supports:

1. **Lightweight Change Detection**: 
   - `get_latest_update_timestamp()`: Efficiently checks for any changes without fetching full data

2. **Delta Loading**:
   - `load_configuration_delta(since)`: Loads only resources changed since a given timestamp
   - Uses SQL queries with timestamp filtering to minimize data transfer

3. **Deletion Tracking**:
   - Database triggers (PostgreSQL, MySQL) and explicit tracking (SQLite)
   - Maintains a deletion history with timestamps for incremental updates
   - Automatic maintenance procedures to clean up old deletion records

### Polling Mechanism

The polling system now operates on two intervals:

1. **Check Interval** (fast, lightweight):
   - Checks only for the presence of changes without fetching data
   - Default: 5 seconds
   - Configurable via `FERRUM_DB_POLL_CHECK_INTERVAL` (in seconds)

2. **Full Poll Interval** (less frequent):
   - Performs full configuration refresh for resilience
   - Default: 30 seconds 
   - Configurable via `FERRUM_DB_POLL_INTERVAL` (in seconds)

### Control Plane to Data Plane Updates

- When using incremental polling, Control Plane sends only deltas to Data Plane nodes
- Full snapshot is sent only when a DP node is new or significantly out of date
- Efficient propagation to many Data Plane nodes

## Configuration Options

| Environment Variable | Default | Description |
|----------------------|---------|-------------|
| `FERRUM_DB_POLL_INTERVAL` | `30` (seconds) | Interval for full configuration checks |
| `FERRUM_DB_POLL_CHECK_INTERVAL` | `5` (seconds) | Interval for lightweight change detection |
| `FERRUM_DB_INCREMENTAL_POLLING` | `true` | Enable/disable incremental polling |

## Benefits

1. **Reduced Database Load**: Queries only fetch changed data, not the entire configuration
2. **Lower Network Traffic**: Only changes are transmitted between components
3. **Faster Updates**: Changes propagate more quickly via the check interval
4. **Improved Scalability**: Supports larger configurations and more Data Plane nodes
5. **Better Resource Utilization**: Less CPU, memory, and network bandwidth consumption

## Robustness and Fallbacks

The implementation includes several safety mechanisms:

1. **Resilient Fallback**: If delta updates fail, the system falls back to a full configuration reload
2. **Periodic Full Refresh**: Even with incremental enabled, periodic full refreshes ensure consistency
3. **Database Indexes**: New indexes on deletion tables ensure efficient queries
4. **Transaction Safety**: All database operations use transactions for consistency
5. **Error Handling**: Comprehensive error handling and logging

## Example: Update Flow

1. Admin makes a change via Admin API
2. Database poll check detects a timestamp change 
3. Delta is loaded with only changed resources
4. Delta is applied to the in-memory configuration
5. Router and DNS cache are selectively updated
6. If Control Plane, delta is propagated to Data Plane nodes
