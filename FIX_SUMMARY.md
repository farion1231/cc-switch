# Fix Applied: Nvidia Provider Endpoint Bug

## Summary
Fixed the Nvidia provider failure by preventing `?beta=true` parameter from being added to OpenAI Chat Completions endpoints.

## Changes Made

### 1. Modified `/tmp/cc-switch-sandbox/src-tauri/src/proxy/providers/claude.rs`

#### Lines 266-278: Fixed `build_url()` method
```rust
// OLD CODE (BUGGY):
if (endpoint.contains("/v1/messages") || endpoint.contains("/v1/chat/completions"))
    && !endpoint.contains('?')
{
    format!("{base}?beta=true")
} else {
    base
}

// NEW CODE (FIXED):
// 为 Claude 原生 /v1/messages 端点添加 ?beta=true 参数
// 这是某些上游服务（如 DuckCoding）验证请求来源的关键参数
// 注意：不要为 OpenAI Chat Completions (/v1/chat/completions) 添加此参数
//       当 apiFormat="openai_chat" 时，请求会转发到 /v1/chat/completions，
//       但该端点是 OpenAI 标准，不支持 ?beta=true 参数
if endpoint.contains("/v1/messages")
    && !endpoint.contains("/v1/chat/completions")
    && !endpoint.contains('?')
{
    format!("{base}?beta=true")
} else {
    base
}
```

#### Lines 511-523: Added new test case
```rust
#[test]
fn test_build_url_no_beta_for_openai_chat_completions() {
    let adapter = ClaudeAdapter::new();
    // OpenAI Chat Completions 端点不添加 ?beta=true
    // 这是 Nvidia 等 apiFormat="openai_chat" 供应商使用的端点
    let url = adapter.build_url("https://integrate.api.nvidia.com", "/v1/chat/completions");
    assert_eq!(url, "https://integrate.api.nvidia.com/v1/chat/completions");
}
```

## Test Results
```
running 13 tests
test proxy::providers::claude::tests::test_needs_transform ... ok
test proxy::providers::claude::tests::test_extract_base_url_from_env ... ok
test proxy::providers::claude::tests::test_build_url_no_beta_for_other_endpoints ... ok
test proxy::providers::claude::tests::test_build_url_no_beta_for_openai_chat_completions ... ok
test proxy::providers::claude::tests::test_provider_type_detection ... ok
test proxy::providers::claude::tests::test_build_url_preserve_existing_query ... ok
test proxy::providers::claude::tests::test_extract_auth_anthropic_api_key ... ok
test proxy::providers::claude::tests::test_extract_auth_anthropic ... ok
test proxy::providers::claude::tests::test_extract_auth_openrouter ... ok
test proxy::providers::claude::tests::test_extract_auth_claude_auth_mode ... ok
test proxy::providers::claude::tests::test_extract_auth_claude_auth_env_mode ... ok
test proxy::providers::claude::tests::test_build_url_anthropic ... ok
test proxy::providers::claude::tests::test_build_url_openrouter ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured
```

## Impact

### Before Fix
- Nvidia URL: `https://integrate.api.nvidia.com/v1/chat/completions?beta=true` ❌
- Result: Nvidia API rejects the request (doesn't recognize `?beta=true`)

### After Fix
- Nvidia URL: `https://integrate.api.nvidia.com/v1/chat/completions` ✅
- Result: Nvidia API accepts the request

### Backward Compatibility
- Anthropic `/v1/messages`: Still gets `?beta=true` ✅
- OpenRouter `/v1/messages`: Still gets `?beta=true` ✅
- Other endpoints: No change ✅

## Verification
To test the Nvidia provider after applying this fix:
1. Build cc-switch: `cd /tmp/cc-switch-sandbox && npm run tauri build`
2. Configure Nvidia provider with `apiFormat: "openai_chat"`
3. Send a test request
4. Check logs for: `[Claude] >>> 请求 URL: https://integrate.api.nvidia.com/v1/chat/completions`
5. Verify no `?beta=true` parameter is present
6. Verify request body is in OpenAI format (not Anthropic)

## Files Modified
- `/tmp/cc-switch-sandbox/src-tauri/src/proxy/providers/claude.rs` (2 changes)

## Files Created (for reference)
- `/tmp/cc-switch-sandbox/NVIDIA_FAILURE_ANALYSIS.md` - Detailed analysis
- `/tmp/cc-switch-sandbox/QUICK_SUMMARY.md` - Quick reference
- `/tmp/cc-switch-sandbox/test_nvidia_endpoint.rs` - Standalone test case
- `/tmp/cc-switch-sandbox/FIX_SUMMARY.md` - This file
