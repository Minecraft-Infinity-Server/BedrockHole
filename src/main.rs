use chrono::Local;
use tracing_subscriber::fmt::{format::Writer, time::FormatTime};

mod config;
mod ddns;
mod forward;
mod stun;

struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", Local::now().format("%Y-%m-%d %H:%M:%S%.6f"))
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_timer(LocalTime).init();

    let config = config::BHConfig::_default_load().unwrap_or_else(|e| {
        tracing::error!(error = %e, "Failed to load configuration file");
        std::process::exit(1);
    });

    ddns::init(config.ddns).unwrap_or_else(|e| {
        tracing::error!(error = %e, "Failed to initialize DDNS provider");
        std::process::exit(1);
    });

    tracing::info!("Starting Bedrock-Hole core services...");

    let wan_addr = stun::run(config.general, config.forward.local_port).await;

    forward::run(config.forward, wan_addr.ip())
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "Core service execution failed");
            std::process::exit(1);
        });
}
