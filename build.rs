fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tell Cargo to rerun this build script if the proto file changes
    println!("cargo:rerun-if-changed=src/grpc/proto/config.proto");
    
    // Configure the protobuf build
    tonic_build::configure()
        .build_server(true)
        .compile(&["src/grpc/proto/config.proto"], &["src/grpc/proto"])?;
    
    Ok(())
}
