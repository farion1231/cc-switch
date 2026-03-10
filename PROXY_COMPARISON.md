# CC-Switch Proxy vs Litebike/Literbike Analysis

## Executive Summary

**CC-Switch** and **Litebike/Literbike** are complementary projects with different focuses:

| Aspect | CC-Switch | Litebike/Literbike |
|--------|-----------|-------------------|
| **Primary Purpose** | AI CLI provider management & failover proxy | Network proxy + system utilities + protocol detection |
| **Proxy Focus** | HTTP/HTTPS for LLM APIs (Anthropic, OpenAI, Gemini) | Multi-protocol (HTTP, SOCKS5, TLS, QUIC, WebRTC, etc.) |
| **Key Feature** | Provider failover, circuit breaker, model routing | Protocol auto-detection, QUIC support, TLS fingerprinting |
| **Network Layer** | TCP only | TCP + UDP + QUIC |
| **Protocol Detection** | None (assumes HTTP) | ✅ Auto-detect from bytes |
| **Model/Provider Management** | ✅ Full CRUD + UI | ❌ None |

---

## Architecture Comparison

### CC-Switch Proxy Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   CC-Switch Proxy                        │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  Provider    │  │   Circuit    │  │   Failover   │  │
│  │   Router     │  │   Breaker    │  │   Manager    │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   Health     │  │    Model     │  │   Handler    │  │
│  │   Checker    │  │   Mapper     │  │   (Axum)     │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
│                                                          │
│  Protocol: HTTP/HTTPS only (LLM APIs)                   │
│  Port: 15721 (default)                                  │
│  Backend: Axum web framework                            │
└─────────────────────────────────────────────────────────┘
```

**Strengths**:
- ✅ Provider failover with circuit breaker
- ✅ Health checking per provider
- ✅ Model family routing (Haiku/Sonnet/Opus)
- ✅ Tauri integration (system tray, notifications)
- ✅ SQLite database for persistence
- ✅ UI for configuration

**Weaknesses**:
- ❌ HTTP/HTTPS only (no QUIC, WebSocket, etc.)
- ❌ No protocol auto-detection
- ❌ No TLS fingerprinting
- ❌ No UDP support
- ❌ Single-port, single-protocol

---

### Litebike/Literbike Architecture

```
┌─────────────────────────────────────────────────────────┐
│               Litebike/Literbike                         │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  Universal   │  │   Protocol   │  │   QUIC       │  │
│  │  Listener    │  │  Registry    │  │  Stack       │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │     TLS      │  │   Bonjour    │  │     UPnP     │  │
│  │ Fingerprint  │  │  Discovery   │  │  Discovery   │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
│                                                          │
│  Protocols: HTTP, HTTPS, SOCKS5, QUIC, WebRTC, TLS, ... │
│  Port: 8888 (default, multi-protocol)                   │
│  Backend: Tokio + custom protocol handlers              │
└─────────────────────────────────────────────────────────┘
```

**Strengths**:
- ✅ Multi-protocol support (30+ protocols)
- ✅ Protocol auto-detection from bytes
- ✅ QUIC support (RFC 9000 varint encoding)
- ✅ TLS fingerprinting (JA3-style mobile browser profiles)
- ✅ Bonjour/mDNS auto-discovery
- ✅ UPnP port forwarding
- ✅ POSIX socket peek for non-destructive detection
- ✅ PrefixedStream for protocol handoff

**Weaknesses**:
- ❌ No provider management
- ❌ No failover logic
- ❌ No circuit breaker
- ❌ No UI (CLI only)
- ❌ No persistence layer

---

## Feature Matrix

| Feature | CC-Switch | Litebike | Literbike | Integration Priority |
|---------|-----------|----------|-----------|---------------------|
| **HTTP Proxy** | ✅ | ✅ | ✅ | N/A (both have) |
| **HTTPS Proxy** | ✅ | ✅ | ✅ | N/A (both have) |
| **SOCKS5** | ❌ | ✅ | ✅ | 🔴 HIGH |
| **QUIC** | ❌ | ✅ (basic) | ✅ | 🔴 HIGH |
| **WebSocket** | ❌ | ✅ | ✅ | 🟡 MEDIUM |
| **WebRTC/STUN** | ❌ | ❌ | ✅ | 🟢 LOW |
| **Protocol Detection** | ❌ | ✅ | ✅ | 🔴 HIGH |
| **TLS Fingerprinting** | ❌ | ❌ | ✅ | 🟡 MEDIUM |
| **Bonjour/mDNS** | ❌ | ✅ | ✅ | 🟢 LOW |
| **UPnP** | ❌ | ✅ | ✅ | 🟢 LOW |
| **Provider CRUD** | ✅ | ❌ | ❌ | N/A (CC-Switch only) |
| **Failover** | ✅ | ❌ | ❌ | N/A (CC-Switch only) |
| **Circuit Breaker** | ✅ | ❌ | ❌ | N/A (CC-Switch only) |
| **Model Routing** | ✅ | ❌ | ❌ | N/A (CC-Switch only) |
| **Tauri Integration** | ✅ | ❌ | ❌ | N/A (CC-Switch only) |
| **SQLite Database** | ✅ | ❌ | ❌ | N/A (CC-Switch only) |

---

## Code Comparison: Proxy Handlers

### CC-Switch (Axum-based)

```rust
// src-tauri/src/proxy/handlers.rs
pub async fn proxy_handler(
    State(state): State<ProxyState>,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    // 1. Select provider from failover queue
    let providers = state.provider_router.select_providers("claude").await?;
    
    // 2. Apply circuit breaker
    let allow_result = state.provider_router
        .allow_provider_request(&providers[0].id, "claude")
        .await;
    
    if !allow_result.allowed {
        return Err(ProxyError::CircuitOpen);
    }
    
    // 3. Forward request to upstream
    let response = forwarder::forward(
        request,
        &providers[0],
        &state.config,
    ).await?;
    
    // 4. Record result for circuit breaker
    state.provider_router.record_result(
        &providers[0].id,
        "claude",
        allow_result.used_half_open_permit,
        response.status().is_success(),
        None,
    ).await?;
    
    Ok(response)
}
```

**Characteristics**:
- High-level abstraction (Axum)
- Provider-aware routing
- Circuit breaker integration
- Async/await throughout

---

### Literbike (Tokio + Custom Protocol Detection)

```rust
// src/universal_listener.rs
pub async fn detect_protocol<S>(stream: &mut S) -> io::Result<(Protocol, Vec<u8>)>
where
    S: AsyncRead + Unpin,
{
    let mut buffer = vec![0u8; 1024];
    let n = stream.read(&mut buffer).await?;
    
    if n == 0 {
        return Ok((Protocol::Unknown, vec![]));
    }
    
    buffer.truncate(n);
    
    // SOCKS5 starts with version byte 0x05
    if n >= 2 && buffer[0] == 0x05 {
        debug!("Detected SOCKS5 protocol");
        return Ok((Protocol::Socks5, buffer));
    }
    
    // Check for text-based protocols
    if let Ok(text) = std::str::from_utf8(&buffer[..std::cmp::min(n, 512)]) {
        // HTTP methods
        if text.starts_with("GET ") || text.starts_with("POST ") {
            // WebSocket upgrade detection
            if text_upper.contains("UPGRADE: WEBSOCKET") {
                return Ok((Protocol::WebSocket, buffer));
            }
            return Ok((Protocol::Http, buffer));
        }
        
        // WebRTC STUN detection
        if n >= 20 && buffer[0] == 0x00 && buffer[1] == 0x01 {
            if buffer[4] == 0x21 && buffer[5] == 0x12 {
                return Ok((Protocol::WebRTC, buffer));
            }
        }
    }
    
    Ok((Protocol::Unknown, buffer))
}
```

**Characteristics**:
- Low-level byte inspection
- Protocol-agnostic
- Uses `PrefixedStream` to replay buffered bytes
- POSIX `peek()` for non-destructive detection

---

## Integration Opportunities

### 🔴 HIGH PRIORITY: Borrow from Literbike → CC-Switch

#### 1. Protocol Auto-Detection

**Why**: CC-Switch currently assumes all traffic is HTTP. Adding protocol detection would enable:
- WebSocket support for streaming LLM responses
- SOCKS5 support for client applications
- QUIC support for HTTP/3

**How**:
```rust
// Add to cc-switch/src-tauri/src/proxy/
pub mod protocol_detector {
    use tokio::io::{AsyncRead, AsyncReadExt};
    
    pub enum DetectedProtocol {
        Http,
        Https,
        WebSocket,
        Socks5,
        Quic,
        Unknown,
    }
    
    pub async fn detect<S>(stream: &mut S) -> Result<(DetectedProtocol, Vec<u8>)>
    where
        S: AsyncRead + Unpin,
    {
        // Borrow from literbike/src/universal_listener.rs
    }
}
```

#### 2. QUIC Support

**Why**: QUIC (HTTP/3) provides:
- Lower latency (0-RTT handshakes)
- Better multiplexing (no head-of-line blocking)
- Built-in encryption

**How**:
```rust
// Add QUIC listener alongside TCP
use quinn::{Endpoint, ServerConfig};

pub async fn start_quic_listener(
    addr: SocketAddr,
) -> Result<Endpoint, QuicError> {
    // Borrow from literbike/src/quic/
    // Use quinn crate for QUIC implementation
}
```

#### 3. TLS Fingerprinting

**Why**: Some LLM providers block automated traffic. TLS fingerprinting can:
- Mimic browser TLS handshakes
- Evade automated traffic detection
- Improve compatibility with strict providers

**How**:
```rust
// Add to cc-switch/src-tauri/src/proxy/
pub mod tls_fingerprint {
    // Borrow from literbike/src/tls_fingerprint.rs
    pub fn get_mobile_browser_fingerprint() -> TlsFingerprint {
        // Return Chrome/Safari mobile TLS profile
    }
}
```

---

### 🟡 MEDIUM PRIORITY: Shared Development

#### 1. Unified Protocol Registry

Create a shared crate for protocol detection:

```toml
# Cargo.toml
[workspace]
members = [
    "cc-switch",
    "literbike",
    "crates/protocol-registry",  # NEW shared crate
]
```

```rust
// crates/protocol-registry/src/lib.rs
pub trait ProtocolDetector: Send + Sync {
    fn detect(&self, data: &[u8]) -> ProtocolDetectionResult;
    fn protocol_name(&self) -> &'static str;
}

pub struct HttpDetector;
impl ProtocolDetector for HttpDetector { ... }

pub struct Socks5Detector;
impl ProtocolDetector for Socks5Detector { ... }

pub struct QuicDetector;
impl ProtocolDetector for QuicDetector { ... }
```

#### 2. WebSocket Support for LLM Streaming

**Use Case**: Some LLM providers support WebSocket for streaming:

```rust
// CC-Switch handler with WebSocket support
pub async fn proxy_handler(
    ws: WebSocketUpgrade,
    State(state): State<ProxyState>,
) -> Result<Response, ProxyError> {
    Ok(ws.on_upgrade(|socket| async move {
        // Handle WebSocket connection to LLM provider
        handle_llm_websocket(socket, state).await;
    }))
}
```

---

### 🟢 LOW PRIORITY: Future Enhancements

#### 1. Bonjour/mDNS for Provider Discovery

**Idea**: Auto-discover local LLM providers (Ollama, vLLM, etc.) via mDNS:

```rust
// Discover local LLM servers
let providers = bonjour_discover("_llm-api._tcp").await?;
for provider in providers {
    log::info!("Found LLM provider: {}:{} ", provider.host, provider.port);
}
```

#### 2. UPnP for Public Proxy Exposure

**Idea**: Automatically forward proxy port for remote access:

```rust
// Auto-forward proxy port
if config.expose_publicly {
    upnp_forward_port(15721, 15721)?;
    log::info!("Proxy exposed on public IP:{}", get_public_ip()?);
}
```

---

## QUIC Network Model Locator Proposal

### The Idea

**"Plugging models into a QUIC network to just have locators"**

Instead of HTTP-based provider configuration, use QUIC with:
1. **Model Locators** - QUIC connection IDs that identify models
2. **Provider Discovery** - QUIC-based service discovery
3. **0-RTT Handshakes** - Instant model switching

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│              QUIC Model Locator Network                  │
│                                                          │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────┐ │
│  │   CC-Switch  │────▶│   QUIC       │────▶│  Model   │ │
│  │   Client     │     │   Locator    │     │ Provider │ │
│  │              │◀────│   Network    │◀────│          │ │
│  └──────────────┘     └──────────────┘     └──────────┘ │
│         │                    │                    │      │
│         │ Connection ID:     │                    │      │
│         │ "claude-sonnet"    │                    │      │
│         ▼                    ▼                    ▼      │
└─────────────────────────────────────────────────────────┘
```

### Implementation

#### 1. QUIC Connection ID as Model Locator

```rust
// Model locator format
pub struct ModelLocator {
    pub connection_id: ConnectionId,  // QUIC connection ID
    pub model_id: String,              // "anthropic/claude-sonnet-4"
    pub provider_id: String,           // "openrouter-cc"
    pub capabilities: Vec<Capability>, // [Vision, FunctionCalling]
}

// QUIC connection with model metadata
pub async fn connect_to_model(
    locator: &ModelLocator,
) -> Result<QuicConnection, LocatorError> {
    let mut endpoint = Endpoint::client("[::]:0")?;
    
    // Use connection ID as model identifier
    let mut client_config = ClientConfig::default();
    client_config.transport_config(Arc::new({
        let mut config = TransportConfig::default();
        config.connection_id_locator(locator.connection_id);
        config
    }));
    
    endpoint.set_default_client_config(client_config);
    
    let conn = endpoint
        .connect(locator.address, "llm-model")?
        .await?;
    
    Ok(conn)
}
```

#### 2. Model Locator Registry

```rust
// Centralized model locator registry
pub struct ModelLocatorRegistry {
    pub models: HashMap<String, ModelLocator>,
    pub quic_endpoint: Endpoint,
}

impl ModelLocatorRegistry {
    pub async fn register_model(
        &mut self,
        model_id: String,
        provider: Provider,
    ) -> Result<ModelLocator, RegistryError> {
        // Generate unique connection ID for model
        let connection_id = ConnectionId::random();
        
        let locator = ModelLocator {
            connection_id,
            model_id: model_id.clone(),
            provider_id: provider.id,
            capabilities: provider.capabilities,
        };
        
        self.models.insert(model_id, locator.clone());
        Ok(locator)
    }
    
    pub async fn discover_model(
        &self,
        model_id: &str,
    ) -> Option<&ModelLocator> {
        self.models.get(model_id)
    }
}
```

#### 3. CC-Switch Integration

```rust
// CC-Switch provider router with QUIC locators
pub struct ProviderRouter {
    db: Arc<Database>,
    quic_registry: Arc<ModelLocatorRegistry>,
    circuit_breakers: Arc<RwLock<HashMap<String, Arc<CircuitBreaker>>>>,
}

impl ProviderRouter {
    pub async fn select_providers(
        &self,
        app_type: &str,
        model_id: &str,
    ) -> Result<Vec<Provider>, AppError> {
        // Try QUIC locator first
        if let Some(locator) = self.quic_registry.discover_model(model_id).await {
            // Use QUIC connection for model
            let conn = connect_to_model(locator).await?;
            return Ok(vec![Provider::from_quic_locator(locator, conn)]);
        }
        
        // Fall back to HTTP providers
        self.select_http_providers(app_type).await
    }
}
```

### Benefits

| Benefit | Description |
|---------|-------------|
| **0-RTT Handshakes** | Instant model switching without TLS handshake |
| **Connection Migration** | Switch networks without dropping connection |
| **Multiplexing** | Multiple models over single QUIC connection |
| **Built-in Encryption** | No separate TLS layer needed |
| **Locator-Based** | Models identified by connection ID, not URL |

### Challenges

| Challenge | Mitigation |
|-----------|------------|
| QUIC library maturity | Use `quinn` crate (production-ready) |
| Provider support | Fall back to HTTP for non-QUIC providers |
| Connection ID management | Use CC-Switch database for persistence |
| Firewall traversal | Use UDP hole punching (built into QUIC) |

---

## Implementation Roadmap

### Phase 1: Protocol Detection (2-3 weeks)

**Tasks**:
1. Port `universal_listener.rs` to CC-Switch
2. Add WebSocket support to proxy handler
3. Test with streaming LLM responses

**Deliverable**: CC-Switch can detect and proxy WebSocket connections

---

### Phase 2: QUIC Support (3-4 weeks)

**Tasks**:
1. Add `quinn` dependency to CC-Switch
2. Implement QUIC listener alongside TCP
3. Create `ModelLocator` registry
4. Test 0-RTT handshakes

**Deliverable**: CC-Switch can proxy QUIC connections with model locators

---

### Phase 3: TLS Fingerprinting (1-2 weeks)

**Tasks**:
1. Port `tls_fingerprint.rs` to CC-Switch
2. Add mobile browser profiles
3. Test with strict LLM providers

**Deliverable**: CC-Switch can mimic browser TLS fingerprints

---

### Phase 4: Shared Protocol Registry (Ongoing)

**Tasks**:
1. Create `protocol-registry` crate
2. Move detection logic from both projects
3. Publish to crates.io

**Deliverable**: Shared protocol detection library

---

## Conclusion

**CC-Switch** excels at:
- Provider management
- Failover logic
- Circuit breaker
- UI/UX

**Literbike** excels at:
- Protocol auto-detection
- QUIC support
- TLS fingerprinting
- Low-level network handling

**Integration Strategy**:
1. Borrow protocol detection from Literbike → CC-Switch
2. Add QUIC model locators for instant switching
3. Keep projects separate (no monorepo)
4. Share protocol registry crate (optional)

**Result**: CC-Switch becomes a **multi-protocol AI proxy** with QUIC model locators, while Literbike remains a **general-purpose network toolkit**.
