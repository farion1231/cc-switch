# Quickstart: Codex Anthropic API Format Transform

## How to use

### 1. Configure a Codex provider with Anthropic backend

In cc-switch, create a new Codex provider with:

- **Base URL**: `http://aigw.fx.ctripcorp.com/llm/100000667`
- **API Key**: `sk-gMyHk4Jbddic4HT2zaVbwQ`
- **API Format**: `anthropic` (in provider meta settings)
- **Upstream Model**: `claude-opus-4-6-v1` (optional: remaps model name)

### 2. Start the proxy

Enable the local reverse proxy in cc-switch settings. The proxy will listen on the configured port (default: varies by setup).

### 3. Configure Codex CLI

Point Codex CLI to the local proxy:

```bash
export OPENAI_BASE_URL=http://localhost:<proxy_port>/v1
export OPENAI_API_KEY=<any-value>  # proxy handles auth
```

### 4. Use Codex CLI normally

Codex CLI sends Responses API requests → cc-switch proxy converts to Anthropic Messages API → forwards to the gateway → converts response back → returns to Codex CLI.

## How it works

```
Codex CLI                cc-switch proxy              Anthropic Gateway
   |                          |                             |
   |-- POST /v1/responses --> |                             |
   |   (Responses API)       |-- POST /v1/messages ------> |
   |                          |   (Anthropic Messages API) |
   |                          |                             |
   |                          | <-- Anthropic SSE events -- |
   | <-- Responses SSE ------+                             |
   |   events (converted)    |                             |
```

## Development

### Run tests

```bash
cd src-tauri
cargo test transform_codex -- --nocapture
cargo test streaming_codex_anthropic -- --nocapture
```

### Key files

| File | Purpose |
|------|---------|
| `src-tauri/src/proxy/providers/codex.rs` | `get_codex_api_format()` — detects "anthropic" mode |
| `src-tauri/src/proxy/providers/transform_codex.rs` | Request/response conversion functions |
| `src-tauri/src/proxy/providers/streaming_codex_anthropic.rs` | Anthropic SSE → Responses SSE streaming |
| `src-tauri/src/proxy/handlers.rs` | Handler routing for api_format branches |
| `src-tauri/src/proxy/forwarder.rs` | Endpoint routing to /v1/messages |
