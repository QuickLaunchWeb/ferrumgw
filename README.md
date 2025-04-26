# Ferrum Gateway

Ferrum Gateway is a high-performance, extensible API Gateway and Reverse Proxy built in Rust. It leverages `tokio` for asynchronous processing and `hyper` for HTTP functionality, making it capable of handling high throughput and low latency traffic efficiently.

## Features

- **High Performance**: Built on Rust and tokio for maximum throughput and minimal latency
- **Multiple Operating Modes**: Support for Database, File, and Control Plane/Data Plane modes
- **Dynamic Configuration**: Hot-reload of configuration without downtime
- **Advanced Routing**: Longest prefix matching for request routing
- **Protocol Support**: HTTP, HTTPS, WebSockets, gRPC
- **TLS Support**: End-to-end encryption with TLS termination and mTLS to backends
- **Authentication/Authorization**: Multiple authentication methods (JWT, OAuth2, Basic Auth, Key Auth)
- **Plugin System**: Extensible architecture with plugin hooks at key request/response processing stages
- **Rate Limiting**: Protect backends with configurable rate limits
- **Request/Response Transformation**: Modify requests and responses on-the-fly
- **DNS Caching**: Optimized DNS resolution with configurable caching
- **Logging and Monitoring**: Detailed logging with JSON output and metrics endpoint

## Operating Modes

Ferrum Gateway supports three distinct operating modes:

### Database Mode (`FERRUM_MODE=database`)

In this mode, Ferrum Gateway reads its configuration from a database (PostgreSQL, MySQL, or SQLite), handles end-user proxy traffic, and provides an Admin API for configuration management. The gateway polls the database periodically to detect configuration changes and applies them with zero downtime.

**Use cases**: Single gateway instance or small deployments where direct database access is viable.

### File Mode (`FERRUM_MODE=file`)

In this mode, Ferrum Gateway reads its configuration from local YAML or JSON files. It only handles end-user proxy traffic and does not provide an Admin API. Configuration changes require a file update and a reload signal (SIGHUP).

**Use cases**: Simple deployments, static configurations, or environments where a database is not available.

### Control Plane / Data Plane Mode (`FERRUM_MODE=cp` and `FERRUM_MODE=dp`)

This mode splits the gateway into two types of nodes:

- **Control Plane (CP)**: Acts as the centralized configuration authority, reading from a database and exposing the Admin API. It does not process end-user traffic but instead pushes configuration to Data Plane nodes via gRPC.
- **Data Plane (DP)**: Processes end-user traffic based on configuration received from the Control Plane. Does not connect directly to the database and does not expose an Admin API.

**Use cases**: Large-scale deployments where separation of concerns and horizontal scaling of proxy traffic is required.

## Prerequisites

- Rust toolchain (latest stable version)
- For Database mode: PostgreSQL, MySQL, or SQLite database
- For TLS support: TLS certificates and private keys

## Installation

Clone the repository and build the application:

```bash
git clone https://github.com/QuickLaunchWeb/ferrumgw.git
cd ferrumgw
cargo build --release
```

The binary will be available at `target/release/ferrumgw`.

## Getting Started

### Running in Database Mode

```bash
export FERRUM_MODE=database
export FERRUM_DB_TYPE=postgres
export FERRUM_DB_URL=postgres://user:password@localhost/ferrumgw
export FERRUM_ADMIN_JWT_SECRET=your-secret-key
./target/release/ferrumgw
```

### Running in File Mode

```bash
export FERRUM_MODE=file
export FERRUM_FILE_CONFIG_PATH=/path/to/config.yaml
./target/release/ferrumgw
```

### Running in Control Plane / Data Plane Mode

Control Plane:

```bash
export FERRUM_MODE=cp
export FERRUM_DB_TYPE=postgres
export FERRUM_DB_URL=postgres://user:password@localhost/ferrumgw
export FERRUM_ADMIN_JWT_SECRET=your-admin-secret
export FERRUM_CP_GRPC_LISTEN_ADDR=0.0.0.0:50051
export FERRUM_CP_GRPC_JWT_SECRET=your-grpc-secret
./target/release/ferrumgw
```

Data Plane:

```bash
export FERRUM_MODE=dp
export FERRUM_DP_CP_GRPC_URL=http://cp-hostname:50051
export FERRUM_DP_GRPC_AUTH_TOKEN=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
./target/release/ferrumgw
```

## Configuration

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|---------|----------|
| `FERRUM_MODE` | Operating mode (`database`, `file`, `cp`, `dp`) | - | Yes |
| `FERRUM_LOG_LEVEL` | Log level (`error`, `warn`, `info`, `debug`, `trace`) | `info` | No |
| `FERRUM_PROXY_HTTP_PORT` | HTTP port for proxy traffic | `8000` | No |
| `FERRUM_PROXY_HTTPS_PORT` | HTTPS port for proxy traffic | `8443` | No |
| `FERRUM_PROXY_TLS_CERT_PATH` | Path to TLS certificate for HTTPS proxy | - | If HTTPS enabled |
| `FERRUM_PROXY_TLS_KEY_PATH` | Path to TLS private key for HTTPS proxy | - | If HTTPS enabled |
| `FERRUM_ADMIN_HTTP_PORT` | HTTP port for Admin API | `9000` | No |
| `FERRUM_ADMIN_HTTPS_PORT` | HTTPS port for Admin API | `9443` | No |
| `FERRUM_ADMIN_TLS_CERT_PATH` | Path to TLS certificate for HTTPS Admin API | - | If Admin HTTPS enabled |
| `FERRUM_ADMIN_TLS_KEY_PATH` | Path to TLS private key for HTTPS Admin API | - | If Admin HTTPS enabled |
| `FERRUM_ADMIN_JWT_SECRET` | Secret for Admin API JWT authentication | - | In Database & CP modes |
| `FERRUM_CP_GRPC_JWT_SECRET` | Secret for CP gRPC authentication | - | In CP mode |
| `FERRUM_DP_GRPC_AUTH_TOKEN` | JWT token for DP authentication to CP | - | In DP mode |
| `FERRUM_DB_TYPE` | Database type (`postgres`, `mysql`, `sqlite`) | - | In Database & CP modes |
| `FERRUM_DB_URL` | Database connection URL | - | In Database & CP modes |
| `FERRUM_DB_POLL_INTERVAL_SECONDS` | Interval for polling DB changes | `15` | No |
| `FERRUM_FILE_CONFIG_PATH` | Path to config file or directory | - | In File mode |
| `FERRUM_CP_GRPC_LISTEN_ADDR` | Address for CP gRPC server | - | In CP mode |
| `FERRUM_DP_CP_GRPC_URL` | URL of CP gRPC server | - | In DP mode |
| `FERRUM_MAX_HEADER_SIZE_BYTES` | Maximum request header size | `16384` | No |
| `FERRUM_MAX_BODY_SIZE_BYTES` | Maximum request body size | `10485760` | No |
| `FERRUM_DNS_CACHE_TTL_SECONDS` | TTL for DNS cache entries | `300` | No |
| `FERRUM_DNS_OVERRIDES` | DNS hostname overrides (JSON) | `{}` | No |

### File Configuration Format

When using File mode, Ferrum Gateway expects a YAML or JSON configuration file with the following structure:

```yaml
proxies:
  - id: "proxy1"
    name: "Example API Proxy"
    listen_path: "/api"
    backend_protocol: "https"
    backend_host: "api.example.com"
    backend_port: 443
    backend_path: "/v1"
    strip_listen_path: true
    preserve_host_header: false
    backend_connect_timeout_ms: 5000
    backend_read_timeout_ms: 30000
    backend_write_timeout_ms: 30000
    auth_mode: "single"
    plugins:
      - plugin_config_id: "plugin1"

consumers:
  - id: "consumer1"
    username: "api-user"
    credentials:
      keyauth:
        key: "hashed-api-key-value"

plugin_configs:
  - id: "plugin1"
    plugin_name: "key_auth"
    config:
      key_location: "header"
      header_name: "X-API-Key"
    scope: "proxy"
    proxy_id: "proxy1"
    enabled: true
```

### Database Schema

For Database and CP modes, Ferrum Gateway requires the following tables:

- `proxies`: Stores proxy configurations
- `consumers`: Stores consumer identities
- `plugin_configs`: Stores plugin configurations
- `proxy_plugin_associations`: Links plugins to proxies

The `proxies` table must have a UNIQUE constraint on the `listen_path` column.

## Admin API

The Admin API is available in Database and CP modes, providing a RESTful interface for managing gateway resources.

### Authentication

All Admin API requests (except `/health`) require a valid JWT token in the `Authorization` header:

```
Authorization: Bearer <jwt_token>
```

You can generate a token with a tool like:

```bash
export TOKEN=$(curl -s -X POST http://localhost:9000/admin/tokens \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"your-admin-password"}' | jq -r '.token')
```

### API Endpoints

#### Proxies

- `GET /proxies` - List all proxies
- `POST /proxies` - Create a new proxy
- `GET /proxies/{proxy_id}` - Get a specific proxy
- `PUT /proxies/{proxy_id}` - Update a proxy
- `DELETE /proxies/{proxy_id}` - Delete a proxy

Example:

```bash
# Create a proxy
curl -X POST http://localhost:9000/proxies \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Test API",
    "listen_path": "/test",
    "backend_protocol": "http",
    "backend_host": "httpbin.org",
    "backend_port": 80,
    "strip_listen_path": true
  }'
```

#### Consumers

- `GET /consumers` - List all consumers
- `POST /consumers` - Create a new consumer
- `GET /consumers/{consumer_id}` - Get a specific consumer
- `PUT /consumers/{consumer_id}` - Update a consumer
- `DELETE /consumers/{consumer_id}` - Delete a consumer
- `PUT /consumers/{consumer_id}/credentials/{credential_type}` - Set credentials
- `DELETE /consumers/{consumer_id}/credentials/{credential_type}` - Delete credentials

#### Plugins

- `GET /plugins` - List available plugin types
- `GET /plugins/config` - List all plugin configurations
- `POST /plugins/config` - Create a new plugin configuration
- `GET /plugins/config/{config_id}` - Get a specific plugin configuration
- `PUT /plugins/config/{config_id}` - Update a plugin configuration
- `DELETE /plugins/config/{config_id}` - Delete a plugin configuration

#### Metrics

- `GET /admin/metrics` - Get runtime metrics

Example metrics response:

```json
{
  "mode": "database",
  "config_last_updated_at": "2025-04-26T03:00:00Z",
  "config_source_status": "online",
  "proxy_count": 5,
  "consumer_count": 10,
  "requests_per_second_current": 42.5,
  "status_codes_last_second": {
    "200": 38,
    "401": 3,
    "404": 1,
    "500": 0
  }
}
```

## Plugin System

Ferrum Gateway includes a plugin system that allows extending functionality at various points in the request/response lifecycle.

### Plugin Lifecycle Hooks

- `on_request_received`: Called when a request is first received
- `authenticate`: Called during the authentication phase
- `authorize`: Called during the authorization phase
- `before_proxy`: Called just before the request is proxied to the backend
- `after_proxy`: Called after receiving the response from the backend
- `log`: Called to log the transaction

### Multi-Authentication Mode

When a Proxy is configured with `auth_mode: "multi"`, all attached authentication plugins are executed sequentially. The first plugin that successfully identifies a Consumer attaches that context to the request. The Access Control plugin then checks if any Consumer was identified.

### Built-in Plugins

#### stdout_logging

Logs transaction summaries to standard output in text or JSON format.

Configuration:
```json
{
  "json_format": true
}
```

#### http_logging

Sends transaction summaries to an HTTP endpoint.

Configuration:
```json
{
  "endpoint_url": "http://log-collector:8080/logs",
  "authorization_header": "Bearer your-secret-token",
  "timeout_ms": 5000
}
```

#### transaction_debugger

Logs verbose request/response details for debugging.

Configuration:
```json
{
  "log_request_body": true,
  "log_response_body": true,
  "max_body_size": 10240
}
```

#### oauth2_auth

Performs OAuth2 authentication using Bearer tokens.

Configuration:
```json
{
  "validation_mode": "introspection",
  "introspection_url": "https://auth.example.com/oauth2/introspect",
  "introspection_client_id": "client-id",
  "introspection_client_secret": "client-secret",
  "consumer_claim_field": "sub"
}
```

#### jwt_auth

Performs JWT Bearer token authentication.

Configuration:
```json
{
  "token_lookup": "header",
  "consumer_claim_field": "sub",
  "algorithm": "HS256",
  "secret": "your-jwt-secret"
}
```

#### key_auth

Performs API Key authentication.

Configuration:
```json
{
  "key_location": "header",
  "header_name": "X-API-Key"
}
```

#### basic_auth

Performs HTTP Basic authentication.

Configuration:
```json
{
  "realm": "API Gateway"
}
```

#### access_control

Authorizes requests based on consumer identity.

Configuration:
```json
{
  "allowed_consumers": ["user1", "user2"],
  "disallowed_consumers": ["blocked-user"],
  "allow_anonymous": false
}
```

#### request_transformer

Modifies incoming requests before proxying.

Configuration:
```json
{
  "add_headers": {
    "X-Request-ID": "${uuid}"
  },
  "remove_headers": ["X-Powered-By"],
  "add_query_params": {
    "version": "1.0"
  }
}
```

#### response_transformer

Modifies responses before sending back to clients.

Configuration:
```json
{
  "add_headers": {
    "X-Served-By": "Ferrum Gateway"
  },
  "remove_headers": ["Server"],
  "hide_server_header": true
}
```

#### rate_limiting

Enforces request rate limits.

Configuration:
```json
{
  "limit_by": "consumer",
  "requests_per_second": 10,
  "requests_per_minute": 300,
  "requests_per_hour": 10000,
  "add_headers": true
}
```

## Proxying Behavior

### Routing

Ferrum Gateway uses **longest prefix matching** to select the appropriate Proxy for each request. Given a request to `/api/users/123` and two proxies with `listen_path` values of `/api` and `/api/users`, the `/api/users` proxy would be selected because it provides a longer matching prefix.

### Path Handling

The handling of paths depends on the `strip_listen_path` setting:

- When `strip_listen_path: true` (default): If a request comes to `/api/users/123` and matches a proxy with `listen_path: "/api"`, only `/users/123` will be forwarded to the backend.
- When `strip_listen_path: false`: The full original path (`/api/users/123`) will be forwarded to the backend.

The `backend_path` setting adds a prefix to the forwarded path. If set to `/v1`, the above example would forward to `/v1/users/123`.

### WebSocket & gRPC Support

Ferrum Gateway supports WebSocket upgrades and gRPC (HTTP/2) proxying. Configure the appropriate `backend_protocol` in the Proxy settings.

## Resilience & Caching

### Configuration Caching

In all modes, Ferrum Gateway maintains an in-memory cache of the last valid configuration. If the configuration source (database or Control Plane) becomes temporarily unavailable, the gateway continues to operate using the cached configuration.

### DNS Caching

Ferrum Gateway implements an in-memory DNS cache for resolving backend hostnames. On startup, it performs DNS warmup by resolving all unique backend hostnames to minimize latency on initial requests.

Cache TTL can be configured globally (`FERRUM_DNS_CACHE_TTL_SECONDS`) or per-proxy (`dns_cache_ttl_seconds`).

### Static DNS Overrides

For testing or specific routing needs, you can provide static DNS overrides:

- Globally via the `FERRUM_DNS_OVERRIDES` environment variable
- Per-proxy via the `dns_override` field

## Security

### TLS Configuration

For HTTPS support, provide valid TLS certificates and private keys via the respective environment variables.

### JWT Secrets

The Admin API and CP/DP communications use JWT for authentication. Ensure these secrets are properly secured:

- `FERRUM_ADMIN_JWT_SECRET`: For Admin API authentication
- `FERRUM_CP_GRPC_JWT_SECRET`: For CP/DP authentication

### Credential Hashing

Ferrum Gateway automatically hashes sensitive credentials (passwords, API keys, secrets) before storing them in the database.

## Testing

### Unit Tests

Run the unit test suite:

```bash
# Run all tests
cargo test

# Run tests for a specific module
cargo test --package ferrumgw --lib database

# Run tests with verbose output
cargo test -- --nocapture
```

### Integration Tests

Integration tests verify the full functionality of the gateway:

```bash
# Run integration tests (requires Docker)
cargo test --test integration

# Run a specific integration test
cargo test --test integration -- test_proxy_routing
```

For database integration tests, you can use the provided Docker Compose file:

```bash
# Start the test database containers
docker-compose -f docker/docker-compose.test.yml up -d

# Run the database integration tests
cargo test --test database_integration

# Stop the containers when done
docker-compose -f docker/docker-compose.test.yml down
```

### Manual Testing

You can manually test the gateway with curl:

```bash
# Start the gateway in file mode
export FERRUM_MODE=file
export FERRUM_FILE_CONFIG_PATH=./config/test.yaml
cargo run

# In another terminal, test a proxy route
curl -v http://localhost:8000/api/test

# Test the metrics endpoint
curl -v http://localhost:9000/admin/metrics
```

## Building for Production

For production deployments, build with optimizations and SQLx offline mode:

```bash
# Generate SQLx query metadata (if using database mode)
cargo sqlx prepare --database-url "postgres://user:password@localhost/ferrumgw"

# Build with all optimizations
cargo build --release

# Check binary size and dependencies
ls -lh target/release/ferrumgw
ldd target/release/ferrumgw
```

### Containerization

A Dockerfile is provided for containerized deployments:

```bash
# Build the Docker image
docker build -t ferrumgw:latest .

# Run the container
docker run -p 8000:8000 -p 9000:9000 \
  -e FERRUM_MODE=database \
  -e FERRUM_DB_TYPE=postgres \
  -e FERRUM_DB_URL=postgres://user:password@db-host/ferrumgw \
  ferrumgw:latest
```

## Development Workflow

### Setting Up the Development Environment

1. Clone the repository:
   ```bash
   git clone https://github.com/QuickLaunchWeb/ferrumgw.git
   cd ferrumgw
   ```

2. Install development tools:
   ```bash
   # Install SQLx CLI (for database migrations)
   cargo install sqlx-cli
   
   # Install development tools
   cargo install cargo-watch cargo-expand cargo-udeps
   ```

3. Set up a local database (for Database or CP mode):
   ```bash
   # PostgreSQL
   createdb ferrumgw
   
   # Run migrations
   sqlx migrate run --database-url postgres://localhost/ferrumgw
   ```

4. Run with auto-reload during development:
   ```bash
   # Start with cargo-watch for automatic rebuilding
   cargo watch -x 'run -- --mode=file --config=./config/dev.yaml'
   ```

## Monitoring and Operation

### Health Checks

The gateway provides health check endpoints:

- Proxy traffic: `GET /health` - returns 200 OK when the proxy is operational
- Admin API: `GET /admin/health` - returns 200 OK when the admin API is operational

### Metrics

Runtime metrics are available at the `/admin/metrics` endpoint. You can integrate with Prometheus by using the provided metrics exporter plugin.

### Performance Tuning

For high-traffic deployments, consider the following optimizations:

- Increase system file descriptor limits (ulimit)
- Enable kernel SO_REUSEPORT for better load balancing across worker threads
- Set RUSTFLAGS="-C target-cpu=native" for platform-specific optimizations
- Adjust worker thread count with TOKIO_WORKER_THREADS environment variable
- Configure larger connection pool sizes for the database client

## Disaster Recovery

### Backup and Restore

For Database mode, regular database backups are recommended:

```bash
# PostgreSQL backup example
pg_dump -Fc ferrumgw > ferrumgw_backup.dump

# Restore
pg_restore -d ferrumgw ferrumgw_backup.dump
```

### Fallback Strategies

Configure your deployment with fallback mechanisms:

1. Use multiple DP nodes with a load balancer
2. Implement file mode as a fallback if the database becomes unavailable
3. Use DNS failover for CP redundancy in Control Plane/Data Plane mode

## Troubleshooting

### Common Issues

- **404 Not Found on a proxied request**: Check that your `listen_path` is configured correctly and the matching proxy is active.
- **502 Bad Gateway**: The gateway couldn't establish a connection to the backend. Check backend availability and DNS resolution.
- **Database connection errors**: Verify your database connection string and ensure the database server is running.
- **Configuration not updating**: Check database connectivity or CP/DP connections, depending on your mode.

### Debugging Tips

1. Increase the log level: `FERRUM_LOG_LEVEL=debug`
2. Use the `transaction_debugger` plugin on specific proxies to trace request/response flow
3. Check metrics via the `/admin/metrics` endpoint to monitor gateway health

## Contributing

Contributions to Ferrum Gateway are welcome! Please follow these steps:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

Ferrum Gateway is licensed under the Apache License 2.0. See the LICENSE file for details.
