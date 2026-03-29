# Contract: Responses API ↔ Anthropic Messages API Mapping

**Date**: 2026-03-27

## Provider Configuration Contract

A Codex provider configured for Anthropic backend must have:

```json
{
  "meta": {
    "api_format": "anthropic"
  },
  "settings_config": {
    "base_url": "http://aigw.fx.ctripcorp.com/llm/100000667",
    "env": {
      "OPENAI_API_KEY": "sk-gMyHk4Jbddic4HT2zaVbwQ"
    },
    "upstream_model": "claude-opus-4-6-v1"
  }
}
```

- `meta.api_format = "anthropic"` — triggers the transform path
- `base_url` — the Anthropic-compatible API endpoint (proxy appends `/v1/messages`)
- `OPENAI_API_KEY` — used as Bearer token for auth
- `upstream_model` (optional) — remaps model name in the forwarded request

## Request Transform Contract

### Input: Responses API Request

```json
{
  "model": "o3-mini",
  "instructions": "You are a helpful assistant.",
  "input": [
    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello"}]}
  ],
  "tools": [
    {"type": "function", "name": "get_weather", "description": "Get weather", "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}}
  ],
  "max_output_tokens": 4096,
  "stream": true
}
```

### Output: Anthropic Messages API Request

```json
{
  "model": "claude-opus-4-6-v1",
  "system": "You are a helpful assistant.",
  "messages": [
    {"role": "user", "content": [{"type": "text", "text": "Hello"}]}
  ],
  "tools": [
    {"name": "get_weather", "description": "Get weather", "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}}
  ],
  "max_tokens": 4096,
  "stream": true
}
```

## Response Transform Contract

### Input: Anthropic Messages API Response

```json
{
  "id": "msg_abc123",
  "type": "message",
  "role": "assistant",
  "content": [
    {"type": "text", "text": "Hello! How can I help?"}
  ],
  "model": "claude-opus-4-6-v1",
  "stop_reason": "end_turn",
  "usage": {"input_tokens": 25, "output_tokens": 10}
}
```

### Output: Responses API Response

```json
{
  "id": "resp_abc123",
  "object": "response",
  "model": "claude-opus-4-6-v1",
  "status": "completed",
  "output": [
    {
      "type": "message",
      "id": "msg_...",
      "role": "assistant",
      "status": "completed",
      "content": [{"type": "output_text", "text": "Hello! How can I help?", "annotations": []}]
    }
  ],
  "usage": {"input_tokens": 25, "output_tokens": 10, "total_tokens": 35}
}
```

## HTTP Contract

### Forwarded Request

```
POST {base_url}/v1/messages
Authorization: Bearer {api_key}
Content-Type: application/json
anthropic-version: 2023-06-01
```

### Response to Codex CLI

Non-streaming:
```
HTTP/1.1 200 OK
Content-Type: application/json
```

Streaming:
```
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
```
