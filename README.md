# BedrockHole

**BedrockHole** is a lightweight, high-performance network tunneling and port-forwarding tool designed to bridge the gap between isolated NAT1 (Full Cone NAT) internal networks and the public internet. It enables users to host publicly accessible Minecraft servers (or other TCP services) directly from their home machines without requiring a dedicated public IP or expensive relay servers.

------

## Getting Started

Setting up **BedrockHole** is straightforward. Follow these steps to get your server punched through the NAT and onto the internet.

------

### 1. Configuration

Before running the application, you need to configure your environment. Create a file named `config.json` and place it in the same directory as the `bedrock-hole` binary.

> **Note:** Ensure your router supports **NAT1 (Full Cone NAT)** for the STUN traversal to work correctly.

### 2. Execution

Open your terminal, navigate to the folder containing the binary, and execute the following command:

```bash
# On Linux or macOS
chmod +x bedrock-hole
./bedrock-hole
# On Windows
.\bedrock-hole.exe
```

### 3. Verification

Once executed, monitor the console output. You should see logs indicating the service initialization, STUN detection, and DNS synchronization.

**Successful startup example:**

```Plaintext
2026-01-29 12:17:11.410766  INFO bedrock_hole: Starting Bedrock-Hole core services...
2026-01-29 12:17:11.410838  INFO bedrock_hole::stun: Register stun worker.
2026-01-29 12:17:11.411022  INFO bedrock_hole::stun: Listening on [::]:23333 (IPv6) -> Target: [::1]:25565
2026-01-29 12:17:11.411044  INFO bedrock_hole::stun: Register IPv6 forward worker.
2026-01-29 12:17:11.768724  INFO bedrock_hole::stun: Detected a change in public network address: 1.1.1.1:57785
2026-01-29 12:17:11.768893  INFO bedrock_hole::ddns::cloudflare: Starting Cloudflare DNS synchronization domain=example.com sub_domain=v4
2026-01-29 12:17:13.306958  INFO bedrock_hole::ddns::cloudflare: Cloudflare record synchronization successful action=PATCH rectype=A name=v4.example.com content=1.1.1.1
2026-01-29 12:17:14.051646  INFO bedrock_hole::ddns::cloudflare: Cloudflare record synchronization successful action=PATCH rectype=SRV name=_minecraft._tcp.v4.example.com content=v4.example.com
```

If you see the **"Cloudflare record synchronization successful"** message, your server is now accessible via your domain! Players can connect using your configured hostname without needing to worry about the port.

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