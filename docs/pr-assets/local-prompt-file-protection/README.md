# Local prompt-file protection evidence

Refs #7673. This change implements only opt-in local rule-file protection for Claude Code and Codex. API-only synchronization remains outside this PR.

## Screenshots

| Before | Protection enabled |
|---|---|
| ![Before](before.png) | ![After](after.png) |

[After releasing protection, before explicit adoption](released.png).

The screenshots render the actual React components with a controlled Tauri IPC fixture. The displayed rule-file contents come from real Rust/SQLite regression results. They are not packaged native-app or live WebDAV/S3 screenshots. All inputs use synthetic rules and credentials.

## Local validation (2026-10-24)

- macOS ARM64, Node 22.22.2, Rust 1.95.
- Frontend: 255 files / 3,280 tests passed; TypeScript, formatting and renderer build passed.
- Backend with `test-hooks`: 4,028 unit tests + 218 integration tests passed, 0 failed, 11 existing default ignores. Rust formatting and strict Clippy passed.
- The dedicated `prompt_file_protection` suite has 10 passing tests on this platform, including directory changes, legacy migration, release/adoption, stale edits, unreadable policies and files, exact backups, and unresolved parent paths.
- Browser checks confirmed protected controls, copying, policy toggles, and an inactive template after releasing protection, with no page errors.

```sh
cargo test --locked --features test-hooks --manifest-path src-tauri/Cargo.toml --test prompt_file_protection
```

Full backend validation used an isolated `CC_SWITCH_TEST_HOME` and `NO_PROXY=127.0.0.1,localhost,::1` for loopback fixtures. No Windows/Linux native-app or real cloud-storage validation is claimed. This policy governs these applications' prompt-management paths; it is not a filesystem-wide lock against other applications sharing a target or a user explicitly saving another file there.
