# Claude Proxy Verification Commands

This note records the exact commands used to verify the Claude proxy fixes.

## Rust tests

Run in `src-tauri/`:

```bash
cargo test openrouter_tools_request_fallbacks_to_default_model --quiet
cargo test openrouter_non_tools_request_keeps_type_specific_mapping --quiet
cargo test test_failover_disabled_uses_current_provider --quiet
cargo test test_failover_disabled_fallbacks_when_current_provider_unroutable --quiet
cargo test estimate_count_tokens --quiet
```

## Build and reinstall app

Run in repo root:

```bash
pnpm build
pkill -f "/cc-switch" || true
cp -R "/Users/jim/work/cc-switch/src-tauri/target/release/bundle/macos/CC Switch.app" "/Applications/CC Switch.app"
open -a "/Applications/CC Switch.app"
```

## Claude CLI smoke tests

Run in target workspace (example: `conductor2`):

```bash
cd /Users/jim/work/conductor2
claude -p "reply exactly: TEST_OK"
claude -p --model claude-sonnet-4-5-20250929 "reply exactly: MODEL_OK"
claude -p --model claude-sonnet-4-5-20250929 "reply exactly: AGAIN_OK"
```

## Optional diagnostics

```bash
tail -n 120 ~/.cc-switch/logs/cc-switch.log
sqlite3 ~/.cc-switch/cc-switch.db "SELECT request_id,provider_id,app_type,model,request_model,status_code,created_at FROM proxy_request_logs ORDER BY created_at DESC LIMIT 12;"
```
