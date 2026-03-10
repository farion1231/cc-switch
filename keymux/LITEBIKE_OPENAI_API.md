# LiteBike OpenAI-Compatible API

## Overview

LiteBike exposes an **OpenAI-compatible OpenAPI 3.0 interface** for LLM routing. ModelMux integrates with this API to provide:

- `/v1/chat/completions` - Chat completions
- `/v1/models` - Model listing
- `/v1/embeddings` - Embeddings (TODO)

## OpenAPI 3.0 Specification

```yaml
openapi: 3.0.0
info:
  title: LiteBike LLM API
  version: 1.0.0
  description: OpenAI-compatible LLM proxy with intelligent routing

servers:
  - url: http://localhost:8888/v1
    description: Local LiteBike instance

paths:
  /chat/completions:
    post:
      summary: Create chat completion
      operationId: createChatCompletion
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/ChatCompletionRequest'
      responses:
        '200':
          description: Successful response
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ChatCompletionResponse'
        '401':
          description: Authentication error
        '429':
          description: Rate limit exceeded
        '500':
          description: Server error

  /models:
    get:
      summary: List available models
      operationId: listModels
      responses:
        '200':
          description: Successful response
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ModelList'

components:
  schemas:
    ChatCompletionRequest:
      type: object
      required:
        - model
        - messages
      properties:
        model:
          type: string
          description: Model identifier (e.g., /anthropic/claude-3-5-sonnet)
          example: /anthropic/claude-3-5-sonnet
        messages:
          type: array
          items:
            $ref: '#/components/schemas/Message'
        temperature:
          type: number
          format: float
          minimum: 0
          maximum: 2
          default: 1.0
        max_tokens:
          type: integer
          minimum: 1
        stream:
          type: boolean
          default: false
        tools:
          type: array
          items:
            $ref: '#/components/schemas/Tool'

    Message:
      type: object
      required:
        - role
        - content
      properties:
        role:
          type: string
          enum: [system, user, assistant, tool]
        content:
          type: string
        tool_calls:
          type: array
          items:
            $ref: '#/components/schemas/ToolCall'

    ChatCompletionResponse:
      type: object
      properties:
        id:
          type: string
        object:
          type: string
          enum: [chat.completion]
        created:
          type: integer
        model:
          type: string
        choices:
          type: array
          items:
            $ref: '#/components/schemas/Choice'
        usage:
          $ref: '#/components/schemas/Usage'

    Choice:
      type: object
      properties:
        index:
          type: integer
        message:
          $ref: '#/components/schemas/Message'
        finish_reason:
          type: string
          enum: [stop, length, tool_calls, content_filter]

    Usage:
      type: object
      properties:
        prompt_tokens:
          type: integer
        completion_tokens:
          type: integer
        total_tokens:
          type: integer

    ModelList:
      type: object
      properties:
        object:
          type: string
          enum: [list]
        data:
          type: array
          items:
            $ref: '#/components/schemas/Model'

    Model:
      type: object
      properties:
        id:
          type: string
        object:
          type: string
          enum: [model]
        created:
          type: integer
        owned_by:
          type: string
```

## Model Syntax

LiteBike uses `/provider/model` syntax:

```
/anthropic/claude-3-5-sonnet
/openai/gpt-4o
/google/gemini-2.5-pro
/deepseek/deepseek-chat
/openrouter/anthropic/claude-3-5-sonnet
```

## Integration with ModelMux

ModelMux acts as a **client** to LiteBike's OpenAI API:

```
┌──────────────┐     OpenAI API      ┌──────────────┐
│  ModelMux    │ ──────────────────▶ │   LiteBike   │
│              │  POST /v1/chat      │              │
│              │  /completions       │              │
│              │ ◀────────────────── │              │
│              │  200 OK (stream)    │              │
└──────────────┘                     └──────────────┘
```

### ModelMux → LiteBike Flow

1. **ModelMux receives request** from II-Agent/Claw/Cursor
2. **Parses `/provider/model`** from model field
3. **Selects best key** (ranker + quota + LiteBike metrics)
4. **Forwards to LiteBike** with transformed request
5. **Streams response** back to client

## Request Transformation

### OpenAI → Anthropic

```json
// Input (OpenAI format)
{
  "model": "/anthropic/claude-3-5-sonnet",
  "messages": [
    {"role": "system", "content": "You are helpful"},
    {"role": "user", "content": "Hello"}
  ]
}

// Output (Anthropic format)
{
  "model": "claude-3-5-sonnet",
  "system": "You are helpful",
  "messages": [
    {"role": "user", "content": "Hello"}
  ],
  "max_tokens": 1024
}
```

### OpenAI → Google

```json
// Input (OpenAI format)
{
  "model": "/google/gemini-2.5-pro",
  "messages": [
    {"role": "user", "content": "Hello"}
  ]
}

// Output (Google format)
{
  "contents": [
    {
      "role": "user",
      "parts": [{"text": "Hello"}]
    }
  ]
}
```

## LiteBike Integration Points

### Phase 1: HTTP Client (✅ Current)

ModelMux uses `reqwest` to call LiteBike:

```rust
// modelmux/src/router.rs
async fn forward_to_upstream(...) {
    let client = reqwest::Client::new();
    
    let response = client
        .post("http://localhost:8889/v1/chat/completions") // LiteBike port
        .header("Authorization", format!("Bearer {}", key.key))
        .json(&transformed_request)
        .send()
        .await?;
    
    Ok(response.json().await?)
}
```

### Phase 2: LiteBike Module Integration (⏳ TODO)

Direct integration with LiteBike crate:

```rust
// TODO: Use LiteBike crate directly
use litebike::client::LiteBikeClient;

async fn forward_to_upstream(...) {
    let client = LiteBikeClient::new();
    
    let response = client
        .chat_completions(&transformed_request)
        .await?;
    
    Ok(response)
}
```

### Phase 3: Shared Runtime (⏳ TODO)

Run ModelMux and LiteBike in same process:

```rust
// TODO: Single binary with both
#[tokio::main]
async fn main() {
    // Start LiteBike server
    tokio::spawn(litebike::serve());
    
    // Start ModelMux server
    modelmux::serve().await;
}
```

## Configuration

### LiteBike Config (`~/.litebike/config.json`)

```json
{
  "port": 8889,
  "providers": {
    "anthropic": {
      "base_url": "https://api.anthropic.com",
      "api_key_env": "ANTHROPIC_API_KEY"
    },
    "openai": {
      "base_url": "https://api.openai.com/v1",
      "api_key_env": "OPENAI_API_KEY"
    }
  }
}
```

### ModelMux Config (`~/.cc-switch/muxer.json`)

```json
{
  "litebike_url": "http://localhost:8889/v1",
  "quota_enabled": true,
  "ranker": {
    "latency_weight": 0.3,
    "cost_weight": 0.2,
    "quota_weight": 0.2,
    "litebike_weight": 0.3
  }
}
```

## Error Handling

### LiteBike Error Response

```json
{
  "error": {
    "message": "Rate limit exceeded",
    "type": "rate_limit_error",
    "code": 429
  }
}
```

### ModelMux Error Mapping

| LiteBike Error | ModelMux Response |
|----------------|-------------------|
| 401 Unauthorized | 401 (key invalid) |
| 429 Rate Limit | 429 (try next key) |
| 500 Server Error | 502 (upstream error) |
| 503 Unavailable | 503 (failover) |

## Testing

### Test LiteBike Directly

```bash
# Start LiteBike
litebike serve --port 8889

# Test OpenAI endpoint
curl http://localhost:8889/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "/anthropic/claude-3-5-sonnet",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### Test ModelMux → LiteBike

```bash
# Start LiteBike
litebike serve --port 8889

# Start ModelMux
modelmux --port 8888 --proto tcp

# Test through ModelMux
curl http://localhost:8888/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "/anthropic/claude-3-5-sonnet",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## Performance

| Metric | Value |
|--------|-------|
| **ModelMux overhead** | <5ms |
| **LiteBike routing** | <10ms |
| **Total overhead** | <15ms |
| **Streaming latency** | ~50ms first token |

## References

- **OpenAI API**: https://platform.openai.com/docs/api-reference
- **LiteBike**: `/Users/jim/work/literbike/`
- **ModelMux**: `/Users/jim/work/cc-switch/modelmux/`
