use std::net::SocketAddr;

use chrono::Local;
use tokio::sync::{OnceCell, RwLock};
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

pub static WAN_ADDR: OnceCell<RwLock<SocketAddr>> = OnceCell::const_new();

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_timer(LocalTime).init();
    WAN_ADDR
        .set(RwLock::new(format!("0.0.0.0:0").parse().unwrap()))
        .unwrap();

    let config = config::BHConfig::_default_load().unwrap_or_else(|e| {
        tracing::error!(error = %e, "Failed to load configuration file");
        std::process::exit(1);
    });

    ddns::init(config.ddns).unwrap_or_else(|e| {
        tracing::error!(error = %e, "Failed to initialize DDNS provider");
        std::process::exit(1);
    });

    tracing::info!("Starting Bedrock-Hole core services...");

    stun::run(config.general, config.forward.local_port).await;

    forward::run(config.forward).await.unwrap_or_else(|e| {
        tracing::error!(error = %e, "Core service execution failed");
        std::process::exit(1);
    });
}
