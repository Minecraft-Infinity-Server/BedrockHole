# BedrockHole

**BedrockHole** is a lightweight, high-performance network tunneling and port-forwarding tool designed to bridge the gap between isolated NAT1 (Full Cone NAT) internal networks and the public internet. It enables users to host publicly accessible Minecraft servers (or other TCP services) directly from their home machines without requiring a dedicated public IP or expensive relay servers.

------

## 🚀 Key Functionalities

- **STUN Traversal & Hole Punching**: Automatically detects public IP and port mappings in NAT1 environments using the STUN protocol.
- **Automated DDNS**: Synchronizes public network changes to Cloudflare by updating **A records** and **SRV records** (perfect for Minecraft's non-standard ports).
- **High-Performance Forwarding**: Handles traffic redirection with minimal latency and overhead.
- **Real IP Transparency**: Injects **HAProxy Proxy Protocol (v1/v2)** headers into forwarded packets, allowing backend servers (using `HAProxyDetector`) to identify the original client's IP address.

------

## 🛠 The Workflow

`BedrockHole` operates through a sophisticated, automated pipeline:

1. **Probe**: It communicates with a STUN server to identify the public-facing IP and the specific port mapped by your NAT1 router.
2. **Broadcast**: It pushes the identified IP to a Cloudflare **A record** and the mapped port to a **SRV record** (e.g., `_minecraft._tcp.example.com`). This allows players to connect via a domain name without manually entering changing ports.
3. **Listen**: It initializes a high-performance listener on the local machine to capture incoming traffic from the "punched" hole.
4. **Inject & Forward**: As traffic flows through, `BedrockHole` wraps the data with HAProxy headers before passing it to the target server, ensuring accurate logging and security filtering on the backend.

------

## ✨ Features & Specifications

- **Platform Support**: Fully compatible with **Linux** (including OpenWrt, Ubuntu) and **macOS**.
- **Architectures**: Supports both **x86_64** and **ARM** (Apple Silicon/Raspberry Pi).
- **Efficiency**: Written in **Rust**, offering extreme memory safety and low CPU/RAM footprint.
- **Scalability**: Built on the `Tokio` asynchronous runtime to handle numerous concurrent connections smoothly.

------

## 🌐 Use Case

> "I have a powerful Mac Mini or a Proxmox VM at home, but my ISP only provides a NAT1 environment. I want my friends to join my Minecraft server using a domain name, and I want to see their real IP addresses in my server logs for moderation."

If this sounds like your situation, **BedrockHole** is the definitive solution.