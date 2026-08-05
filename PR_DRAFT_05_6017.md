# PR draft: #6017

## Title

fix(recovery): merge common config into the existing Live backup

## Summary

Crash recovery now applies enabled common configuration to the saved Live snapshot instead of rebuilding the file from the provider record. The backup's authentication, model, MCP, and other user settings remain intact. If the optional snippet is malformed, or the current provider cannot be found, recovery keeps the original backup and logs the reason.

## Validation

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib crash_restore_merges_common_config_into_existing_codex_backup`

Submission status: the independent branch is pushed to `KDB-Wind/cc-switch`; this draft targets `farion1231/cc-switch`.
