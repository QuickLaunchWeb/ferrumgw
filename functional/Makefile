# Ferrum Gateway Functional Test Makefile

# Default configuration
FERRUM_MODE ?= file
FERRUM_FILE_CONFIG_PATH ?= $(CURDIR)/config.yaml
FERRUM_PROXY_HTTP_PORT ?= 8000
FERRUM_PROXY_HTTPS_PORT ?= 8443
FERRUM_PROXY_HTTP3_PORT ?= 8444
FERRUM_ADMIN_HTTP_PORT ?= 9000
FERRUM_ADMIN_HTTPS_PORT ?= 9443
FERRUM_ADMIN_HTTP3_PORT ?= 9444
FERRUM_LOG_LEVEL ?= debug

# Self-signed TLS certificate generation paths
CERT_DIR := $(CURDIR)/certs
CERT_FILE := $(CERT_DIR)/cert.pem
KEY_FILE := $(CERT_DIR)/key.pem

.PHONY: all build run run-file run-database test-proxy test-admin clean certs mock-services help

all: help

help:
	@echo "Ferrum Gateway Functional Testing"
	@echo ""
	@echo "Available commands:"
	@echo "  make build           - Build the Ferrum Gateway"
	@echo "  make certs           - Generate self-signed TLS certificates"
	@echo "  make run             - Run the gateway with default settings (file mode)"
	@echo "  make run-file        - Run in file mode with sample config.yaml"
	@echo "  make run-database    - Run in database mode (requires PostgreSQL)"
	@echo "  make mock-services   - Start mock backend services"
	@echo "  make test-proxy      - Test proxy routes"
	@echo "  make test-admin      - Test admin API"
	@echo "  make clean           - Remove artifacts"
	@echo ""
	@echo "Environment variables:"
	@echo "  FERRUM_MODE          - Operation mode (file, database, data_plane, control_plane)"
	@echo "  FERRUM_FILE_CONFIG_PATH - Path to config file (default: ./config.yaml)"
	@echo "  FERRUM_PROXY_HTTP_PORT  - HTTP port for proxy (default: 8000)"
	@echo "  FERRUM_ADMIN_HTTP_PORT  - HTTP port for admin API (default: 9000)"
	@echo "  FERRUM_LOG_LEVEL     - Logging level (default: debug)"

build:
	@echo "Building Ferrum Gateway..."
	cd .. && cargo build

certs:
	@echo "Generating self-signed TLS certificates..."
	@mkdir -p $(CERT_DIR)
	openssl req -x509 -newkey rsa:4096 -keyout $(KEY_FILE) -out $(CERT_FILE) -days 365 -nodes -subj "/CN=localhost"
	@echo "Certificates generated at $(CERT_DIR)"

run: build
	@echo "Running Ferrum Gateway in file mode..."
	cd .. && RUST_LOG=$(FERRUM_LOG_LEVEL) \
		FERRUM_MODE=$(FERRUM_MODE) \
		FERRUM_FILE_CONFIG_PATH=$(FERRUM_FILE_CONFIG_PATH) \
		FERRUM_PROXY_HTTP_PORT=$(FERRUM_PROXY_HTTP_PORT) \
		FERRUM_PROXY_HTTPS_PORT=$(FERRUM_PROXY_HTTPS_PORT) \
		FERRUM_PROXY_HTTP3_PORT=$(FERRUM_PROXY_HTTP3_PORT) \
		FERRUM_ADMIN_HTTP_PORT=$(FERRUM_ADMIN_HTTP_PORT) \
		FERRUM_ADMIN_HTTPS_PORT=$(FERRUM_ADMIN_HTTPS_PORT) \
		FERRUM_ADMIN_HTTP3_PORT=$(FERRUM_ADMIN_HTTP3_PORT) \
		FERRUM_PROXY_TLS_CERT_PATH=$(CERT_FILE) \
		FERRUM_PROXY_TLS_KEY_PATH=$(KEY_FILE) \
		FERRUM_ADMIN_TLS_CERT_PATH=$(CERT_FILE) \
		FERRUM_ADMIN_TLS_KEY_PATH=$(KEY_FILE) \
		cargo run

run-file: build certs
	@echo "Running Ferrum Gateway with file configuration..."
	$(MAKE) run FERRUM_MODE=file

run-database: build certs
	@echo "Running Ferrum Gateway with database configuration..."
	$(MAKE) run FERRUM_MODE=database FERRUM_DB_TYPE=sqlite FERRUM_DB_URL=sqlite://ferrum.db

mock-services:
	@echo "Starting mock backend services..."
	./start-mock-services.sh

test-proxy:
	@echo "Testing proxy functionality..."
	./test-proxy.sh

test-admin:
	@echo "Testing admin API..."
	./test-admin.sh

clean:
	@echo "Cleaning up..."
	rm -rf $(CERT_DIR)
	cd .. && cargo clean
