#!/bin/bash
# Test script for Ferrum Gateway proxy functionality

set -e

# Configuration
PROXY_HOST="localhost:8000"
ADMIN_HOST="localhost:9000"
MOBILE_API_KEY="mobileapp123456"
WEB_API_KEY="webapp123456"

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

function test_endpoint() {
    local name="$1"
    local endpoint="$2"
    local expected_status="$3"
    local headers="$4"

    echo "Testing endpoint: $endpoint"
    
    # Make the request with provided headers
    status=$(curl -s -o /dev/null -w "%{http_code}" $headers "$endpoint")
    
    # Check if status matches expected
    if [ "$status" -eq "$expected_status" ]; then
        success "$name: Got expected status $status"
    else
        error "$name: Expected status $expected_status but got $status"
    fi
}

# Check if gateway is running
info "Checking if Ferrum Gateway is running..."
if ! curl -s "http://$ADMIN_HOST/health" > /dev/null; then
    error "Ferrum Gateway is not running. Please start it with 'make run'"
fi
success "Ferrum Gateway is running"

# Test 1: Public API (no auth)
info "Testing public API..."
test_endpoint "Public API" "http://$PROXY_HOST/public/status" 200 ""

# Test 2: API with API Key auth
info "Testing API with API Key auth..."
test_endpoint "API with API Key (valid)" "http://$PROXY_HOST/api/v1/users" 200 "-H 'X-API-Key: $MOBILE_API_KEY'"
test_endpoint "API with API Key (invalid)" "http://$PROXY_HOST/api/v1/users" 401 "-H 'X-API-Key: invalid-key'"

# Test 3: API with API Key via query param
info "Testing API with API Key via query param..."
test_endpoint "API with query param key (valid)" "http://$PROXY_HOST/api/v2/items?api_key=$WEB_API_KEY" 200 ""
test_endpoint "API with query param key (invalid)" "http://$PROXY_HOST/api/v2/items?api_key=invalid-key" 401 ""

# Test 4: Auth service with JWT
info "Testing Auth service with JWT..."
# First, we need to create a test JWT
JWT="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
test_endpoint "Auth service with JWT (valid)" "http://$PROXY_HOST/auth/verify" 200 "-H 'Authorization: Bearer $JWT'"

# Test 5: Rate limiting
info "Testing rate limiting..."
for i in {1..5}; do
    curl -s -o /dev/null "http://$PROXY_HOST/public/status"
    echo -n "."
done
echo ""
success "Made 5 requests to rate-limited endpoint"

# Validate that rate limiting is working by sending enough requests to trigger it
info "Now attempting to trigger rate limit..."
status=$(
    for i in {1..100}; do
        curl -s -o /dev/null -w "%{http_code}" "http://$PROXY_HOST/public/status"
        echo -n "."
    done | grep -m 1 "429" || echo "No rate limiting triggered"
)
if [[ $status == *"429"* ]]; then
    success "Rate limiting triggered successfully (got 429)"
else
    error "Failed to trigger rate limiting"
fi

# Test 6: WebSocket connection
info "Testing WebSocket connection..."
echo "This requires wscat tool. Install with: npm install -g wscat"
if command -v wscat &> /dev/null; then
    echo "Testing WebSocket connection to ws://$PROXY_HOST/ws"
    echo "Sending test message and waiting for response..."
    response=$(echo '{"message":"test"}' | wscat -c "ws://$PROXY_HOST/ws" --connect-timeout 5000 2>/dev/null || echo "Failed to connect")
    if [[ $response == *"Failed to connect"* ]]; then
        error "Failed to establish WebSocket connection"
    else
        success "WebSocket connection established successfully"
    fi
else
    info "wscat tool not found. Skipping WebSocket test."
fi

success "All proxy tests completed successfully!"
