# cc-switch Nvidia Provider Failure Analysis

## Problem Statement
The Nvidia provider in cc-switch is failing because the proxy scheme/endpoint handling is "orphaned".

## Root Cause Analysis

### 1. Configuration (Frontend)
In `src/config/claudeProviderPresets.ts`:
```typescript
{
  name: "Nvidia",
  websiteUrl: "https://build.nvidia.com",
  apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://integrate.api.nvidia.com",
      ANTHROPIC_AUTH_TOKEN: "",
      ANTHROPIC_MODEL: "moonshotai/kimi-k2.5",
      ...
    },
  },
  category: "aggregator",
  apiFormat: "openai_chat",  // <-- Critical setting
  icon: "nvidia",
  iconColor: "#000000",
}
```

### 2. Backend Handling (Rust)

The `ClaudeAdapter` in `src-tauri/src/proxy/providers/claude.rs`:

**Line 60-102: API Format Detection**
```rust
fn get_api_format(&self, provider: &Provider) -> &'static str {
    // 1) Preferred: meta.apiFormat
    if let Some(meta) = provider.meta.as_ref() {
        if let Some(api_format) = meta.api_format.as_deref() {
            return if api_format == "openai_chat" {
                "openai_chat"
            } else {
                "anthropic"
            };
        }
    }
    // ... fallbacks for legacy configs
}
```

**Line 298-303: Transform Check**
```rust
fn needs_transform(&self, provider: &Provider) -> bool {
    self.get_api_format(provider) == "openai_chat"
}
```

### 3. Forwarder Endpoint Mapping

In `src-tauri/src/proxy/forwarder.rs` lines 749-757:

```rust
let needs_transform = adapter.needs_transform(provider);

let effective_endpoint =
    if needs_transform && adapter.name() == "Claude" && endpoint == "/v1/messages" {
        "/v1/chat/completions"  // <-- Endpoint gets remapped
    } else {
        endpoint
    };
```

### 4. URL Construction

In `ClaudeAdapter::build_url()` (lines 247-276):

```rust
fn build_url(&self, base_url: &str, endpoint: &str) -> String {
    let mut base = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        endpoint.trim_start_matches('/')
    );

    // Remove duplicate /v1/v1
    while base.contains("/v1/v1") {
        base = base.replace("/v1/v1", "/v1");
    }

    // Add ?beta=true for Claude endpoints
    if (endpoint.contains("/v1/messages") || endpoint.contains("/v1/chat/completions"))
        && !endpoint.contains('?')
    {
        format!("{base}?beta=true")
    } else {
        base
    }
}
```

## The Actual Problem

### For Nvidia Provider:
1. **Client sends**: `POST /v1/messages` (Anthropic format)
2. **Base URL**: `https://integrate.api.nvidia.com`
3. **apiFormat**: `"openai_chat"` (from preset)
4. **Expected**: Transform to OpenAI format and send to `/v1/chat/completions`
5. **Actual URL constructed**: `https://integrate.api.nvidia.com/v1/chat/completions?beta=true`

### Verified: Meta Serialization is Correct
```rust
// src-tauri/src/provider.rs:233-237
#[serde(rename = "apiFormat", skip_serializing_if = "Option::is_none")]
pub api_format: Option<String>,
```
The `#[serde(rename = "apiFormat")]` ensures proper two-way mapping between TypeScript's `apiFormat` and Rust's `api_format`.

## THE ACTUAL BUG: `?beta=true` Parameter on OpenAI Endpoints

### Root Cause
In `ClaudeAdapter::build_url()` (lines 267-275):

```rust
// 为 Claude 相关端点添加 ?beta=true 参数
// 这是某些上游服务（如 DuckCoding）验证请求来源的关键参数
// 注：openai_chat 模式下会转发到 /v1/chat/completions，此处也需要保持一致
if (endpoint.contains("/v1/messages") || endpoint.contains("/v1/chat/completions"))
    && !endpoint.contains('?')
{
    format!("{base}?beta=true")
} else {
    base
}
```

**The bug**: The code adds `?beta=true` to BOTH `/v1/messages` AND `/v1/chat/completions` endpoints.

For Nvidia provider:
- **Input endpoint**: `/v1/messages`
- **Mapped to**: `/v1/chat/completions` (by forwarder)
- **build_url receives**: `/v1/chat/completions`
- **Final URL**: `https://integrate.api.nvidia.com/v1/chat/completions?beta=true` ❌

**Why it fails**: `?beta=true` is an **Anthropic-specific convention** used by some proxy services (like DuckCoding) to verify the request comes from Claude Code. Nvidia's OpenAI Chat Completions endpoint does NOT recognize this parameter and likely rejects it with a 400/404 error.

## Fix

### Fix: Conditional `beta=true` Parameter

**File**: `src-tauri/src/proxy/providers/claude.rs`
**Lines**: 247-276 (specifically 267-275)

**Current Code**:
```rust
// 为 Claude 相关端点添加 ?beta=true 参数
if (endpoint.contains("/v1/messages") || endpoint.contains("/v1/chat/completions"))
    && !endpoint.contains('?')
{
    format!("{base}?beta=true")
} else {
    base
}
```

**Fixed Code**:
```rust
// 为 Claude 原生 /v1/messages 端点添加 ?beta=true 参数
// 注意：不要为 OpenAI Chat Completions (/v1/chat/completions) 添加此参数
if endpoint.contains("/v1/messages")
    && !endpoint.contains("/v1/chat/completions")
    && !endpoint.contains('?')
{
    format!("{base}?beta=true")
} else {
    base
}
```

**Rationale**: The `?beta=true` parameter should only be added for Anthropic's native `/v1/messages` endpoint, not for OpenAI Chat Completions endpoints. When `apiFormat: "openai_chat"` is set, the request is transformed and sent to `/v1/chat/completions`, which is an OpenAI-standard endpoint that doesn't recognize or use the `?beta=true` parameter.

### Alternative Approach: Pass Context to `build_url()`

A cleaner solution would be to pass the `needs_transform` flag to `build_url()`:

```rust
// In ClaudeAdapter
fn build_url(&self, base_url: &str, endpoint: &str, needs_transform: bool) -> String {
    let mut base = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        endpoint.trim_start_matches('/')
    );

    while base.contains("/v1/v1") {
        base = base.replace("/v1/v1", "/v1");
    }

    // Only add ?beta=true for Anthropic format (not OpenAI Chat)
    if endpoint.contains("/v1/messages") && !needs_transform && !endpoint.contains('?') {
        format!("{base}?beta=true")
    } else {
        base
    }
}
```

This requires updating the `ProviderAdapter` trait and all implementations, but is more robust.

## Test Case
To verify the fix:
1. Create a Nvidia provider with `apiFormat: "openai_chat"`
2. Send a test request
3. Check logs for:
   - `api_format=openai_chat`
   - `effective_endpoint=/v1/chat/completions`
   - Final URL: `https://integrate.api.nvidia.com/v1/chat/completions` (without `?beta=true`)
4. Verify request body is in OpenAI format (not Anthropic)

## Files to Modify
**Primary**: `/tmp/cc-switch-sandbox/src-tauri/src/proxy/providers/claude.rs`
- `build_url()` method (lines 267-275) - Fix `?beta=true` condition

**Optional**: Add debug logging in forwarder to verify endpoint mapping:
- `/tmp/cc-switch-sandbox/src-tauri/src/proxy/forwarder.rs` - Add logging at line 760

## Verification Commands
```bash
# Build the project
cd /tmp/cc-switch-sandbox
cargo build

# Check the ClaudeAdapter build_url logic
grep -A 10 "fn build_url" src-tauri/src/proxy/providers/claude.rs

# Search for beta=true usage
grep -rn "beta=true" src-tauri/src/proxy/
```

## Related Code References
- `src-tauri/src/proxy/providers/claude.rs:247-276` - `build_url()` method
- `src-tauri/src/proxy/providers/claude.rs:60-102` - `get_api_format()` method
- `src-tauri/src/proxy/providers/claude.rs:298-303` - `needs_transform()` method
- `src-tauri/src/proxy/forwarder.rs:749-757` - Endpoint mapping logic
- `src-tauri/src/proxy/providers/transform.rs` - Format transformation functions
