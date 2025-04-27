#!/bin/bash
# Test script for Ferrum Gateway admin API functionality

set -e

# Configuration
ADMIN_HOST="localhost:9000"

# Color output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Helper functions
function success() {
    echo -e "${GREEN}✓ $1${NC}"
}

function error() {
    echo -e "${RED}✗ $1${NC}"
    exit 1
}

function info() {
    echo -e "${YELLOW}ℹ $1${NC}"
}

# Check if gateway is running
info "Checking if Ferrum Gateway admin API is running..."
if ! curl -s "http://$ADMIN_HOST/health" > /dev/null; then
    error "Ferrum Gateway is not running. Please start it with 'make run'"
fi
success "Ferrum Gateway admin API is running"

# Test 1: Health check
info "Testing health check endpoint..."
response=$(curl -s "http://$ADMIN_HOST/health")
if [[ "$response" == *"\"status\":\"ok\""* ]]; then
    success "Health check returned OK status"
else
    error "Health check failed, unexpected response: $response"
fi

# Test 2: Get all proxies
info "Testing GET /proxies endpoint..."
response=$(curl -s "http://$ADMIN_HOST/proxies")
if [[ "$response" == *"\"data\":"* ]]; then
    proxy_count=$(echo "$response" | grep -o "\"id\":" | wc -l)
    success "Found $proxy_count proxies"
else
    error "Failed to get proxies: $response"
fi

# Test 3: Get specific proxy
info "Testing GET /proxies/{id} endpoint..."
proxy_id=$(echo "$response" | grep -o "\"id\":\"[^\"]*\"" | head -1 | cut -d'"' -f4)
if [[ -n "$proxy_id" ]]; then
    response=$(curl -s "http://$ADMIN_HOST/proxies/$proxy_id")
    if [[ "$response" == *"\"id\":\"$proxy_id\""* ]]; then
        success "Successfully retrieved proxy with ID $proxy_id"
    else
        error "Failed to get proxy with ID $proxy_id: $response"
    fi
else
    info "No proxies found to test GET /proxies/{id}"
fi

# Test 4: Get all consumers
info "Testing GET /consumers endpoint..."
response=$(curl -s "http://$ADMIN_HOST/consumers")
if [[ "$response" == *"\"data\":"* ]]; then
    consumer_count=$(echo "$response" | grep -o "\"id\":" | wc -l)
    success "Found $consumer_count consumers"
else
    error "Failed to get consumers: $response"
fi

# Test 5: Get all plugins
info "Testing GET /plugins endpoint..."
response=$(curl -s "http://$ADMIN_HOST/plugins")
if [[ "$response" == *"data"* ]]; then
    plugin_count=$(echo "$response" | grep -o "\"id\":" | wc -l)
    success "Found $plugin_count plugin configurations"
else
    error "Failed to get plugins: $response"
fi

# Test 6: Get plugin types
info "Testing GET /plugins/types endpoint..."
response=$(curl -s "http://$ADMIN_HOST/plugins/types")
if [[ "$response" == *"api_key_auth"* && "$response" == *"rate_limiting"* ]]; then
    plugin_type_count=$(echo "$response" | grep -o "\"type\":" | wc -l)
    success "Found $plugin_type_count plugin types"
else
    error "Failed to get plugin types: $response"
fi

# Test 7: Get DNS cache stats
info "Testing GET /dns/stats endpoint..."
response=$(curl -s "http://$ADMIN_HOST/dns/stats")
if [[ "$response" == *"\"cache_size\":"* ]]; then
    cache_size=$(echo "$response" | grep -o "\"cache_size\":[0-9]*" | cut -d':' -f2)
    success "DNS cache stats: $cache_size entries in cache"
else
    error "Failed to get DNS cache stats: $response"
fi

# Test 8: Pagination on proxies
info "Testing pagination on /proxies endpoint..."
response=$(curl -s "http://$ADMIN_HOST/proxies?page=1&limit=2")
if [[ "$response" == *"\"pagination\":"* ]]; then
    success "Pagination working correctly"
else
    error "Pagination not working: $response"
fi

success "All admin API tests completed successfully!"
