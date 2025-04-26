/// Request to subscribe to configuration updates
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SubscribeRequest {
    /// Data Plane node identifier
    #[prost(string, tag = "1")]
    pub node_id: ::prost::alloc::string::String,
    /// Current configuration version (0 if none)
    #[prost(uint64, tag = "2")]
    pub current_version: u64,
}
/// Request to get a full configuration snapshot
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SnapshotRequest {
    /// Data Plane node identifier
    #[prost(string, tag = "1")]
    pub node_id: ::prost::alloc::string::String,
}
/// Configuration update message sent from CP to DP
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfigUpdate {
    /// Type of update
    #[prost(enumeration = "UpdateType", tag = "1")]
    pub update_type: i32,
    /// Configuration version
    #[prost(uint64, tag = "4")]
    pub version: u64,
    /// Timestamp of this update (ISO8601 string)
    #[prost(string, tag = "5")]
    pub updated_at: ::prost::alloc::string::String,
    /// Full configuration or delta update based on update_type
    #[prost(oneof = "config_update::Update", tags = "2, 3")]
    pub update: ::core::option::Option<config_update::Update>,
}
/// Nested message and enum types in `ConfigUpdate`.
pub mod config_update {
    /// Full configuration or delta update based on update_type
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Update {
        #[prost(message, tag = "2")]
        FullSnapshot(super::ConfigSnapshot),
        #[prost(message, tag = "3")]
        Delta(super::ConfigDelta),
    }
}
/// Full configuration snapshot
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfigSnapshot {
    /// All proxies in the configuration
    #[prost(message, repeated, tag = "1")]
    pub proxies: ::prost::alloc::vec::Vec<Proxy>,
    /// All consumers in the configuration
    #[prost(message, repeated, tag = "2")]
    pub consumers: ::prost::alloc::vec::Vec<Consumer>,
    /// All plugin configurations
    #[prost(message, repeated, tag = "3")]
    pub plugin_configs: ::prost::alloc::vec::Vec<PluginConfig>,
    /// Configuration version
    #[prost(uint64, tag = "4")]
    pub version: u64,
    /// Timestamp of this snapshot (ISO8601 string)
    #[prost(string, tag = "5")]
    pub created_at: ::prost::alloc::string::String,
}
/// Delta configuration update
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfigDelta {
    /// Added or modified proxies
    #[prost(message, repeated, tag = "1")]
    pub upsert_proxies: ::prost::alloc::vec::Vec<Proxy>,
    /// IDs of removed proxies
    #[prost(string, repeated, tag = "2")]
    pub remove_proxy_ids: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// Added or modified consumers
    #[prost(message, repeated, tag = "3")]
    pub upsert_consumers: ::prost::alloc::vec::Vec<Consumer>,
    /// IDs of removed consumers
    #[prost(string, repeated, tag = "4")]
    pub remove_consumer_ids: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// Added or modified plugin configurations
    #[prost(message, repeated, tag = "5")]
    pub upsert_plugin_configs: ::prost::alloc::vec::Vec<PluginConfig>,
    /// IDs of removed plugin configurations
    #[prost(string, repeated, tag = "6")]
    pub remove_plugin_config_ids: ::prost::alloc::vec::Vec<
        ::prost::alloc::string::String,
    >,
}
/// Proxy configuration
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Proxy {
    /// Unique identifier
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Optional name
    #[prost(string, tag = "2")]
    pub name: ::prost::alloc::string::String,
    /// Path prefix for matching requests (MUST be unique)
    #[prost(string, tag = "3")]
    pub listen_path: ::prost::alloc::string::String,
    /// Backend protocol: http, https, ws, wss, grpc
    #[prost(enumeration = "Protocol", tag = "4")]
    pub backend_protocol: i32,
    /// Backend hostname
    #[prost(string, tag = "5")]
    pub backend_host: ::prost::alloc::string::String,
    /// Backend port
    #[prost(uint32, tag = "6")]
    pub backend_port: u32,
    /// Optional path prefix to add to backend requests
    #[prost(string, tag = "7")]
    pub backend_path: ::prost::alloc::string::String,
    /// Whether to strip the listen_path from forwarded requests
    #[prost(bool, tag = "8")]
    pub strip_listen_path: bool,
    /// Whether to preserve the Host header from the client request
    #[prost(bool, tag = "9")]
    pub preserve_host_header: bool,
    /// Connection timeout to backend in milliseconds
    #[prost(uint64, tag = "10")]
    pub backend_connect_timeout_ms: u64,
    /// Read timeout for backend in milliseconds
    #[prost(uint64, tag = "11")]
    pub backend_read_timeout_ms: u64,
    /// Write timeout for backend in milliseconds
    #[prost(uint64, tag = "12")]
    pub backend_write_timeout_ms: u64,
    /// Path to client certificate for mTLS to backend
    #[prost(string, tag = "13")]
    pub backend_tls_client_cert_path: ::prost::alloc::string::String,
    /// Path to client key for mTLS to backend
    #[prost(string, tag = "14")]
    pub backend_tls_client_key_path: ::prost::alloc::string::String,
    /// Whether to verify the backend server certificate
    #[prost(bool, tag = "15")]
    pub backend_tls_verify_server_cert: bool,
    /// Path to CA certificate for verifying the backend server
    #[prost(string, tag = "16")]
    pub backend_tls_server_ca_cert_path: ::prost::alloc::string::String,
    /// Override IP address for DNS resolution
    #[prost(string, tag = "17")]
    pub dns_override: ::prost::alloc::string::String,
    /// TTL for DNS cache entries in seconds (overrides global setting)
    #[prost(uint64, tag = "18")]
    pub dns_cache_ttl_seconds: u64,
    /// Authentication mode: single or multi
    #[prost(enumeration = "AuthMode", tag = "19")]
    pub auth_mode: i32,
    /// Associated plugin configuration IDs
    #[prost(string, repeated, tag = "20")]
    pub plugin_config_ids: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// Creation timestamp (ISO8601 string)
    #[prost(string, tag = "21")]
    pub created_at: ::prost::alloc::string::String,
    /// Last update timestamp (ISO8601 string)
    #[prost(string, tag = "22")]
    pub updated_at: ::prost::alloc::string::String,
}
/// Consumer configuration
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Consumer {
    /// Unique identifier
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Username (unique)
    #[prost(string, tag = "2")]
    pub username: ::prost::alloc::string::String,
    /// Optional custom identifier
    #[prost(string, tag = "3")]
    pub custom_id: ::prost::alloc::string::String,
    /// Credential data (serialized JSON)
    #[prost(string, tag = "4")]
    pub credentials_json: ::prost::alloc::string::String,
    /// Creation timestamp (ISO8601 string)
    #[prost(string, tag = "5")]
    pub created_at: ::prost::alloc::string::String,
    /// Last update timestamp (ISO8601 string)
    #[prost(string, tag = "6")]
    pub updated_at: ::prost::alloc::string::String,
}
/// Plugin configuration
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PluginConfig {
    /// Unique identifier
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// Plugin type name
    #[prost(string, tag = "2")]
    pub plugin_name: ::prost::alloc::string::String,
    /// Plugin configuration (serialized JSON)
    #[prost(string, tag = "3")]
    pub config_json: ::prost::alloc::string::String,
    /// Scope: "global", "proxy", "consumer"
    #[prost(string, tag = "4")]
    pub scope: ::prost::alloc::string::String,
    /// Proxy ID if scope is "proxy"
    #[prost(string, tag = "5")]
    pub proxy_id: ::prost::alloc::string::String,
    /// Consumer ID if scope is "consumer"
    #[prost(string, tag = "6")]
    pub consumer_id: ::prost::alloc::string::String,
    /// Whether the plugin is enabled
    #[prost(bool, tag = "7")]
    pub enabled: bool,
    /// Creation timestamp (ISO8601 string)
    #[prost(string, tag = "8")]
    pub created_at: ::prost::alloc::string::String,
    /// Last update timestamp (ISO8601 string)
    #[prost(string, tag = "9")]
    pub updated_at: ::prost::alloc::string::String,
}
/// Health report from Data Plane to Control Plane
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HealthReport {
    /// Data Plane node identifier
    #[prost(string, tag = "1")]
    pub node_id: ::prost::alloc::string::String,
    /// Timestamp of report (ISO8601 string)
    #[prost(string, tag = "2")]
    pub timestamp: ::prost::alloc::string::String,
    /// Current configuration version
    #[prost(uint64, tag = "3")]
    pub config_version: u64,
    /// System metrics
    #[prost(map = "string, string", tag = "4")]
    pub metrics: ::std::collections::HashMap<
        ::prost::alloc::string::String,
        ::prost::alloc::string::String,
    >,
    /// Node status: "healthy", "degraded", "unhealthy"
    #[prost(string, tag = "5")]
    pub status: ::prost::alloc::string::String,
}
/// Acknowledgment of health report
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HealthAck {
    /// Success status
    #[prost(bool, tag = "1")]
    pub success: bool,
    /// Optional message
    #[prost(string, tag = "2")]
    pub message: ::prost::alloc::string::String,
}
/// Types of configuration updates
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum UpdateType {
    /// Complete replacement of configuration
    Full = 0,
    /// Incremental update (additions, modifications, removals)
    Delta = 1,
}
impl UpdateType {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            UpdateType::Full => "FULL",
            UpdateType::Delta => "DELTA",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "FULL" => Some(Self::Full),
            "DELTA" => Some(Self::Delta),
            _ => None,
        }
    }
}
/// Protocol types for backend connections
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Protocol {
    Http = 0,
    Https = 1,
    Ws = 2,
    Wss = 3,
    Grpc = 4,
}
impl Protocol {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            Protocol::Http => "HTTP",
            Protocol::Https => "HTTPS",
            Protocol::Ws => "WS",
            Protocol::Wss => "WSS",
            Protocol::Grpc => "GRPC",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "HTTP" => Some(Self::Http),
            "HTTPS" => Some(Self::Https),
            "WS" => Some(Self::Ws),
            "WSS" => Some(Self::Wss),
            "GRPC" => Some(Self::Grpc),
            _ => None,
        }
    }
}
/// Authentication modes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum AuthMode {
    Single = 0,
    Multi = 1,
}
impl AuthMode {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            AuthMode::Single => "SINGLE",
            AuthMode::Multi => "MULTI",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "SINGLE" => Some(Self::Single),
            "MULTI" => Some(Self::Multi),
            _ => None,
        }
    }
}
/// Generated client implementations.
pub mod config_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /// Control Plane / Data Plane configuration service
    #[derive(Debug, Clone)]
    pub struct ConfigServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl ConfigServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> ConfigServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> ConfigServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
            >>::Error: Into<StdError> + Send + Sync,
        {
            ConfigServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        /// Subscribe to configuration updates from the Control Plane
        /// Data Plane nodes call this to receive initial and ongoing configuration updates
        pub async fn subscribe_config_updates(
            &mut self,
            request: impl tonic::IntoRequest<super::SubscribeRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::ConfigUpdate>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/ferrumgw.config.ConfigService/SubscribeConfigUpdates",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "ferrumgw.config.ConfigService",
                        "SubscribeConfigUpdates",
                    ),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        /// Get a snapshot of the current configuration from the Control Plane
        /// Used by Data Plane nodes after reconnection to quickly sync state
        pub async fn get_config_snapshot(
            &mut self,
            request: impl tonic::IntoRequest<super::SnapshotRequest>,
        ) -> std::result::Result<tonic::Response<super::ConfigSnapshot>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/ferrumgw.config.ConfigService/GetConfigSnapshot",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("ferrumgw.config.ConfigService", "GetConfigSnapshot"),
                );
            self.inner.unary(req, path, codec).await
        }
        /// Send health status from Data Plane to Control Plane
        pub async fn report_health(
            &mut self,
            request: impl tonic::IntoRequest<super::HealthReport>,
        ) -> std::result::Result<tonic::Response<super::HealthAck>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/ferrumgw.config.ConfigService/ReportHealth",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("ferrumgw.config.ConfigService", "ReportHealth"),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod config_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with ConfigServiceServer.
    #[async_trait]
    pub trait ConfigService: Send + Sync + 'static {
        /// Server streaming response type for the SubscribeConfigUpdates method.
        type SubscribeConfigUpdatesStream: futures_core::Stream<
                Item = std::result::Result<super::ConfigUpdate, tonic::Status>,
            >
            + Send
            + 'static;
        /// Subscribe to configuration updates from the Control Plane
        /// Data Plane nodes call this to receive initial and ongoing configuration updates
        async fn subscribe_config_updates(
            &self,
            request: tonic::Request<super::SubscribeRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::SubscribeConfigUpdatesStream>,
            tonic::Status,
        >;
        /// Get a snapshot of the current configuration from the Control Plane
        /// Used by Data Plane nodes after reconnection to quickly sync state
        async fn get_config_snapshot(
            &self,
            request: tonic::Request<super::SnapshotRequest>,
        ) -> std::result::Result<tonic::Response<super::ConfigSnapshot>, tonic::Status>;
        /// Send health status from Data Plane to Control Plane
        async fn report_health(
            &self,
            request: tonic::Request<super::HealthReport>,
        ) -> std::result::Result<tonic::Response<super::HealthAck>, tonic::Status>;
    }
    /// Control Plane / Data Plane configuration service
    #[derive(Debug)]
    pub struct ConfigServiceServer<T: ConfigService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: ConfigService> ConfigServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for ConfigServiceServer<T>
    where
        T: ConfigService,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/ferrumgw.config.ConfigService/SubscribeConfigUpdates" => {
                    #[allow(non_camel_case_types)]
                    struct SubscribeConfigUpdatesSvc<T: ConfigService>(pub Arc<T>);
                    impl<
                        T: ConfigService,
                    > tonic::server::ServerStreamingService<super::SubscribeRequest>
                    for SubscribeConfigUpdatesSvc<T> {
                        type Response = super::ConfigUpdate;
                        type ResponseStream = T::SubscribeConfigUpdatesStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SubscribeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).subscribe_config_updates(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SubscribeConfigUpdatesSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/ferrumgw.config.ConfigService/GetConfigSnapshot" => {
                    #[allow(non_camel_case_types)]
                    struct GetConfigSnapshotSvc<T: ConfigService>(pub Arc<T>);
                    impl<
                        T: ConfigService,
                    > tonic::server::UnaryService<super::SnapshotRequest>
                    for GetConfigSnapshotSvc<T> {
                        type Response = super::ConfigSnapshot;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SnapshotRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).get_config_snapshot(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetConfigSnapshotSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/ferrumgw.config.ConfigService/ReportHealth" => {
                    #[allow(non_camel_case_types)]
                    struct ReportHealthSvc<T: ConfigService>(pub Arc<T>);
                    impl<
                        T: ConfigService,
                    > tonic::server::UnaryService<super::HealthReport>
                    for ReportHealthSvc<T> {
                        type Response = super::HealthAck;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::HealthReport>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).report_health(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = ReportHealthSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        Ok(
                            http::Response::builder()
                                .status(200)
                                .header("grpc-status", "12")
                                .header("content-type", "application/grpc")
                                .body(empty_body())
                                .unwrap(),
                        )
                    })
                }
            }
        }
    }
    impl<T: ConfigService> Clone for ConfigServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    impl<T: ConfigService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: ConfigService> tonic::server::NamedService for ConfigServiceServer<T> {
        const NAME: &'static str = "ferrumgw.config.ConfigService";
    }
}
