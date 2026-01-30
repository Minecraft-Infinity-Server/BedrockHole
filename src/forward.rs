use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::anyhow;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional},
    net::{TcpListener, TcpSocket, TcpStream, lookup_host},
};

use crate::config::{ForwardConfig, HAProxyVersion};

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

async fn listener_handle(
    wan_host: IpAddr,
    listener: TcpListener,
    server_addr: SocketAddr,
    haproxy: bool,
    haproxy_version: HAProxyVersion,
    protocol: &str,
) {
    tracing::info!("Register {} forward worker.", protocol);
    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                // heartbeat server
                if addr.ip().to_canonical() == wan_host {
                    let mut buf = [0u8; 4];
                    match client_stream.peek(&mut buf).await {
                        Ok(n) if n >= 4 && &buf == b"hbpk" => {
                            tokio::spawn(heartbeat_server(client_stream));
                            continue;
                        }
                        _ => {
                            tracing::info!(
                                "Internal redirection: Loopback connection from player at {}",
                                addr
                            );
                        }
                    }
                }

                tracing::info!("New connection from: {}", addr);
                tokio::spawn(async move {
                    if let Err(e) = match haproxy_version {
                        HAProxyVersion::V1 => forward(client_stream, server_addr, haproxy).await,
                        HAProxyVersion::V2 => forward_v2(client_stream, server_addr, haproxy).await,
                    } {
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

async fn heartbeat_server(mut stream: TcpStream) {
    let mut buf = [0u8; 64];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) => {
                tracing::info!("Heartbeat client disconnected.");
                break;
            }
            Ok(n) => {
                if &buf[..n] == b"hbpk" {
                    if let Err(e) = stream.write_all(b"hbre").await {
                        tracing::error!("Failed to send response to heartbeat client: {}", e);
                        break;
                    }
                } else {
                    tracing::warn!(
                        "Received unknown data from heartbeat client: {:?}",
                        &buf[..n]
                    );
                }
            }
            Err(e) => {
                tracing::error!("Read error from heartbeat client: {}", e);
                break;
            }
        }
    }
}

pub async fn run(config: ForwardConfig, wan_host: IpAddr) -> anyhow::Result<()> {
    let host_with_port = format!("{}:{}", config.server_host, config.server_port);

    let ipv6_res = async {
        let mut server_addr = lookup_host(&host_with_port)
            .await?
            .find(|addr| addr.is_ipv6())
            .ok_or_else(|| anyhow!("No IPv6 found"))?;
        server_addr.set_port(config.server_port);

        let socket = TcpSocket::new_v6()?;
        let local_addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), config.local_port);
        socket.set_reuseaddr(true)?;
        #[cfg(unix)]
        socket.set_reuseport(true)?;
        socket.set_nodelay(true)?;
        socket.bind(local_addr)?;
        let listener = socket.listen(1024)?;

        tracing::info!(
            "Listening on [::]:{} (IPv6) -> Target: {}",
            config.local_port,
            server_addr
        );
        listener_handle(
            wan_host,
            listener,
            server_addr,
            config.haproxy_support,
            config.haproxy_version,
            "IPv6",
        )
        .await;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(e) = ipv6_res {
        tracing::warn!("IPv6 setup failed: {}. Falling back to IPv4...", e);

        let mut server_addr = lookup_host(&host_with_port)
            .await?
            .find(|addr| addr.is_ipv4())
            .ok_or_else(|| anyhow!("No IPv4 found"))?;
        server_addr.set_port(config.server_port);

        let socket = TcpSocket::new_v4()?;
        let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), config.local_port);
        socket.set_reuseaddr(true)?;
        #[cfg(unix)]
        socket.set_reuseport(true)?;
        socket.set_nodelay(true)?;
        socket.bind(local_addr)?;
        let listener = socket.listen(1024)?;

        tracing::info!(
            "Listening on 0.0.0.0:{} (IPv4) -> Target: {}",
            config.local_port,
            server_addr
        );
        listener_handle(
            wan_host,
            listener,
            server_addr,
            config.haproxy_support,
            config.haproxy_version,
            "IPv4",
        )
        .await;
    }

    Ok(())
}
