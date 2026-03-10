# ModelMux - OpenAI-Compatible LLM Proxy

**Single drop-in endpoint exposing `/v1/chat/completions`, `/v1/models`, `/v1/embeddings` with intelligent routing.**

## Features

- ✅ **`/provider/model` routing** - e.g., `anthropic/claude-3-5-sonnet`, `openai/gpt-4o-mini`
- ✅ **OpenAI-compatible API** - Works with Claw, Cursor, Continue, Aider, LangGraph, MCP servers
- ✅ **Automatic key rotation** - Per-key quotas, rate limits, intelligent ranker
- ✅ **QUIC/h3 support** - Single UDP port, 0-RTT resumption, connection migration
- ✅ **Radio-aware egress** - LiteBike integration for mobile last-millimeter optimization
- ✅ **POSIX ACL key vault** - Filesystem-based (`~/.cc-switch/acl/`) + env var opt-in
- ✅ **Agents never see real keys** - Virtual tokens, encrypted storage

## Quick Start

```bash
# Build
cd modelmux
cargo build --release

# Run (TCP mode)
./target/release/modelmux --port 8888 --proto tcp

# Run (QUIC mode)
./target/release/modelmux --port 8888 --proto quic

# Run (Auto mode - QUIC + TCP fallback)
./target/release/modelmux --port 8888 --proto auto
```

## Configuration

### Method 1: Filesystem ACL (Recommended for Production)

```bash
# Create provider directory
mkdir -p ~/.cc-switch/acl/anthropic

# Add key
echo "sk-ant-0123456789abcdef" > ~/.cc-switch/acl/anthropic/key-1.key

# Set secure permissions
chmod 600 ~/.cc-switch/acl/anthropic/key-1.key

# Add metadata (optional)
cat > ~/.cc-switch/acl/anthropic/key-1.meta <<EOF
quota_limit=100.0
quota_used=0.0
permissions=600
EOF
```

### Method 2: Environment Variables (Opt-In, Development)

```bash
# In .bashrc, .zshrc, or shell
export ANTHROPIC_API_KEY=sk-ant-...
export OPENAI_API_KEY=sk-...
export GOOGLE_API_KEY=...

# ModelMux auto-loads these on startup
```

### Configure Agent

```bash
# Any OpenAI-compatible client
export OPENAI_BASE_URL=http://127.0.0.1:8888/v1
export OPENAI_MODEL=anthropic/claude-3-5-sonnet

# Example: Cursor settings
# Settings → AI → Base URL: http://127.0.0.1:8888/v1
# Settings → AI → Model: anthropic/claude-3-5-sonnet
```

## API Reference

### POST /v1/chat/completions

```bash
curl http://127.0.0.1:8888/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "anthropic/claude-3-5-sonnet",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### GET /v1/models

```bash
curl http://127.0.0.1:8888/v1/models
```

### GET /health

```bash
curl http://127.0.0.1:8888/health
```

## Integration with LiteBike

ModelMux is designed to be integrated into LiteBike as a binary mode:

```bash
# After integration
litebike mux --port 8888 --proto quic
```

### LiteBike Cargo.toml additions

```toml
[dependencies]
# Add to existing LiteBike Cargo.toml
axum = "0.7"
h3 = "0.0.6"
h3-quinn = "0.0.7"
quinn = "0.11"
```

### LiteBike src/bin/litebike.rs additions

```rust
// Add mux subcommand
match args.command {
    Command::Mux { port, proto } => {
        modelmux::main(port, proto).await?;
    }
    // ... existing commands
}
```

## Integration with CC-Switch

ModelMux uses the same SQLite database as cc-switch for key storage.

### CC-Switch Tauri Commands

```rust
// src-tauri/src/commands/muxer.rs

#[tauri::command]
pub async fn muxer_add_key(
    state: State<'_, AppState>,
    provider: String,
    key: String,
) -> Result<String, String> {
    // Add key to shared SQLite
    // Return virtual token for agent config
}

#[tauri::command]
pub async fn muxer_start(
    state: State<'_, AppState>,
    port: u16,
    proto: String,
) -> Result<(), String> {
    // Spawn modelmux daemon
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    ModelMux                              │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  TCP :8888   │  │ QUIC :8888   │  │   WS :8889   │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │
│         └─────────────────┴─────────────────┘           │
│                           │                              │
│         ┌─────────────────▼─────────────────┐           │
│         │    /provider/model Router          │           │
│         └─────────────────┬─────────────────┘           │
│                           │                              │
│         ┌─────────────────▼─────────────────┐           │
│         │    Intelligent Ranker              │           │
│         │  score = f(latency, cost, quota)   │           │
│         └─────────────────┬─────────────────┘           │
│                           │                              │
│         ┌─────────────────▼─────────────────┐           │
│         │    Encrypted Key Vault             │           │
│         │  (shared with cc-switch)           │           │
│         └─────────────────┬─────────────────┘           │
│                           │                              │
│         ┌─────────────────▼─────────────────┐           │
│         │    Upstream LLM APIs               │           │
│         │  Anthropic, OpenAI, Google, ...    │           │
│         └───────────────────────────────────┘           │
└─────────────────────────────────────────────────────────┘
```

## Ranker

Default ranker uses weighted sum:

```
score = 0.30 * latency_score
      + 0.20 * cost_score
      + 0.15 * capability_score
      + 0.20 * quota_score
      + 0.15 * carrier_score
```

### Custom Ranker

Implement the `Ranker` trait:

```rust
use modelmux::ranker::{Ranker, RankContext};

struct MyCustomRanker;

impl Ranker for MyCustomRanker {
    fn score(&self, ctx: &RankContext) -> f64 {
        // Custom scoring logic
    }
}
```

## Security

- **Encrypted key vault** - API keys encrypted at rest (TODO: implement proper encryption)
- **Virtual tokens** - Agents use virtual tokens, never see real provider keys
- **Per-key quotas** - Prevent runaway spending
- **Rate limiting** - Per-key RPM limits

## Performance

| Metric | Target |
|--------|--------|
| Tool-call roundtrip | <200ms (QUIC 0-RTT) |
| Session drop rate | ~0% (QUIC migration) |
| Multi-model throughput | ~250 req/min |
| Port footprint | Single UDP 8888 |

## License

AGPL-3.0 (same as LiteBike)

## Contributing

1. Fork LiteBike
2. Add modelmux crate
3. Integrate with cc-switch for GUI
4. Test with your agentic tools

## Related Projects

- **LiteBike** - Mobile Rust proxy with radio awareness
- **CC-Switch** - Provider config management with Tauri GUI
- **LiteLLM** - Python LLM proxy (inspiration)
- **quinn** - Rust QUIC implementation
