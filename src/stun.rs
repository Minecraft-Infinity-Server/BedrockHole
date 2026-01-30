use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::anyhow;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream, lookup_host},
};

use crate::{WAN_ADDR, config::GeneralConfig, ddns::PROVIDER};

const STUN_MAGIC_COOKIE: u32 = 0x2112A442;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;

fn parse_addr(buf: &[u8]) -> anyhow::Result<SocketAddr> {
    if buf.len() < 20 {
        return Err(anyhow!("Mismatched message length."));
    }

    let mut pos = 20;
    while pos + 4 <= buf.len() {
        let attr_type = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let attr_len = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]) as usize;
        pos += 4;

        if attr_type == ATTR_XOR_MAPPED_ADDRESS {
            let x_port = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]);
            let x_ip = [buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]];

            let port = x_port ^ (STUN_MAGIC_COOKIE >> 16) as u16;

            let mc_bytes = STUN_MAGIC_COOKIE.to_be_bytes();
            let ip = Ipv4Addr::new(
                x_ip[0] ^ mc_bytes[0],
                x_ip[1] ^ mc_bytes[1],
                x_ip[2] ^ mc_bytes[2],
                x_ip[3] ^ mc_bytes[3],
            );
            return Ok(SocketAddr::new(IpAddr::V4(ip), port));
        }
        pos += attr_len;
    }

    Err(anyhow!("XOR-MAPPED-ADDRESS attribute not found。"))
}

async fn stun_connect(server: SocketAddr, client_port: u16) -> anyhow::Result<TcpStream> {
    let socket = TcpSocket::new_v4()?;

    socket.set_reuseaddr(true)?;
    #[cfg(unix)]
    socket.set_reuseport(true)?;
    socket.set_nodelay(true)?;
    socket.set_keepalive(true)?;

    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), client_port);
    socket.bind(local_addr)?;

    let stream =
        tokio::time::timeout(std::time::Duration::from_secs(3), socket.connect(server)).await??;

    Ok(stream)
}

async fn get_addr(config: GeneralConfig, local_port: u16) -> anyhow::Result<SocketAddr> {
    let server_addr = loop {
        match lookup_host(format!(
            "{}:{}",
            config.stun_server_host, config.stun_server_port
        ))
        .await
        {
            Ok(mut addrs) => {
                if let Some(addr) = addrs.find(|ip| ip.is_ipv4()) {
                    break addr;
                }
            }
            Err(e) => tracing::warn!("DNS lookup failed: {}, retrying...", e),
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    };

    tracing::info!("Register stun worker.");

    let mut stream = loop {
        match stun_connect(server_addr, local_port).await {
            Ok(s) => {
                tracing::info!("Successfully connected to STUN server.");
                break s;
            }
            Err(e) => {
                tracing::error!("Failed to connect to STUN server: {}, retrying in 5s...", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    };

    let mut request = [0u8; 20];
    request[0..2].copy_from_slice(&0x0001u16.to_be_bytes());
    request[4..8].copy_from_slice(&0x2112A442u32.to_be_bytes());
    request[8..20].copy_from_slice(&[0xAA; 12]);

    stream.write_all(&request).await?;

    let mut response = [0u8; 1024];
    let _ = stream.read(&mut response).await?;

    let addr = parse_addr(&response)?;
    let host = addr.ip();
    let port = addr.port();

    tracing::info!("Public addr: {}", addr);

    loop {
        match PROVIDER
            .get()
            .unwrap()
            .update_srv(&host.to_string(), port)
            .await
        {
            Ok(()) => break,
            Err(e) => {
                tracing::error!(
                    "An error occurred while updating the SRV record: {}, retrying in 5s...",
                    e
                );
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    Ok(addr)
}

async fn heartbeat_loop(addr: SocketAddr, heartbeat: u64) -> anyhow::Result<()> {
    async fn conn(addr: SocketAddr) -> anyhow::Result<TcpStream> {
        let socket = TcpSocket::new_v4()?;
        socket.set_keepalive(true)?;

        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(5), socket.connect(addr)).await??;

        Ok(stream)
    }
    let mut stream = conn(addr)
        .await
        .map_err(|e| anyhow!("Initial connect failed: {}", e))?;
    tracing::info!("Successfully connected to heartbeat server.");

    let timeout = std::time::Duration::from_secs(heartbeat);
    let io_timeout = std::time::Duration::from_secs(5);
    let data = b"hbpk";
    let expected_resp = b"hbre";

    loop {
        let res: anyhow::Result<()> = async {
            tokio::time::timeout(io_timeout, stream.write_all(data)).await??;

            let mut buf = [0u8; 64];
            let n = tokio::time::timeout(io_timeout, stream.read(&mut buf)).await??;

            if n == 0 {
                return Err(anyhow::anyhow!("Connection closed by remote peer"));
            }

            if n < expected_resp.len() || &buf[..expected_resp.len()] != expected_resp {
                return Err(anyhow::anyhow!(
                    "Invalid heartbeat response: {:?}",
                    &buf[..n]
                ));
            }

            tracing::info!("Heartbeat packet sent.");
            tokio::time::sleep(timeout).await;

            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = res {
            tracing::error!("Heartbeat error: {}. Retrying in 5s...", e);
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            return Err(e);
        }
    }
}

pub async fn run(config: GeneralConfig, local_port: u16) {
    let config_cloned = config.clone();
    let heartbeat = config.heartbeat;
    let mut wan_addr = get_addr(config, local_port).await.unwrap_or_else(|e| {
        tracing::error!("{:?}", e);

        std::process::exit(1);
    });

    {
        let mut wa = WAN_ADDR.get().unwrap().write().await;
        *wa = wan_addr;
    }

    tokio::spawn(async move {
        let mut retries = 0;
        loop {
            if retries >= 3 {
                match get_addr(config_cloned.clone(), local_port).await {
                    Ok(new_addr) => {
                        wan_addr = new_addr;
                        retries = 0;

                        {
                            let mut wa = WAN_ADDR.get().unwrap().write().await;
                            *wa = wan_addr;
                        }

                        tracing::info!("Global WAN address synchronized: {}", new_addr);
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to re-fetch WAN address: {}, retrying in 10s...",
                            e
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        continue;
                    }
                }
            }
            if let Err(e) = heartbeat_loop(wan_addr, heartbeat).await {
                tracing::error!(
                    "Heartbeat session ended: {}. Retry count: {}",
                    e,
                    retries + 1
                );
                retries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            } else {
                retries = 0;
            }
        }
    });
}
