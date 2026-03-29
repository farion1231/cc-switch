# Data Model: Codex Anthropic API Format Transform

**Date**: 2026-03-27

## Entities

This feature operates at the proxy/transform layer. No database entities or persistent storage changes are needed. The key data structures are request/response JSON payloads that flow through the transform functions.

### OpenAI Responses API Request (Input from Codex CLI)

```
ResponsesRequest:
  model: string                    # e.g., "o3-mini", "codex-mini"
  input: string | InputItem[]      # conversation input
  instructions?: string            # system prompt
  tools?: Tool[]                   # function definitions (flat format)
  tool_choice?: string | object    # "auto", "required", "none", or function selector
  max_output_tokens?: number       # max tokens for output
  temperature?: number
  top_p?: number
  stream?: boolean

InputItem:
  type: "message" | "function_call" | "function_call_output"
  # message fields:
  role?: "user" | "assistant"
  content?: string | ContentPart[]
  # function_call fields:
  call_id?: string
  name?: string
  arguments?: string (JSON)
  # function_call_output fields:
  output?: string

ContentPart:
  type: "input_text" | "output_text" | "input_image"
  text?: string
  image_url?: string

Tool (Responses format):
  type: "function"
  name: string
  description: string
  parameters: object (JSON Schema)
```

### Anthropic Messages API Request (Output to upstream)

```
AnthropicRequest:
  model: string                    # e.g., "claude-opus-4-6-v1"
  messages: Message[]              # conversation messages
  system?: string | SystemBlock[]  # system prompt
  tools?: AnthropicTool[]          # function definitions (nested format)
  tool_choice?: object             # {"type": "auto"/"any"/"tool", "name"?: string}
  max_tokens: number               # required in Anthropic API
  temperature?: number
  top_p?: number
  stream?: boolean

Message:
  role: "user" | "assistant"
  content: string | ContentBlock[]

ContentBlock:
  type: "text" | "tool_use" | "tool_result" | "image"
  # text fields:
  text?: string
  # tool_use fields:
  id?: string
  name?: string
  input?: object
  # tool_result fields:
  tool_use_id?: string
  content?: string

AnthropicTool:
  name: string
  description: string
  input_schema: object (JSON Schema)
```

### Anthropic Messages API Response (Input from upstream)

```
AnthropicResponse:
  id: string                       # e.g., "msg_..."
  type: "message"
  role: "assistant"
  content: ContentBlock[]          # text, tool_use, thinking blocks
  model: string
  stop_reason: "end_turn" | "tool_use" | "max_tokens" | null
  stop_sequence: string | null
  usage:
    input_tokens: number
    output_tokens: number
    cache_read_input_tokens?: number
    cache_creation_input_tokens?: number
```

### OpenAI Responses API Response (Output to Codex CLI)

```
ResponsesResponse:
  id: string                       # "resp_..."
  object: "response"
  model: string
  status: "completed" | "incomplete" | "in_progress"
  output: OutputItem[]
  usage:
    input_tokens: number
    output_tokens: number
    total_tokens: number
  incomplete_details?: { reason: string }

OutputItem:
  type: "message" | "function_call" | "reasoning"
  # message fields:
  id?: string
  role?: "assistant"
  status?: "completed"
  content?: OutputContentPart[]
  # function_call fields:
  call_id?: string
  name?: string
  arguments?: string (JSON)

OutputContentPart:
  type: "output_text"
  text: string
  annotations: []
```

## Field Mapping Summary

### Request Direction (Responses → Anthropic)

| Responses API | Anthropic Messages | Notes |
|--------------|-------------------|-------|
| `input` (string) | `messages[{role: "user", content: text}]` | Simple text input |
| `input[].type = "message"` | `messages[{role, content}]` | Role preserved |
| `input[].type = "function_call"` | `messages[{role: "assistant", content: [{type: "tool_use", ...}]}]` | Lifted to content block |
| `input[].type = "function_call_output"` | `messages[{role: "user", content: [{type: "tool_result", ...}]}]` | Lifted to content block |
| `instructions` | `system` | System prompt |
| `tools[].parameters` | `tools[].input_schema` | Schema wrapper |
| `max_output_tokens` | `max_tokens` | Direct map |
| `tool_choice: "required"` | `tool_choice: {type: "any"}` | Different naming |

### Response Direction (Anthropic → Responses)

| Anthropic Messages | Responses API | Notes |
|-------------------|--------------|-------|
| `content[].type = "text"` | `output[].type = "message"` with `content[].type = "output_text"` | Wrapped in message |
| `content[].type = "tool_use"` | `output[].type = "function_call"` | Flattened |
| `content[].type = "thinking"` | `output[].type = "reasoning"` or skip | Optional mapping |
| `stop_reason: "end_turn"` | `status: "completed"` | Direct map |
| `stop_reason: "tool_use"` | `status: "completed"` | With function_call output |
| `stop_reason: "max_tokens"` | `status: "incomplete"` | With incomplete_details |
| `usage.input_tokens` | `usage.input_tokens` | Direct map |
| `usage.output_tokens` | `usage.output_tokens` | Direct map |

### SSE Event Mapping (Anthropic → Responses)

| Anthropic SSE Event | Responses API SSE Event |
|--------------------|----------------------|
| `message_start` | `response.created` |
| `content_block_start` (text) | `response.output_item.added` + `response.content_part.added` |
| `content_block_delta` (text_delta) | `response.output_text.delta` |
| `content_block_stop` | `response.content_part.done` + `response.output_item.done` |
| `content_block_start` (tool_use) | `response.output_item.added` (function_call) |
| `content_block_delta` (input_json_delta) | `response.function_call_arguments.delta` |
| `content_block_stop` (tool_use) | `response.function_call_arguments.done` + `response.output_item.done` |
| `message_delta` | (extract stop_reason, usage for final event) |
| `message_stop` | `response.completed` |
