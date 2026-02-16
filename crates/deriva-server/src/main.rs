use clap::Parser;
use deriva_compute::async_executor::VerificationMode;
use deriva_compute::builtins;
use deriva_compute::registry::FunctionRegistry;
use deriva_server::service::proto::deriva_server::DerivaServer;
use deriva_server::service::DerivaService;
use deriva_server::state::ServerState;
use deriva_storage::StorageBackend;
use std::sync::Arc;
use tonic::transport::Server;

#[derive(Parser)]
#[command(name = "deriva-server")]
#[command(about = "Deriva computation-addressed storage server")]
struct Args {
    /// Data directory for storage
    #[arg(long, default_value = "/tmp/deriva-data")]
    data_dir: String,

    /// Listen address
    #[arg(long, default_value = "[::1]:50051")]
    listen_addr: String,

    /// Verification mode: "off", "dual", or "sampled:RATE" (e.g., "sampled:0.1")
    #[arg(long, default_value = "off")]
    verification: String,
}

fn parse_verification(s: &str) -> Result<VerificationMode, String> {
    match s {
        "off" => Ok(VerificationMode::Off),
        "dual" => Ok(VerificationMode::DualCompute),
        s if s.starts_with("sampled:") => {
            let rate_str = &s[8..];
            let rate: f64 = rate_str
                .parse()
                .map_err(|_| format!("invalid sample rate: {}", rate_str))?;
            if !(0.0..=1.0).contains(&rate) {
                return Err(format!("sample rate must be between 0.0 and 1.0, got {}", rate));
            }
            Ok(VerificationMode::Sampled { rate })
        }
        _ => Err(format!(
            "invalid verification mode: {}. Expected 'off', 'dual', or 'sampled:RATE'",
            s
        )),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let addr = args.listen_addr.parse()?;

    let verification = parse_verification(&args.verification)
        .map_err(|e| format!("verification argument error: {}", e))?;

    println!("Opening storage at {}", args.data_dir);
    let storage = StorageBackend::open(&args.data_dir)?;

    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    let state = Arc::new(ServerState::with_verification(storage, registry, verification)?);
    let service = DerivaService::new(state);

    println!("Deriva server listening on {}", addr);
    println!("Verification mode: {:?}", verification);
    Server::builder()
        .add_service(DerivaServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
