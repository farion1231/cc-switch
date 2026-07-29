# Ephemeral SSH Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. The user explicitly prohibited staging, commits, branches and worktrees; all Git write steps are omitted.

**Goal:** Make SSH remote mode work against an unprepared Linux server by transparently launching a minimal, statically linked, session-scoped Agent.

**Architecture:** Extract remote-safe business behavior into a Tauri-free core, build a standalone Agent against that core, embed signed architecture-specific bytes in the desktop, and deploy them only to a random remote temporary path for the lifetime of the SSH session.

**Tech Stack:** Rust 1.85+, Cargo workspace crates, musl Linux targets, serde/serde_json framed stdio protocol, rusqlite bundled SQLite, SHA-256, system OpenSSH/scp, existing Tauri runtime and Rust/Vitest tests.

---

## File Map

- Create `src-tauri/crates/cc-switch-protocol/`: shared frame DTOs, codec and capability metadata.
- Create `src-tauri/crates/cc-switch-core/`: Tauri-free state and Provider command service.
- Create `src-tauri/crates/cc-switch-agent/`: minimal `--stdio` binary and session loop.
- Create `src-tauri/src/remote/embedded_agent.rs`: architecture selection, metadata and embedded-byte access.
- Create `src-tauri/src/remote/ephemeral_deploy.rs`: random remote path, scp arguments, launch command and cleanup guard.
- Modify `src-tauri/Cargo.toml`: workspace members and desktop dependencies on protocol/core.
- Modify `src-tauri/src/remote/{protocol,capabilities,agent,provider_dispatch}.rs`: replace implementations with compatibility re-exports or remove desktop-only copies.
- Modify `src-tauri/src/remote/ssh.rs`: remove persistent version directory and external artifact lookup; use ephemeral deployer.
- Modify `.github/workflows/{ci,release}.yml`: build and verify musl Agent artifacts before desktop packaging.
- Modify `src-tauri/tauri.conf.json`: include generated Agent resources when resource embedding is used by the selected Tauri target.
- Test `src-tauri/tests/remote_agent_minimal.rs`: dependency and binary-boundary assertions.
- Test `src-tauri/tests/remote_ephemeral_deploy.rs`: transport construction and cleanup behavior.

### Task 1: Freeze the Minimal Agent Boundary

- [ ] Add a failing `remote_agent_minimal` test that loads `cargo metadata` for `cc-switch-agent` and rejects dependency names containing `tauri`, `gtk`, `webkit`, `wry` or `muda`.
- [ ] Run `cargo test -j 1 --test remote_agent_minimal` and confirm failure because the standalone package does not exist.
- [ ] Create the three workspace package manifests with Chinese maintenance comments in source files and the smallest dependency sets required by their responsibilities.
- [ ] Move the protocol codec and capability registry into `cc-switch-protocol`; keep desktop import paths stable with re-exports.
- [ ] Run protocol, capability and dependency-boundary tests until green.

### Task 2: Extract the Headless Provider Core

- [ ] Add failing core integration tests covering a temporary HOME, provider list/add/update/delete/switch/sort and live-file side effects without constructing Tauri state.
- [ ] Introduce `HeadlessState` containing only database and remote-safe services; it must not construct `ProxyServer`, tray state or window state.
- [ ] Move shared Provider DTOs, database access and live configuration writers behind `cc-switch-core` APIs. Desktop `ProviderService` delegates to the core rather than retaining a second implementation.
- [ ] Make background sync hooks injectable/no-op in Agent mode so database initialization does not pull WebDAV/S3/Tauri lifecycle modules into the Agent.
- [ ] Run existing Provider tests plus core integration tests and resolve all behavior differences before proceeding.

### Task 3: Build the Standalone Stdio Agent

- [ ] Add a failing process test that builds `cc-switch-agent`, performs hello/hello_ack and Provider CRUD through stdin/stdout, then confirms EOF exits cleanly.
- [ ] Implement the Agent session against `HeadlessState` and shared protocol crate; stderr is the only diagnostic channel.
- [ ] Add `--version` and `--stdio` modes with stable exit codes for unsupported invocation.
- [ ] Run `cargo tree -p cc-switch-agent` and the banned-dependency test; both must prove the GUI stack is absent.
- [ ] Build on Linux musl and verify `file` reports a static executable and `ldd` reports “not a dynamic executable”.

### Task 4: Specify Ephemeral Deployment Before Changing SSH

- [ ] Add failing tests for `EphemeralAgentSpec::for_architecture`, random `/tmp/cc-switch-agent-<hex>` paths, SHA-256 metadata, scp argument isolation and cleanup command construction.
- [ ] Add failing lifecycle tests for upload failure, handshake failure, normal EOF and child kill; every path must schedule idempotent remote cleanup.
- [ ] Implement `embedded_agent.rs` and `ephemeral_deploy.rs` without changing the active `OpenSshSession` path yet.
- [ ] Run the focused deployment tests and security assertions until green.

### Task 5: Replace Persistent Agent Installation

- [ ] Change the connection regression test to expect no `~/.cc-switch/agents/<version>` checks and no `CC_SWITCH_AGENT_ARTIFACT` lookup.
- [ ] Update `OpenSshSession::connect` to preflight, select embedded bytes, upload to the random temporary path, launch `--stdio`, and retain a cleanup guard.
- [ ] Map upload, integrity, start and compatibility failures to the new stable error codes.
- [ ] Remove `ensure_agent`, `resolve_agent_artifact` and the persistent install path only after the new test is green.
- [ ] Verify disconnect, target switch and delete-active-target all terminate the Agent and clean the temporary file.

### Task 6: Build and Embed Both Linux Architectures

- [ ] Add CI assertions that fail when either `linux-x86_64` or `linux-aarch64` static Agent is absent.
- [ ] Build musl Agents before each desktop target, record length/SHA-256 metadata and embed them into the desktop resource boundary.
- [ ] Keep a development-only explicit override for test fixtures; production must reject external paths and use embedded bytes.
- [ ] Verify Windows, macOS and Linux desktop builds can access both Agent metadata entries without requiring a local Linux toolchain at application runtime.

### Task 7: Real Clean-Server Verification

- [ ] Start a clean Linux sshd container with no Rust, Node or CC Switch installation.
- [ ] Connect using the desktop SSH client, complete handshake and Provider vertical slice, then disconnect.
- [ ] Assert no listener was opened and no `cc-switch-agent*` file remains under HOME, `/tmp` or `/dev/shm`.
- [ ] Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, serial Rust tests, frontend tests, typecheck and renderer build.
- [ ] Start the desktop app in normal development mode and manually confirm the existing SSH UI now connects without server preparation.

## Self-Review

- Spec coverage: zero manual server setup, minimal binary, static linking, embedded delivery, temporary cleanup, error semantics and real sshd verification all have explicit tasks.
- Placeholder scan: no deferred implementation placeholders remain; follow-on functionality continues through the shared capability registry.
- Type consistency: `HeadlessState`, `EphemeralAgentSpec`, shared protocol DTOs and error-code names are stable across tasks.
- Git constraint: the plan contains no staging, commit, branch, PR or worktree operation.
