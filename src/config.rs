use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DDNSProvider {
    Cloudflare,
}

#[derive(Serialize, Deserialize, Copy, Clone)]
#[serde(rename_all = "lowercase")]
pub enum HAProxyVersion {
    V1,
    V2
}

#[derive(Serialize, Deserialize)]
pub struct DDNSConfig {
    pub provider: DDNSProvider,
    pub token: String,
    pub domain: String,
    pub sub_domain: String,
}

#[derive(Serialize, Deserialize)]
pub struct ForwardConfig {
    pub local_port: u16,
    pub server_host: String,
    pub server_port: u16,
    pub haproxy_support: bool,
    pub haproxy_version: HAProxyVersion
}

#[derive(Serialize, Deserialize)]
pub struct GeneralConfig {
    pub heartbeat: u64,
    pub keep_alive: bool,
    pub stun_server_host: String,
    pub stun_server_port: u16,
}

#[derive(Serialize, Deserialize)]
pub struct BHConfig {
    pub ddns: DDNSConfig,
    pub forward: ForwardConfig,
    pub general: GeneralConfig,
}

impl BHConfig {
    pub fn load_from_path(path: &PathBuf) -> anyhow::Result<Self> {
        let buf = fs::read(path)?;

        let res = serde_json::from_slice(&buf)?;

        Ok(res)
    }

    pub fn _default_load() -> anyhow::Result<Self> {
        let path = std::env::current_dir()?.join("config.json");

        Self::load_from_path(&path)
    }
}
