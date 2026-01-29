use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::anyhow;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional},
    net::{TcpSocket, TcpStream, lookup_host},
};

use crate::{
    config::{ForwardConfig, GeneralConfig},
    ddns::PROVIDER,
};

async fn forward(
    mut client_stream: TcpStream,
    server: SocketAddr,
    haproxy: bool,
) -> anyhow::Result<()> {
    let mut server_stream = TcpStream::connect(server).await?;

    if haproxy {
        let client_addr = client_stream.peer_addr()?;
        let server_local_addr = server_stream.local_addr()?;

        let header = match (client_addr, server_local_addr) {
            (SocketAddr::V4(src), SocketAddr::V4(dst)) => {
                format!(
                    "PROXY TCP4 {} {} {} {}\r\n",
                    src.ip(),
                    dst.ip(),
                    src.port(),
                    dst.port()
                )
            }
            (SocketAddr::V6(src), SocketAddr::V6(dst)) => {
                format!(
                    "PROXY TCP6 {} {} {} {}\r\n",
                    src.ip(),
                    dst.ip(),
                    src.port(),
                    dst.port()
                )
            }
            _ => return Err(anyhow::anyhow!("Mismatched IP families for PROXY v1")),
        };

        server_stream.write_all(header.as_bytes()).await?;
    }

    tokio::io::copy_bidirectional(&mut client_stream, &mut server_stream).await?;

    Ok(())
}

#[allow(dead_code)]
async fn forward_v2(
    mut client_stream: TcpStream,
    server: SocketAddr,
    haproxy: bool,
) -> anyhow::Result<()> {
    let mut server_stream = TcpStream::connect(server).await?;

    if haproxy {
        let client_addr = client_stream.peer_addr()?;
        let server_local_addr = server_stream.local_addr()?;

        let signature = [
            0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A,
        ];

        let mut header = Vec::with_capacity(64);
        header.extend_from_slice(&signature);

        match (client_addr, server_local_addr) {
            (SocketAddr::V4(src), SocketAddr::V4(dst)) => {
                header.extend_from_slice(&[0x21, 0x11]);
                header.extend_from_slice(&12u16.to_be_bytes());
                header.extend_from_slice(&src.ip().octets());
                header.extend_from_slice(&dst.ip().octets());
                header.extend_from_slice(&src.port().to_be_bytes());
                header.extend_from_slice(&dst.port().to_be_bytes());
            }
            (SocketAddr::V6(src), SocketAddr::V6(dst)) => {
                header.extend_from_slice(&[0x21, 0x21]);
                header.extend_from_slice(&36u16.to_be_bytes());
                header.extend_from_slice(&src.ip().octets());
                header.extend_from_slice(&dst.ip().octets());
                header.extend_from_slice(&src.port().to_be_bytes());
                header.extend_from_slice(&dst.port().to_be_bytes());
            }
            _ => return Err(anyhow::anyhow!("Mismatched IP families for PROXY v2")),
        }

        server_stream.write_all(&header).await?;
    }

    copy_bidirectional(&mut client_stream, &mut server_stream).await?;

    Ok(())
}

async fn register_listener(config: ForwardConfig) -> anyhow::Result<()> {
    let socket = TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    #[cfg(unix)]
    socket.set_reuseport(true)?;
    socket.set_nodelay(true)?;

    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), config.local_port);
    socket.bind(local_addr)?;

    tracing::info!("Bind addr: {}:{}", local_addr.ip(), local_addr.port());
    let listener = socket.listen(1024)?;

    let server_addr: SocketAddr =
        format!("{}:{}", config.server_host, config.server_port).parse()?;

    tracing::info!("Register forward worker.");
    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                tracing::info!("New connection from: {}", addr);

                tokio::spawn(async move {
                    if let Err(e) =
                        forward(client_stream, server_addr, config.haproxy_support).await
                    {
                        tracing::error!("Proxy session error: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::error!("Accept failed: {}", e);

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

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

    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), client_port);
    socket.bind(local_addr)?;

    let stream =
        tokio::time::timeout(std::time::Duration::from_secs(3), socket.connect(server)).await??;

    let actual_local = stream.local_addr()?;
    tracing::info!("Local socket bound to: {}", actual_local);

    Ok(stream)
}

fn stun_loop(config: GeneralConfig, client_port: u16) -> anyhow::Result<()> {
    tokio::spawn(async move {
        let mut last_addr: Option<SocketAddr> = None;
        let mut reconn = false;
        let server_addr = lookup_host(format!(
            "{}:{}",
            config.stun_server_host, config.stun_server_port
        ))
        .await?
        .filter(|ip| ip.is_ipv4())
        .next()
        .ok_or_else(|| {
            tracing::error!("Unable to find a valid IPv4 address.");

            std::process::exit(1);
        })?;

        let mut stream = stun_connect(server_addr, client_port).await?;
        loop {
            if let Err(e) = async {
                if reconn {
                    stream = stun_connect(server_addr, client_port).await?;
                    reconn = false;
                }
                let mut request = [0u8; 20];
                request[0..2].copy_from_slice(&0x0001u16.to_be_bytes());
                request[4..8].copy_from_slice(&0x2112A442u32.to_be_bytes());
                request[8..20].copy_from_slice(&[0xAA; 12]);

                stream.write_all(&request).await?;

                let mut response = [0u8; 1024];
                let _ = stream.read(&mut response).await?;

                let addr = parse_addr(&response)?;

                if Some(addr) != last_addr {
                    let host = addr.ip();
                    let port = addr.port();
                    tracing::info!(
                        "Detected a change in public network addresss: {}:{}",
                        host,
                        port
                    );
                    if let Err(e) = PROVIDER
                        .get()
                        .unwrap()
                        .update_srv(&host.to_string(), port)
                        .await
                    {
                        tracing::error!("An error occurred while updating the SRV record: {}.", e);
                    } else {
                        last_addr = Some(addr);
                    }
                } else {
                    tracing::info!("Heartbeat packet sent.");
                }

                tokio::time::sleep(std::time::Duration::from_secs(config.heartbeat as u64)).await;

                Ok::<(), anyhow::Error>(())
            }
            .await
            {
                reconn = true;
                tracing::error!("{}", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }

        #[allow(unused)]
        Ok::<(), anyhow::Error>(())
    });

    tracing::info!("Register stun worker.");
    Ok(())
}

pub async fn run(general: GeneralConfig, forward: ForwardConfig) -> anyhow::Result<()> {
    stun_loop(general, forward.local_port)?;
    register_listener(forward).await?;
    Ok(())
}
