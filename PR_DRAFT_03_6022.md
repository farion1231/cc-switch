# PR draft: #6022

## Title

fix(proxy): honor the selected provider before failover fallback

## Summary

Switching providers from the tray now keeps the stored `auto_failover_enabled` value instead of turning automatic failover off.

When automatic failover is enabled, the proxy now tries the selected provider first, then follows the configured failover queue. A provider already in the queue is skipped after the first attempt, and an unavailable current provider still falls back to the next available queue entry. The switch does not change the queue itself.

## Validation

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::provider_router` (7 passed)
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` (2312 passed; 2 Windows symlink tests could not run because the process lacked privilege 1314)

Submission status: the independent branch targets `farion1231/cc-switch`. The updated commit is local and has not been pushed.
