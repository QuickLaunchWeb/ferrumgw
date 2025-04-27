# HTTP Protocol Support in Ferrum Gateway

Ferrum Gateway now fully supports modern HTTP protocols, providing enhanced performance, security, and compatibility with various clients.

## Supported Protocols

### HTTP/1.1
- Fully supported on both HTTP and HTTPS endpoints
- Keep-alive connections enabled by default
- Efficiently handles traditional HTTP traffic

### HTTP/2
- Fully supported with and without TLS
- Features:
  - Multiplexing (multiple requests over a single connection)
  - Header compression
  - Server push capabilities
  - Binary framing for improved performance
- Automatically negotiated via ALPN when using TLS

### HTTP/3
- Implemented using QUIC protocol (over UDP)
- Features:
  - Improved connection migration and resilience
  - Built-in TLS 1.3 security
  - Reduced connection establishment latency
  - Elimination of head-of-line blocking
  - Independent streams for improved parallelism

## Configuration

Enable each protocol endpoint using environment variables:

```
# HTTP/1.1 and HTTP/2 over TCP
FERRUM_PROXY_HTTP_PORT=8000

# HTTP/1.1 and HTTP/2 over TLS
FERRUM_PROXY_HTTPS_PORT=8443
FERRUM_PROXY_TLS_CERT_PATH=/path/to/cert.pem
FERRUM_PROXY_TLS_KEY_PATH=/path/to/key.pem

# HTTP/3 over QUIC/UDP (requires TLS)
FERRUM_PROXY_HTTP3_PORT=8444
FERRUM_PROXY_TLS_CERT_PATH=/path/to/cert.pem
FERRUM_PROXY_TLS_KEY_PATH=/path/to/key.pem
```

## Protocol Selection

The gateway handles protocol selection as follows:

1. For unencrypted traffic on HTTP port:
   - Clients can use either HTTP/1.1 or HTTP/2 (h2c)
   - Protocol selection is performed automatically via the connection preface

2. For TLS-encrypted traffic on HTTPS port:
   - Protocol is negotiated using ALPN (Application-Layer Protocol Negotiation)
   - HTTP/2 is preferred over HTTP/1.1 when both are supported by the client

3. For HTTP/3:
   - Operates on a separate UDP port
   - Uses QUIC transport protocol with built-in TLS 1.3
   - Can be discovered via Alt-Svc header from responses on HTTP/1.1 or HTTP/2 connections

## Benefits

- **Improved Performance**: HTTP/2 and HTTP/3 provide significant performance improvements over HTTP/1.1
- **Protocol Flexibility**: Support for all mainstream HTTP protocols ensures compatibility with all client types
- **Future-Proofing**: HTTP/3 support prepares the gateway for the evolving web landscape

## Implementation Notes

- All protocols share the same routing and plugin infrastructure
- The gateway seamlessly handles protocol-specific details while maintaining a consistent API
- Request and response processing is protocol-agnostic, ensuring consistent behavior across all protocols
