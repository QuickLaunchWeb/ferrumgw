# Ferrum Gateway Functional Testing

This directory contains everything needed to run and test the Ferrum Gateway locally with a sample configuration.

## Overview

The functional testing environment includes:

- Sample configuration with all supported resource types
- Mock backend services
- Test scripts for both proxy and admin functionality
- Self-signed TLS certificate generation
- A Makefile with common commands for building and running

## Prerequisites

- Rust toolchain (cargo)
- OpenSSL for certificate generation
- Python 3 for mock services
- cURL for testing endpoints
- wscat (optional, for WebSocket testing)

## Quick Start

1. Generate TLS certificates:
   ```
   make certs
   ```

2. Start the mock backend services:
   ```
   make mock-services
   ```

3. In a new terminal, build and run the gateway:
   ```
   make run-file
   ```

4. In another terminal, test the functionality:
   ```
   make test-proxy
   make test-admin
   ```

## Configuration

The `config.yaml` file includes sample configurations for:

- 5 Proxy routes (HTTP, HTTPS, WebSocket)
- 5 Consumers with various credential types
- 5 Plugins (rate limiting, authentication, transformation)

## Available Commands

Run `make help` to see all available commands:

- `make build` - Build the Ferrum Gateway
- `make certs` - Generate self-signed TLS certificates
- `make run` - Run the gateway with default settings (file mode)
- `make run-file` - Run in file mode with sample config.yaml
- `make run-database` - Run in database mode (requires SQLite)
- `make mock-services` - Start mock backend services
- `make test-proxy` - Test proxy routes
- `make test-admin` - Test admin API
- `make clean` - Remove artifacts

## Environment Variables

You can customize the gateway behavior with environment variables:

- `FERRUM_MODE` - Operation mode (file, database, data_plane, control_plane)
- `FERRUM_FILE_CONFIG_PATH` - Path to config file
- `FERRUM_PROXY_HTTP_PORT` - HTTP port for proxy (default: 8000)
- `FERRUM_ADMIN_HTTP_PORT` - HTTP port for admin API (default: 9000)
- `FERRUM_LOG_LEVEL` - Logging level (default: debug)

## Testing HTTP/3

The setup includes HTTP/3 endpoints on ports 8444 (proxy) and 9444 (admin). To test HTTP/3:

1. Ensure you have an HTTP/3 compatible client
2. Use the client to connect to the HTTP/3 endpoints
3. Verify that the connections are established using QUIC protocol

## WebSocket Testing

The setup includes a WebSocket proxy route at `/ws/` that forwards to a mock WebSocket server.
To test WebSocket functionality:

1. Install wscat: `npm install -g wscat`
2. Connect to the WebSocket endpoint: `wscat -c ws://localhost:8000/ws`
3. Send a message and verify that it's echoed back

## Directory Structure

- `config.yaml` - Sample gateway configuration
- `Makefile` - Commands for building and testing
- `README.md` - This documentation
- `start-mock-services.sh` - Script to start mock backend services
- `test-proxy.sh` - Script to test proxy functionality
- `test-admin.sh` - Script to test admin API
- `certs/` - Generated TLS certificates (after running `make certs`)
