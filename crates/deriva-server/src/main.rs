use deriva_compute::builtins;
use deriva_compute::registry::FunctionRegistry;
use deriva_server::service::proto::deriva_server::DerivaServer;
use deriva_server::service::DerivaService;
use deriva_server::state::ServerState;
use deriva_storage::StorageBackend;
use std::sync::Arc;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var("DERIVA_DATA_DIR").unwrap_or_else(|_| "/tmp/deriva-data".into());
    let addr = std::env::var("DERIVA_LISTEN_ADDR")
        .unwrap_or_else(|_| "[::1]:50051".into())
        .parse()?;

    println!("Opening storage at {}", data_dir);
    let storage = StorageBackend::open(&data_dir)?;

    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    let state = Arc::new(ServerState::new(storage, registry)?);
    let service = DerivaService::new(state);

    println!("Deriva server listening on {}", addr);
    Server::builder()
        .add_service(DerivaServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
