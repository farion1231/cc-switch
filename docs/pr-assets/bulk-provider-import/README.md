# JSON batch provider import — review evidence

Related to #4184. These images document the JSON batch importer and its OMP key warning.

| Scenario | Before | After |
|---|---|---|
| OpenCode provider entry | [Single-provider form](before-opencode.png) | [Batch preview](after-opencode.png) |
| OMP identifier-shaped key | [Rejected](before-omp-key.png) | [Warning with confirmation](after-omp-key.png) |

[13-application overview](agents.png).

The screenshots use real React components with controlled Tauri IPC fixtures. The displayed persistence results come from isolated Rust/SQLite integration tests. All credentials, domains and models are synthetic. No real upstream service or Agent was contacted. The UI is shown in its Chinese locale; the labels and behavior are described here in English.

## Validation

Implementation commit: `2ec2974ff69c9c3bb012ba4a53e88b4a3d8254fb`; base: `de0a235ad6389ce4afa333b472cdee615a3f3987`. Local macOS ARM64, Node 22.22.2, Rust 1.95.

- Full backend: 4,287 passed, 0 failed, 11 existing ignored tests (`cargo test --locked --features test-hooks`).
- Full frontend: 3,340 passed in 257 files (`TZ=UTC`, one worker, original timeouts). Frontend source and dependencies are unchanged by the rebase.
- Focused native batch regressions: 20 passed. Focused frontend regressions: 63 passed.
- TypeScript, Prettier, renderer build, Rust formatting and strict Clippy passed.
- Covered 13 applications, independent database connections in DELETE/WAL modes, partial overlap/re-preview, rollback, duplicate identity, existing native-file preservation, SQL backup restore/reopen, and explicit enabling followed by reimport for all seven coexist apps.

An unchanged OAuth callback test intermittently returned IncompleteMessage; an isolated rerun and the final full rerun passed without changing assertions or timeouts. Earlier frontend runs under concurrent compilation also hit an unchanged timeout; the final full suite ran serially.

Packaged-app end-to-end, Windows/Linux native runtime and real gateway availability were not exercised locally. Remote CI results are separate.

OMP native literal-key support is outside this change. The importer stores the supplied key without environment expansion and presents OMP's existing runtime interpretation as a warning; it does not alter OMP's launch or authentication mechanisms.
