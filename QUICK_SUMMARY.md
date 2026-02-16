# Nvidia Provider Bug - Quick Summary

## The Bug
`ClaudeAdapter::build_url()` incorrectly adds `?beta=true` to OpenAI Chat Completions endpoints.

## Impact
Nvidia provider (`apiFormat: "openai_chat"`) fails because:
1. Requests are transformed from Anthropic to OpenAI format ✓
2. Endpoint is remapped from `/v1/messages` to `/v1/chat/completions` ✓
3. But URL gets `?beta=true` parameter added ✗
4. Nvidia's API rejects requests with `?beta=true` ✗

## Root Cause
```rust
// src-tauri/src/proxy/providers/claude.rs:267-275
if (endpoint.contains("/v1/messages") || endpoint.contains("/v1/chat/completions"))
    && !endpoint.contains('?')
{
    format!("{base}?beta=true")  // BUG: Adds to OpenAI endpoints too!
}
```

## The Fix
```rust
// Only add ?beta=true for Anthropic's /v1/messages, not OpenAI /v1/chat/completions
if endpoint.contains("/v1/messages")
    && !endpoint.contains("/v1/chat/completions")
    && !endpoint.contains('?')
{
    format!("{base}?beta=true")
} else {
    base
}
```

## File to Edit
- `/tmp/cc-switch-sandbox/src-tauri/src/proxy/providers/claude.rs` (line 268)

## Expected URLs After Fix
| Provider | apiFormat | Endpoint | Final URL |
|----------|-----------|----------|-----------|
| Anthropic | anthropic | /v1/messages | `https://api.anthropic.com/v1/messages?beta=true` |
| Nvidia | openai_chat | /v1/chat/completions | `https://integrate.api.nvidia.com/v1/chat/completions` |

## Test
```bash
cd /tmp/cc-switch-sandbox
# Run the test case to verify the bug
cargo test --package cc-switch test_nvidia_endpoint
```
