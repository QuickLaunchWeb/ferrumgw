#!/bin/bash
# Mock backend services for Ferrum Gateway testing

echo "Starting mock backend services..."

# Create a temporary directory for mock services
MOCK_DIR=$(mktemp -d)
echo "Using temporary directory: $MOCK_DIR"

# Create a cleanup function
cleanup() {
  echo "Cleaning up mock services..."
  kill $(jobs -p) 2>/dev/null
  rm -rf "$MOCK_DIR"
  exit
}

# Set up cleanup on script exit
trap cleanup EXIT INT TERM

# Function to create a simple HTTP mock service
create_mock_service() {
  local port=$1
  local name=$2
  local response_file="$MOCK_DIR/$name.json"
  
  # Create a sample response file
  cat > "$response_file" << EOF
{
  "service": "$name",
  "version": "1.0.0",
  "timestamp": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "message": "Hello from $name mock service"
}
EOF

  # Start a simple HTTP server on the specified port
  echo "Starting $name mock service on port $port..."
  (cd "$MOCK_DIR" && python3 -m http.server $port) &
  
  # Wait for the server to start
  sleep 1
  echo "$name mock service is running on http://localhost:$port"
}

# WebSocket mock server function
create_ws_mock_service() {
  local port=$1
  local name=$2
  local ws_script="$MOCK_DIR/ws_$name.py"
  
  # Create a simple WebSocket server script
  cat > "$ws_script" << EOF
#!/usr/bin/env python3
import asyncio
import websockets
import json
import time

async def echo(websocket, path):
    print(f"New connection from {websocket.remote_address}")
    try:
        async for message in websocket:
            print(f"Received: {message}")
            response = {
                "service": "$name",
                "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "echo": message
            }
            await websocket.send(json.dumps(response))
    except Exception as e:
        print(f"Error: {e}")
    finally:
        print(f"Connection closed: {websocket.remote_address}")

async def main():
    print("Starting WebSocket server on port $port...")
    async with websockets.serve(echo, "localhost", $port):
        await asyncio.Future()  # Run forever

if __name__ == "__main__":
    asyncio.run(main())
EOF

  # Make the script executable
  chmod +x "$ws_script"
  
  # Start the WebSocket server
  echo "Starting $name WebSocket mock service on port $port..."
  python3 "$ws_script" &
  
  # Wait for the server to start
  sleep 1
  echo "$name WebSocket mock service is running on ws://localhost:$port"
}

# Create regular HTTP mock services
create_mock_service 8081 "api-backend-1"
create_mock_service 8082 "api-backend-2"
create_mock_service 8083 "auth-service"
create_mock_service 8084 "public-api"

# Create WebSocket mock service
#create_ws_mock_service 8085 "ws-backend"

echo "All mock services started. Press Ctrl+C to stop."
wait
