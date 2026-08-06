# PR draft: #6115

## Title

fix(codex): refresh the upstream model after a provider switch

## Summary

Native Codex Responses requests now use the destination provider's upstream model before forwarding. A stale client model from the previous provider is replaced, while a model that is still present in the destination catalog remains selectable. The resolved model is used for outbound-model tracking. Chat and Anthropic compatibility routes keep their existing behavior, and the official Codex provider is left untouched.

## Validation

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib native_codex_responses_refreshes_stale_client_model_after_provider_switch`

Submission status: the independent branch is pushed to `KDB-Wind/cc-switch`; this draft targets `farion1231/cc-switch`.
