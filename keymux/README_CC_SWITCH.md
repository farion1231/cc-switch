# ModelMux for CC-Switch

## Overview

**ModelMux** is an OpenAI-compatible LLM proxy that provides:

- `/provider/model` routing (e.g., `anthropic/claude-3-5-sonnet`)
- Automatic key rotation with per-key quotas
- QUIC/h3 support (0-RTT, connection migration)
- LiteBike integration for network-aware routing
- POSIX ACL key vault (filesystem + env var opt-in)

## Quick Start

### 1. Add API Keys via CC-Switch UI

1. Open CC-Switch
2. Navigate to **Muxer** tab
3. Select provider (Anthropic, OpenAI, Google, etc.)
4. Paste API key
5. Set optional quota limit
6. Click **Add Key**

### 2. Start ModelMux

1. In Muxer tab, click **Start (Port 8888)**
2. Status changes to "Running on port 8888"

### 3. Configure Your Tools

Set your AI tools to use ModelMux as the OpenAI endpoint:

```bash
export OPENAI_BASE_URL=http://127.0.0.1:8888/v1
export OPENAI_MODEL=anthropic/claude-3-5-sonnet
```

**Supported Tools**:
- II-Agent
- Claw / OpenClaw
- Cursor
- Continue
- Aider
- LangGraph agents
- Any OpenAI-compatible client

## Model Syntax

Use `/provider/model` format:

| Model String | Routes To |
|--------------|-----------|
| `/anthropic/claude-3-5-sonnet` | Anthropic API |
| `/openai/gpt-4o` | OpenAI API |
| `/google/gemini-2.5-pro` | Google API |
| `/deepseek/deepseek-chat` | DeepSeek API |
| `/openrouter/anthropic/claude-3-5-sonnet` | OpenRouter |

## Key Features

### Automatic Key Rotation

ModelMux automatically rotates between keys based on:
- **Quota remaining** - Avoids exhausted keys
- **Rate limits** - Respects per-key RPM limits
- **Latency** - Prefers faster keys (LiteBike integration)

### Quota Management

Set per-key quota limits to control spending:

```
Key: key-anthropic-abc123
Quota: 50.0 / 100.0 tokens (50%)
```

When quota is exhausted, ModelMux automatically switches to another key.

### QUIC/h3 Support

For supported clients, ModelMux offers:
- **0-RTT handshakes** - Faster reconnections
- **Connection migration** - Survive WiFi → Cellular handoff
- **Single UDP port** - Port 8888 (UDP)

### LiteBike Integration

ModelMux integrates with LiteBike for:
- Network interface detection
- Signal strength monitoring
- Latency probing
- Automatic best interface selection

## Configuration

### Filesystem Keys (Recommended)

Keys are stored in `~/.cc-switch/acl/`:

```
~/.cc-switch/acl/
├── anthropic/
│   ├── key-1.key       # API key
│   └── key-1.meta      # Quota, permissions
├── openai/
│   └── key-1.key
└── google/
    └── key-1.key
```

**Permissions**: 0600 (owner read/write only)

### Environment Variables (Opt-In)

ModelMux also loads keys from environment:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
export OPENAI_API_KEY=sk-...
export GOOGLE_API_KEY=...
```

Only loaded if explicitly set.

## CC-Switch Integration

### Muxer Panel

The CC-Switch Muxer panel provides:
- **Key Management** - Add/remove/list API keys
- **Quota Monitoring** - Track usage per key
- **Muxer Control** - Start/stop proxy server
- **LiteBike Metrics** - Network quality display

### Shared Configuration

ModelMux uses the same SQLite database as CC-Switch:
- `~/.cc-switch/cc-switch.db`
- Provider configs shared between both
- No duplicate configuration needed

## API Reference

### POST /v1/chat/completions

Standard OpenAI chat completions endpoint:

```bash
curl http://127.0.0.1:8888/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "/anthropic/claude-3-5-sonnet",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### GET /v1/models

List available models:

```bash
curl http://127.0.0.1:8888/v1/models
```

### GET /health

Health check:

```bash
curl http://127.0.0.1:8888/health
```

## Troubleshooting

### Muxer Won't Start

```bash
# Check if port 8888 is in use
lsof -i :8888

# Kill existing process
kill <pid>

# Restart from CC-Switch UI
```

### Keys Not Loading

```bash
# Check ACL directory
ls -la ~/.cc-switch/acl/

# Check permissions
chmod 700 ~/.cc-switch/acl
chmod 600 ~/.cc-switch/acl/*/*.key
```

### Quota Not Tracking

```bash
# Check key metadata
cat ~/.cc-switch/acl/anthropic/key-1.meta

# Should show:
# quota_limit=100.0
# quota_used=50.0
```

## Security

- **Encrypted storage** - Keys stored with 0600 permissions
- **Virtual tokens** - Agents never see real provider keys
- **Quota enforcement** - Prevent runaway spending
- **Per-key isolation** - Compromise of one key doesn't affect others

## Performance

| Metric | Value |
|--------|-------|
| **Latency overhead** | <5ms |
| **Throughput** | ~250 req/min |
| **QUIC 0-RTT** | ~0ms handshake |
| **Connection migration** | <100ms handoff |

## Related Documentation

- `ACL_VAULT.md` - POSIX ACL key vault details
- `QUIC_SERVER.md` - QUIC/h3 server implementation
- `LITEBIKE_RADIOS.md` - LiteBike integration
- `IIAGENT_INTEGRATION.md` - II-Agent exo-config rewriter pattern

## License

AGPL-3.0 (same as LiteBike and CC-Switch)
