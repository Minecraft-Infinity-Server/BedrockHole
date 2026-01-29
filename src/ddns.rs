mod cloudflare;

use std::sync::LazyLock;

use async_trait::async_trait;
use tokio::sync::OnceCell;

use crate::config::{DDNSConfig, DDNSProvider};

pub static PROVIDER: OnceCell<Box<dyn DynamicDns + Send + Sync>> = OnceCell::const_new();
pub static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to create HTTP client")
});

#[async_trait]
pub trait DynamicDns {
    async fn update_srv(&self, host: &str, port: u16) -> anyhow::Result<()>;
}

pub fn init(config: DDNSConfig) -> anyhow::Result<()> {
    let provider = match config.provider {
        DDNSProvider::Cloudflare => cloudflare::Provider::new(config),
    };

    let _ = PROVIDER.set(Box::new(provider));

    Ok(())
}
