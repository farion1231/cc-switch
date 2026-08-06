# PR draft: #6101

## Title

fix(proxy): flatten Responses tool schemas for strict Chat gateways

## Summary

Strict Chat Completions gateways can reject Responses tool schemas that still contain root `oneOf`, `$defs`, or other unsupported JSON Schema metadata. The transform expands local `$ref` definitions, flattens union branches into the tool's object properties, keeps fields required by every union branch, and removes unsupported root keywords. It also avoids walking an already expanded schema a second time.

## Validation

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib transform_codex_chat`

Submission status: the independent branch is pushed to `KDB-Wind/cc-switch`; this draft targets `farion1231/cc-switch`.
