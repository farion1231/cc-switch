# ModelMux + II-Agent: Exo-Config Rewriter Pattern

## Architecture Overview

**II-Agent remains unmodified** - ModelMux acts as an **external configuration rewriter** (exo-config rewriter).

```
┌─────────────────────────────────────────────────────────┐
│                    II-Agent                              │
│  (Unmodified upstream code)                              │
│                                                          │
│  Config: OPENAI_BASE_URL=http://127.0.0.1:8888/v1       │
│  Model: /anthropic/claude-3-5-sonnet                    │
└────────────────────┬────────────────────────────────────┘
                     │
                     │ OpenAI-compatible HTTP requests
                     ▼
┌─────────────────────────────────────────────────────────┐
│                   ModelMux                               │
│  (Exo-Config Rewriter)                                   │
│                                                          │
│  1. Parse /provider/model from request                  │
│  2. Select best key (ranker + quota)                    │
│  3. Rewrite config (provider URL + API key)             │
│  4. Forward to upstream                                 │
│  5. Stream response back                                │
└─────────────────────────────────────────────────────────┘
```

## What is Exo-Config Rewriter?

**Exo-Config** = **Exo**ternal **Config**uration rewriting

Instead of modifying II-Agent's code to support multiple providers, ModelMux:
1. **Intercepts** OpenAI-compatible requests
2. **Parses** `/provider/model` syntax from model field
3. **Rewrites** configuration (base URL + API key) dynamically
4. **Forwards** to actual provider
5. **Streams** response back unchanged

**II-Agent thinks** it's talking to a single OpenAI-compatible endpoint.
**Reality**: ModelMux is routing to Anthropic, OpenAI, Google, etc. based on model name.

## II-Agent Configuration

### Environment Variables (`.env` or shell)

```bash
# Point II-Agent to ModelMux
export OPENAI_BASE_URL=http://127.0.0.1:8888/v1
export OPENAI_API_KEY=virtual-token  # Not used by ModelMux, but required by II-Agent

# Model syntax: /provider/model-name
export LLM_MODEL=/anthropic/claude-3-5-sonnet
```

### II-Agent Config File (`config.yaml` or similar)

```yaml
llm:
  provider: openai  # II-Agent thinks it's OpenAI
  base_url: http://127.0.0.1:8888/v1
  api_key: virtual-token  # Placeholder
  model: /anthropic/claude-3-5-sonnet  # ModelMux parses this
```

## ModelMux Configuration

### Add Provider Keys

```bash
# Via cc-switch GUI (recommended)
# Open cc-switch → Muxer panel → Add Provider Key
# Provider: anthropic
# API Key: sk-ant-...
# Quota: 100.0 (optional)

# Or via SQLite directly
sqlite3 ~/.cc-switch/cc-switch.db <<EOF
INSERT INTO api_keys (id, provider, key_encrypted, quota_limit, is_active, created_at, updated_at)
VALUES ('key-anthropic-1', 'anthropic', X'sk-ant-...', 100.0, 1, $(date +%s), $(date +%s));
EOF
```

### Start ModelMux

```bash
# TCP mode (compatible with all clients)
modelmux --port 8888 --proto tcp

# QUIC mode (0-RTT, connection migration)
modelmux --port 8888 --proto quic

# Auto mode (QUIC + TCP fallback)
modelmux --port 8888 --proto auto
```

## Example: II-Agent Request Flow

### 1. II-Agent Sends Request

```python
# II-Agent internal code (unmodified)
import openai

client = openai.OpenAI(
    base_url="http://127.0.0.1:8888/v1",
    api_key="virtual-token"
)

response = client.chat.completions.create(
    model="/anthropic/claude-3-5-sonnet",  # ← ModelMux parses this
    messages=[{"role": "user", "content": "Hello!"}]
)
```

### 2. ModelMux Intercepts

```rust
// ModelMux router.rs
async fn handle_chat_completions(...) {
    // Parse /anthropic/claude-3-5-sonnet
    let model_id = ModelId::parse("/anthropic/claude-3-5-sonnet").unwrap();
    // model_id.provider = "anthropic"
    // model_id.model = "claude-3-5-sonnet"
    
    // Get keys for anthropic
    let keys = key_vault.get_keys_for_provider("anthropic");
    
    // Select best key (ranker + quota)
    let selected_key = ranker.select_best_key(&keys);
    
    // Rewrite request for Anthropic API
    let transformed = transform_for_anthropic(&request);
    
    // Forward to Anthropic
    let response = reqwest::post("https://api.anthropic.com/v1/messages")
        .header("Authorization", format!("Bearer {}", selected_key.key))
        .json(&transformed)
        .send()
        .await?;
    
    // Stream back to II-Agent
    Ok(response)
}
```

### 3. II-Agent Receives Response

```python
# II-Agent receives standard OpenAI-compatible response
print(response.choices[0].message.content)
# "Hello! How can I help you today?"
```

## Supported Model Syntax

| Model String | Routes To | API Format |
|--------------|-----------|------------|
| `/anthropic/claude-3-5-sonnet` | Anthropic API | Anthropic Messages |
| `/openai/gpt-4o` | OpenAI API | OpenAI Chat |
| `/google/gemini-2.5-pro` | Google API | Google GenerateContent |
| `/openrouter/anthropic/claude-3-5-sonnet` | OpenRouter | OpenAI-compatible |
| `/local/ollama/phi3` | Local Ollama | OpenAI-compatible |

## Benefits of Exo-Config Pattern

| Benefit | Description |
|---------|-------------|
| **Zero Code Changes** | II-Agent upstream remains unmodified |
| **Single Endpoint** | All providers via `http://127.0.0.1:8888/v1` |
| **Key Security** | II-Agent never sees real provider keys |
| **Automatic Rotation** | ModelMux handles key rotation, quotas |
| **Intelligent Routing** | Ranker selects best key/provider |
| **QUIC Support** | 0-RTT, connection migration (mobile) |
| **Radio-Aware** | LiteBike integration for mobile egress |

## Migration Path

### Current State (Multiple Configs)

```bash
# II-Agent config for Anthropic
export ANTHROPIC_API_KEY=sk-ant-...
export ANTHROPIC_BASE_URL=https://api.anthropic.com

# Switch to OpenAI requires config change
export OPENAI_API_KEY=sk-...
export OPENAI_BASE_URL=https://api.openai.com
```

### With ModelMux (Single Config)

```bash
# Single config for all providers
export OPENAI_BASE_URL=http://127.0.0.1:8888/v1
export OPENAI_API_KEY=virtual-token

# Change provider via model name only
export LLM_MODEL=/anthropic/claude-3-5-sonnet  # Anthropic
export LLM_MODEL=/openai/gpt-4o               # OpenAI
export LLM_MODEL=/google/gemini-2.5-pro       # Google
```

## Testing

### Start ModelMux

```bash
cd /Users/jim/work/cc-switch/modelmux
cargo run -- --port 8888 --proto tcp
```

### Test with curl

```bash
# Test /v1/models
curl http://127.0.0.1:8888/v1/models

# Test /v1/chat/completions
curl http://127.0.0.1:8888/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "/anthropic/claude-3-5-sonnet",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Test health
curl http://127.0.0.1:8888/health
```

### Test with II-Agent

```bash
# Set environment
export OPENAI_BASE_URL=http://127.0.0.1:8888/v1
export LLM_MODEL=/anthropic/claude-3-5-sonnet

# Run II-Agent (unmodified)
cd /Users/jim/work/ii-agent
python -m ii_agent.cli.chat
```

## Troubleshooting

### II-Agent Can't Connect

```bash
# Check ModelMux is running
curl http://127.0.0.1:8888/health

# Check firewall
sudo lsof -i :8888

# Restart ModelMux
modelmux --port 8888 --proto tcp --verbose
```

### Model Not Found

```bash
# Check model syntax (must start with /)
export LLM_MODEL=/anthropic/claude-3-5-sonnet  # ✅ Correct
export LLM_MODEL=anthropic/claude-3-5-sonnet  # ❌ Missing leading /

# Check key exists
sqlite3 ~/.cc-switch/cc-switch.db "SELECT id, provider FROM api_keys WHERE provider='anthropic';"
```

### Quota Exhausted

```bash
# Check quota usage
sqlite3 ~/.cc-switch/cc-switch.db "SELECT id, quota_limit, quota_used FROM api_keys;"

# Reset quota
sqlite3 ~/.cc-switch/cc-switch.db "UPDATE api_keys SET quota_used=0 WHERE id='key-anthropic-1';"

# Add new key
# Via cc-switch GUI or SQLite INSERT
```

## Next Steps

1. **Start ModelMux**: `cargo run -- --port 8888 --proto tcp`
2. **Add Provider Keys**: cc-switch GUI or SQLite
3. **Configure II-Agent**: Set `OPENAI_BASE_URL=http://127.0.0.1:8888/v1`
4. **Test**: Run II-Agent with `/provider/model` syntax
5. **Enjoy**: Zero code changes, automatic key rotation, intelligent routing
