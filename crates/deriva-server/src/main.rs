use clap::Parser;
use deriva_compute::async_executor::VerificationMode;
use deriva_compute::builtins;
use deriva_compute::registry::FunctionRegistry;
use deriva_server::metrics_server;
use deriva_server::service::proto::deriva_server::DerivaServer;
use deriva_server::service::DerivaService;
use deriva_server::state::ServerState;
use deriva_storage::StorageBackend;
use std::sync::Arc;
use tonic::transport::Server;
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

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

    /// Metrics server port (0 to disable)
    #[arg(long, default_value = "9090")]
    metrics_port: u16,

    /// Log format: "text" or "json"
    #[arg(long, default_value = "text")]
    log_format: String,
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

fn init_tracing(json: bool) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("deriva=info"));

    if json {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json()
                .with_target(true)
                .with_span_events(fmt::format::FmtSpan::CLOSE))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer()
                .with_target(true)
                .with_span_events(fmt::format::FmtSpan::CLOSE)
                .with_timer(fmt::time::uptime()))
            .init();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    init_tracing(args.log_format == "json");

    let addr = args.listen_addr.parse()?;
    let verification = parse_verification(&args.verification)
        .map_err(|e| format!("verification argument error: {}", e))?;

    tracing::info!("Opening storage at {}", args.data_dir);
    let storage = StorageBackend::open(&args.data_dir)?;

    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    let state = Arc::new(ServerState::with_verification(storage, registry, verification)?);
    let service = DerivaService::new(Arc::clone(&state));

    if args.metrics_port > 0 {
        let metrics_state = Arc::clone(&state);
        tokio::spawn(metrics_server::start_metrics_server(args.metrics_port, metrics_state));
    }

    tracing::info!("Deriva server listening on {}", addr);
    tracing::info!("Verification mode: {:?}", verification);
    Server::builder()
        .add_service(DerivaServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
