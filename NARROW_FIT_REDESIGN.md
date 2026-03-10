# CC-Switch Narrow-Fit Redesign

## Vision

**Transform CC-Switch from a multi-app configuration manager into a focused, protocol-agnostic LLM proxy.**

### Current State (Wide-Fit)
```
CC-Switch = Provider Manager + Config Sync + MCP + Skills + Prompts + Proxy
            └─────────────────────────────────────────────────────────────┘
                         Too many responsibilities
```

### Target State (Narrow-Fit)
```
CC-Switch = Unified LLM Proxy with /prov/{vendor}/{model} routing
            └────────────────────────────────────────────────────┘
                    Single, focused responsibility
```

---

## Architecture

### Current Architecture (App-Coupled)

```
┌─────────────────────────────────────────────────────────────────┐
│                         CC-Switch                                │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │   Claude     │  │    Codex     │  │    Gemini    │          │
│  │  Providers   │  │   Providers  │  │   Providers  │          │
│  │  (per-app)   │  │  (per-app)   │  │  (per-app)   │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
│         │                │                │                     │
│  ┌──────▼────────────────▼────────────────▼──────┐             │
│  │           Provider Router (per-app)           │             │
│  │  - Claude failover                            │             │
│  │  - Codex failover                             │             │
│  │  - Gemini failover                            │             │
│  └───────────────────────────────────────────────┘             │
│                                                                  │
│  Problem: Keys coupled to apps, duplicated config,              │
│           no unified routing                                    │
└─────────────────────────────────────────────────────────────────┘
```

### Target Architecture (Uncoupled)

```
┌─────────────────────────────────────────────────────────────────┐
│                  CC-Switch Unified LLM Proxy                     │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                   Protocol Layer                          │   │
│  │                                                           │   │
│  │   ┌──────┐  ┌───────────┐  ┌────────┐  ┌──────────┐     │   │
│  │   │ QUIC │  │ WebSocket │  │ HTTP/2 │  │ HTTP/1.1 │     │   │
│  │   └──────┘  └───────────┘  └────────┘  └──────────┘     │   │
│  │                                                           │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                   │
│  ┌───────────────────────────▼───────────────────────────────┐  │
│  │          /prov/{vendor}/{model} Router                     │  │
│  │                                                            │  │
│  │   GET  /prov/anthropic/claude-sonnet-4                    │  │
│  │   POST /prov/openai/gpt-4o                                │  │
│  │   WS   /prov/google/gemini-2.5-pro                        │  │
│  │   QUIC /prov/deepseek/deepseek-chat                       │  │
│  │                                                            │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              │                                   │
│  ┌───────────────────────────▼───────────────────────────────┐  │
│  │                  Provider Key Vault                        │  │
│  │                                                            │  │
│  │   ┌─────────────────────────────────────────────────┐     │  │
│  │   │  Keys uncoupled from:                            │     │  │
│  │   │  - ❌ Apps (no Claude/Codex/Gemini separation)   │     │  │
│  │   │  - ❌ Presets (no configuration profiles)        │     │  │
│  │   │  - ✅ Per-key rate limits                        │     │  │
│  │   │  - ✅ Per-key quotas                             │     │  │
│  │   │  - ✅ Per-key health status                      │     │  │
│  │   │  - ✅ Per-key usage tracking                     │     │  │
│  │   └─────────────────────────────────────────────────┘     │  │
│  │                                                            │  │
│  │   Keys: [sk-ant-1, sk-ant-2, sk-openai-1, sk-google-1]    │  │
│  │          └────────┘  └────────┘  └────────┘               │  │
│  │            Round-robin with health checking                │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              │                                   │
│  ┌───────────────────────────▼───────────────────────────────┐  │
│  │               Upstream Provider APIs                       │  │
│  │                                                            │  │
│  │   ┌──────────┐  ┌────────┐  ┌─────────┐  ┌──────────┐    │  │
│  │   │Anthropic │  │ OpenAI │  │ Google  │  │ Others   │    │  │
│  │   │   API    │  │  API   │  │  API    │  │          │    │  │
│  │   └──────────┘  └────────┘  └─────────┘  └──────────┘    │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Key Design Decisions

### 1. Keys Uncoupled from Apps

**Before**:
```rust
// Current: Keys tied to specific app
struct Provider {
    id: String,           // "claude-provider-1"
    app_type: AppType,    // AppType::Claude
    settings_config: {
        env: {
            ANTHROPIC_AUTH_TOKEN: "sk-ant-..."
        }
    }
}
```

**After**:
```rust
// New: Keys are vendor-specific, app-agnostic
struct ApiKey {
    id: String,           // "key-anthropic-1"
    vendor: Vendor,       // Vendor::Anthropic
    key: String,          // "sk-ant-..."
    rate_limit: Option<u64>,
    quota_remaining: Option<f64>,
    health: KeyHealth,
    usage: UsageStats,
}

enum Vendor {
    Anthropic,
    OpenAI,
    Google,
    DeepSeek,
    // ... etc
}
```

---

### 2. Keys Uncoupled from Presets

**Before**:
```rust
// Current: Presets define entire provider config
struct ProviderPreset {
    id: "anthropic-official",
    name: "Anthropic Official",
    transport: {
        baseUrl: "https://api.anthropic.com",
        formats: ["anthropic"],
    },
    // ... 20+ fields
}
```

**After**:
```rust
// New: Minimal vendor + model routing
struct RouteConfig {
    vendor: Vendor,
    model: String,
    upstream_base_url: String,
    api_format: ApiFormat,
}

// Presets become simple routing rules
const ROUTES: &[RouteConfig] = &[
    RouteConfig {
        vendor: Vendor::Anthropic,
        model: "claude-sonnet-4".into(),
        upstream_base_url: "https://api.anthropic.com".into(),
        api_format: ApiFormat::Anthropic,
    },
    // ...
];
```

---

### 3. /prov/{vendor}/{model} Projection

**URL Structure**:
```
/prov/{vendor}/{model}

Examples:
  /prov/anthropic/claude-sonnet-4-20250514
  /prov/openai/gpt-4o
  /prov/google/gemini-2.5-pro
  /prov/deepseek/deepseek-chat
```

**Protocol Support**:
```
HTTP/1.1:  GET  /prov/anthropic/claude-sonnet-4
HTTP/2:    POST /prov/openai/gpt-4o
WebSocket: WS   /prov/google/gemini-2.5-pro
QUIC:      QUIC /prov/deepseek/deepseek-chat (connection ID = model)
```

**Handler**:
```rust
// Unified handler for all protocols
pub async fn handle_llm_request(
    protocol: Protocol,
    vendor: Vendor,
    model: String,
    request: Request,
) -> Result<Response, ProxyError> {
    // 1. Select healthy API key for vendor (round-robin with health)
    let key = key_vault.select_key(vendor).await?;
    
    // 2. Get upstream config for vendor+model
    let route = routes.get(vendor, &model)?;
    
    // 3. Forward request with protocol-specific handling
    match protocol {
        Protocol::Http1 | Protocol::Http2 => {
            forward_http(request, key, route).await
        }
        Protocol::WebSocket => {
            forward_websocket(request, key, route).await
        }
        Protocol::Quic => {
            forward_quic(request, key, route).await
        }
    }
}
```

---

### 4. Protocol Layer Abstraction

```rust
// src/proxy/protocol/mod.rs
pub enum Protocol {
    Http1,
    Http2,
    WebSocket,
    Quic,
}

pub trait ProtocolHandler: Send + Sync {
    fn protocol(&self) -> Protocol;
    async fn handle(&self, request: Request) -> Result<Response, ProxyError>;
}

// HTTP handler
pub struct HttpHandler {
    client: reqwest::Client,
}

impl ProtocolHandler for HttpHandler {
    fn protocol(&self) -> Protocol {
        Protocol::Http1
    }
    
    async fn handle(&self, request: Request) -> Result<Response, ProxyError> {
        // Standard HTTP proxy logic
    }
}

// WebSocket handler
pub struct WebSocketHandler;

impl ProtocolHandler for WebSocketHandler {
    fn protocol(&self) -> Protocol {
        Protocol::WebSocket
    }
    
    async fn handle(&self, request: Request) -> Result<Response, ProxyError> {
        // WebSocket upgrade + bidirectional streaming
    }
}

// QUIC handler
pub struct QuicHandler {
    endpoint: quinn::Endpoint,
}

impl ProtocolHandler for QuicHandler {
    fn protocol(&self) -> Protocol {
        Protocol::Quic
    }
    
    async fn handle(&self, request: Request) -> Result<Response, ProxyError> {
        // QUIC 0-RTT handshake + stream
    }
}
```

---

## Data Model Changes

### Before (Complex, App-Coupled)

```rust
// src/provider.rs (945 lines)
pub struct Provider {
    pub id: String,
    pub name: String,
    pub settings_config: Value,  // Complex nested JSON
    pub website_url: Option<String>,
    pub category: Option<String>,
    pub created_at: Option<i64>,
    pub sort_index: Option<usize>,
    pub notes: Option<String>,
    pub meta: Option<ProviderMeta>,  // 20+ fields
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    pub in_failover_queue: bool,
}

pub struct ProviderMeta {
    pub custom_endpoints: HashMap<String, CustomEndpoint>,
    pub usage_script: Option<UsageScript>,
    pub endpoint_auto_select: Option<bool>,
    pub is_partner: Option<bool>,
    pub partner_promotion_key: Option<String>,
    pub cost_multiplier: Option<String>,
    pub pricing_model_source: Option<String>,
    pub limit_daily_usd: Option<String>,
    pub limit_monthly_usd: Option<String>,
    pub test_config: Option<ProviderTestConfig>,
    pub proxy_config: Option<ProviderProxyConfig>,
    pub harmony_support: Option<String>,
    pub api_format: Option<String>,
    // ... etc
}
```

### After (Simple, Uncoupled)

```rust
// src/proxy/key_vault.rs (new, ~200 lines)
pub struct ApiKey {
    pub id: String,           // "key-anthropic-1"
    pub vendor: Vendor,       // Vendor::Anthropic
    pub key: String,          // "sk-ant-..." (encrypted)
    pub rate_limit_rpm: Option<u64>,
    pub quota_remaining: Option<f64>,
    pub health: KeyHealth,
    pub usage: UsageStats,
}

pub struct KeyHealth {
    pub is_healthy: bool,
    pub consecutive_failures: u32,
    pub last_success: Option<i64>,
    pub last_failure: Option<i64>,
}

pub struct UsageStats {
    pub requests_today: u64,
    pub tokens_today: u64,
    pub cost_today: f64,
}

// src/proxy/routes.rs (new, ~100 lines)
pub struct Route {
    pub vendor: Vendor,
    pub model: String,
    pub upstream_base_url: String,
    pub api_format: ApiFormat,
}

pub enum Vendor {
    Anthropic,
    OpenAI,
    Google,
    DeepSeek,
    Moonshot,
    MiniMax,
}

pub enum ApiFormat {
    Anthropic,      // POST /v1/messages
    OpenAI,         // POST /v1/chat/completions
    OpenAIResponses,// POST /v1/responses
    Google,         // POST /v1beta/models/{model}:generateContent
}
```

---

## API Changes

### Before (Per-App Endpoints)

```
GET  /api/providers?app=claude
POST /api/providers?app=claude
PUT  /api/providers/:id?app=claude
DELETE /api/providers/:id?app=claude

GET  /api/providers?app=codex
POST /api/providers?app=codex
...

GET  /api/providers?app=gemini
...
```

### After (Unified Endpoints)

```
# Key management (uncoupled from apps)
GET    /api/keys
POST   /api/keys
PUT    /api/keys/:id
DELETE /api/keys/:id

# Route management (vendor+model routing)
GET    /api/routes
POST   /api/routes
PUT    /api/routes/:id
DELETE /api/routes/:id

# Proxy endpoint (all protocols)
GET    /prov/anthropic/claude-sonnet-4
POST   /prov/openai/gpt-4o
WS     /prov/google/gemini-2.5-pro
QUIC   /prov/deepseek/deepseek-chat
```

---

## Implementation Plan

### Phase 1: Core Proxy Refactor (Week 1-2)

**Tasks**:
1. Create `src/proxy/key_vault.rs` (new module)
2. Create `src/proxy/routes.rs` (new module)
3. Create `src/proxy/protocol/` (new module)
4. Deprecate `src/provider.rs` (keep for migration)

**Files to Create**:
```
src-tauri/src/proxy/
├── key_vault.rs      # NEW: API key management
├── routes.rs         # NEW: /prov/{vendor}/{model} routing
├── protocol/
│   ├── mod.rs        # NEW: Protocol trait
│   ├── http.rs       # NEW: HTTP/1.1 + HTTP/2
│   ├── websocket.rs  # NEW: WebSocket
│   └── quic.rs       # NEW: QUIC
└── handler.rs        # NEW: Unified request handler
```

**Files to Deprecate** (but keep for migration):
```
src-tauri/src/provider.rs           # → Migrate to key_vault.rs
src-tauri/src/proxy/provider_router.rs  # → Simplify to routes.rs
```

---

### Phase 2: Protocol Support (Week 2-3)

**Tasks**:
1. Implement HTTP handler (migrate from existing)
2. Implement WebSocket handler (borrow from literbike)
3. Implement QUIC handler (borrow from literbike)
4. Add protocol auto-detection (borrow from literbike)

**Code to Port from Literbike**:
```rust
// From literbike/src/universal_listener.rs
pub async fn detect_protocol<S>(stream: &mut S) -> Result<(Protocol, Vec<u8>)>;

// From literbike/src/adapters/quic.rs
pub fn quic_adapter_name() -> &'static str;

// From literbike/src/tls_fingerprint.rs
pub fn get_mobile_browser_fingerprint() -> TlsFingerprint;
```

---

### Phase 3: Frontend Simplification (Week 3-4)

**Tasks**:
1. Remove app-specific provider forms
2. Create unified key management UI
3. Create route configuration UI
4. Remove MCP/Skills/Prompts tabs (or move to separate project)

**Files to Remove/Simplify**:
```
src/components/providers/
├── ClaudeForm.tsx       # → Remove (use unified form)
├── CodexForm.tsx        # → Remove (use unified form)
├── GeminiForm.tsx       # → Remove (use unified form)
└── ProviderForm.tsx     # → Simplify (unified key form)

src/components/
├── McpPanel.tsx         # → Remove (separate project)
├── SkillsPanel.tsx      # → Remove (separate project)
└── PromptsPanel.tsx     # → Remove (separate project)
```

---

### Phase 4: Migration Path (Week 4-5)

**Tasks**:
1. Create migration script (old config → new config)
2. Test with existing users
3. Deprecation warnings in UI
4. Final cutover

**Migration Script**:
```rust
// Migrate old provider config to new key vault
pub fn migrate_providers_to_keys(
    old_providers: IndexMap<String, Provider>,
) -> Result<Vec<ApiKey>, MigrationError> {
    let mut keys = Vec::new();
    
    for (id, provider) in old_providers {
        // Extract API key from settings_config
        if let Some(key) = extract_api_key(&provider.settings_config) {
            keys.push(ApiKey {
                id: format!("key-{}", id),
                vendor: detect_vendor(&provider),
                key,
                rate_limit_rpm: None,
                quota_remaining: None,
                health: KeyHealth::default(),
                usage: UsageStats::default(),
            });
        }
    }
    
    Ok(keys)
}
```

---

## Benefits

### Technical Benefits

| Benefit | Description |
|---------|-------------|
| **Simpler Code** | ~50% reduction in lines of code |
| **Protocol Agnostic** | Support QUIC, WebSocket, HTTP/2, HTTP/1.1 |
| **Easier Testing** | Single proxy endpoint to test |
| **Better Performance** | No per-app routing overhead |
| **Cleaner Architecture** | Separation of concerns (keys vs routes vs protocols) |

### User Benefits

| Benefit | Description |
|---------|-------------|
| **Simpler Config** | Just add keys, routes auto-detected |
| **Unified View** | All providers in one place |
| **Protocol Choice** | Use QUIC for low latency, WebSocket for streaming |
| **Better Key Rotation** | Round-robin across all keys for a vendor |
| **Clearer Pricing** | Per-key usage tracking |

---

## Migration Guide for Users

### Before (Current)

```json
// ~/.cc-switch/config.json
{
  "providers": {
    "claude-provider-1": {
      "id": "claude-provider-1",
      "name": "My Anthropic Key",
      "settings_config": {
        "env": {
          "ANTHROPIC_AUTH_TOKEN": "sk-ant-..."
        }
      },
      "category": "official",
      "meta": { ... 20 fields ... }
    },
    "codex-provider-1": {
      "id": "codex-provider-1",
      "name": "My OpenAI Key",
      "settings_config": {
        "env": {
          "OPENAI_API_KEY": "sk-..."
        }
      },
      ...
    }
  }
}
```

### After (New)

```json
// ~/.cc-switch/config.json
{
  "keys": [
    {
      "id": "key-anthropic-1",
      "vendor": "anthropic",
      "key": "sk-ant-...",
      "rate_limit_rpm": 1000
    },
    {
      "id": "key-openai-1",
      "vendor": "openai",
      "key": "sk-...",
      "rate_limit_rpm": 500
    }
  ],
  "routes": [
    {
      "vendor": "anthropic",
      "model": "claude-sonnet-4",
      "upstream_base_url": "https://api.anthropic.com",
      "api_format": "anthropic"
    },
    {
      "vendor": "openai",
      "model": "gpt-4o",
      "upstream_base_url": "https://api.openai.com/v1",
      "api_format": "openai"
    }
  ]
}
```

---

## Conclusion

**Narrow-Fit CC-Switch** = Single, focused LLM proxy with:
- ✅ Keys uncoupled from apps and presets
- ✅ `/prov/{vendor}/{model}` routing
- ✅ Protocol support (QUIC, WebSocket, HTTP/2, HTTP/1.1)
- ✅ ~50% code reduction
- ✅ Easier to maintain and extend

**What We Remove**:
- ❌ App-specific provider management
- ❌ MCP/Skills/Prompts (move to separate project)
- ❌ Complex ProviderMeta with 20+ fields
- ❌ Per-app failover logic

**What We Keep**:
- ✅ Provider failover (now per-vendor, not per-app)
- ✅ Circuit breaker (now per-key, not per-provider)
- ✅ Health checking (now per-key, not per-provider)
- ✅ Tauri integration (system tray, notifications)

**What We Add**:
- ✅ Protocol auto-detection (from literbike)
- ✅ QUIC support (from literbike)
- ✅ WebSocket support (from literbike)
- ✅ TLS fingerprinting (from literbike)
