# HTTP_PROXY Compatibility Fix (#1100)

## Issue Analysis

**Issue #1100**: "开启代理的时候，无法使用 HTTP_PROXY，关闭代理模式，正常"

**User Configuration**:
```json
{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "sk-fce",
    "ANTHROPIC_BASE_URL": "https://xxxai",
    "HTTPS_PROXY": "http://127.0.0.1:1087",
    "HTTP_PROXY": "http://127.0.0.1:1087"
  }
}
```

**Problem**: When CC-Switch proxy mode is enabled, requests return 503 errors if HTTP_PROXY env var is set.

## Root Cause

The current code in `http_client.rs` has this logic:

```rust
// Line 248-256
if system_proxy_points_to_loopback() {
    builder = builder.no_proxy();
    log::warn!(
        "[GlobalProxy] System proxy points to localhost, bypassing to avoid recursion"
    );
}
```

The `system_proxy_points_to_loopback()` function checks if HTTP_PROXY points to localhost, but it doesn't distinguish between:
1. **CC-Switch's own proxy** (which should be bypassed to avoid recursion)
2. **User's external proxy** (v2ray, Clash, etc. - which should be respected)

## Fix Strategy

### Option 1: Only Bypass CC-Switch's Own Proxy (Recommended)

Modify `system_proxy_points_to_loopback()` to only return `true` if the system proxy points to CC-Switch's proxy port, not any localhost proxy.

### Option 2: Add Configuration Flag

Add a setting to control whether to respect HTTP_PROXY when CC-Switch proxy is enabled.

### Option 3: Chain Proxies

Allow CC-Switch proxy to chain through HTTP_PROXY for upstream requests.

## Implementation (Option 1 - Recommended)

### Changes to `http_client.rs`

```rust
// Current code (line 262-275)
fn system_proxy_points_to_loopback() -> bool {
    const KEYS: [&str; 6] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ];

    KEYS.iter()
        .filter_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .any(|value| proxy_points_to_loopback(&value))
}

// Fixed code - only bypass if pointing to CC-Switch's own proxy
fn system_proxy_points_to_loopback() -> bool {
    const KEYS: [&str; 6] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ];

    KEYS.iter()
        .filter_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .any(|value| points_to_cc_switch_proxy(&value))
}

// New helper function - check if proxy points to CC-Switch's own port
fn points_to_cc_switch_proxy(value: &str) -> bool {
    fn is_cc_switch_proxy_port(port: Option<u16>) -> bool {
        let cc_switch_port = get_proxy_port();
        port == Some(cc_switch_port)
    }

    if let Ok(parsed) = url::Url::parse(value) {
        if let Some(host) = parsed.host_str() {
            // Only return true if BOTH:
            // 1. Host is loopback (localhost, 127.0.0.1, ::1)
            // 2. Port matches CC-Switch proxy port
            let is_loopback = host.eq_ignore_ascii_case("localhost")
                || host.parse::<IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false);
            
            return is_loopback && is_cc_switch_proxy_port(parsed.port());
        }
    }
    false
}
```

### Changes to `build_client()` Function

```rust
// Current code (line 248-256)
if system_proxy_points_to_loopback() {
    builder = builder.no_proxy();
    log::warn!(
        "[GlobalProxy] System proxy points to localhost, bypassing to avoid recursion"
    );
} else {
    log::debug!("[GlobalProxy] Following system proxy (no explicit proxy configured)");
}

// Fixed code
if points_to_cc_switch_proxy_from_env() {
    builder = builder.no_proxy();
    log::warn!(
        "[GlobalProxy] System proxy points to CC-Switch's own port, bypassing to avoid recursion"
    );
} else {
    // Respect user's HTTP_PROXY for upstream requests
    log::info!("[GlobalProxy] Following system proxy for upstream requests");
}
```

## Testing

### Test Case 1: User's External Proxy (v2ray/Clash)
```bash
# Set external proxy
export HTTP_PROXY=http://127.0.0.1:1087
export HTTPS_PROXY=http://127.0.0.1:1087

# Enable CC-Switch proxy (port 15721)
# Expected: CC-Switch proxy should work, using external proxy for upstream
# Result: ✅ Should work (no 503 error)
```

### Test Case 2: CC-Switch's Own Proxy
```bash
# Set proxy to CC-Switch port
export HTTP_PROXY=http://127.0.0.1:15721
export HTTPS_PROXY=http://127.0.0.1:15721

# Enable CC-Switch proxy (port 15721)
# Expected: Should bypass to avoid recursion
# Result: ✅ Should work (bypassed)
```

### Test Case 3: No System Proxy
```bash
# No HTTP_PROXY set
unset HTTP_PROXY
unset HTTPS_PROXY

# Enable CC-Switch proxy
# Expected: Direct connection for upstream
# Result: ✅ Should work
```

## Impact

- **Fixes**: #1100 - HTTP_PROXY conflict with proxy mode
- **Preserves**: Recursion avoidance for CC-Switch's own proxy
- **Enables**: Users with v2ray/Clash to use CC-Switch proxy
- **Backward Compatible**: Existing users without HTTP_PROXY unaffected

## Related Issues

- #1069 - Response header handling (may benefit from better proxy chain support)
- #961 - Concurrency limits (separate concern, but proxy chain affects rate limiting)
