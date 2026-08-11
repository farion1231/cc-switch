# PR draft: #6140

## Title

fix(proxy): keep zero cache details in Responses usage

## Summary

Responses usage always includes `input_tokens_details.cached_tokens`. The value is `0` when the upstream reports zero or omits `prompt_tokens_details`; `cache_write_tokens` is included only when it is positive. This keeps the response shape valid for clients that treat `input_tokens_details` as required.

## Validation

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib transform_codex_chat`

Submission status: the independent branch is pushed to `KDB-Wind/cc-switch`; this draft targets `farion1231/cc-switch`.
