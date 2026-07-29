# SSH Runtime Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The user explicitly prohibited staging and commits, so execution must not run any Git write command.

**Goal:** Build the SSH remote runtime foundation and a complete remote provider-management vertical slice while preserving all existing local behavior.

**Architecture:** A headless `cc-switch-agent` reuses the existing Rust database and provider service, exposes an explicit provider command registry over a framed stdio protocol, and is launched by the desktop backend through system OpenSSH. The frontend keeps existing components and routes provider API calls through a runtime-aware invocation wrapper; local-only connection metadata remains on the desktop.

**Tech Stack:** Rust 1.95, Tokio stdio/process APIs, serde/serde_json, Tauri 2 commands/events, system OpenSSH/scp, React 18, TanStack Query, Vitest, existing Radix/shadcn components.

---

## File Map

**Rust protocol and Agent**

- Create `src-tauri/src/remote/mod.rs`: module exports and stable protocol constants.
- Create `src-tauri/src/remote/protocol.rs`: frame kinds, envelopes, codec and protocol errors.
- Create `src-tauri/src/remote/capabilities.rs`: explicit remote command metadata and provider whitelist.
- Create `src-tauri/src/remote/provider_dispatch.rs`: deserialize provider requests and call `ProviderService`.
- Create `src-tauri/src/remote/agent.rs`: handshake, request loop, ping/pong and response writing.
- Create `src-tauri/src/bin/cc-switch-agent.rs`: minimal headless binary entry point.

**Rust desktop transport**

- Create `src-tauri/src/remote/models.rs`: saved target, runtime target, connection status and public DTOs.
- Create `src-tauri/src/remote/target_store.rs`: atomic local persistence in `~/.cc-switch/remote-targets.json`.
- Create `src-tauri/src/remote/ssh.rs`: OpenSSH argument construction, preflight and child lifecycle.
- Create `src-tauri/src/remote/client.rs`: pending requests, framed I/O, heartbeat and event fan-out.
- Create `src-tauri/src/commands/remote.rs`: Tauri commands for target CRUD, switching, status and remote invoke.
- Modify `src-tauri/src/commands/mod.rs`: export remote commands.
- Modify `src-tauri/src/lib.rs`: expose `remote`, manage runtime state and register remote commands.
- Modify `src-tauri/Cargo.toml`: add Tokio `io-std`, `io-util` and `process` features plus Agent binary metadata.

**Frontend runtime and UI**

- Create `src/lib/runtime/types.ts`: runtime DTOs and connection states.
- Create `src/lib/runtime/store.ts`: framework-independent snapshot store for API wrappers.
- Create `src/lib/runtime/invoke.ts`: `appInvoke` and `localInvoke` routing boundary.
- Create `src/lib/api/remote.ts`: target management API and Tauri event subscription.
- Create `src/contexts/RuntimeTargetContext.tsx`: initialization, switching and cache reset.
- Create `src/components/remote/RuntimeTargetSwitcher.tsx`: compact header selector.
- Create `src/components/remote/RemoteTargetsSettings.tsx`: saved server management.
- Modify `src/lib/api/providers.ts`: route provider commands through `appInvoke`.
- Modify `src/main.tsx`: mount `RuntimeTargetProvider` inside QueryClientProvider.
- Modify `src/App.tsx`: render the switcher and disable provider writes while unavailable.
- Modify `src/components/settings/SettingsPage.tsx`: add the remote server settings section.
- Modify `src/i18n/locales/{zh,zh-TW,en,ja}.json`: complete remote runtime strings.

**Tests**

- Create `src-tauri/src/remote/tests.rs`: protocol, registry, store, argument and dispatcher unit tests.
- Create `src-tauri/tests/remote_agent_provider.rs`: stdio Agent provider round trip in a temporary HOME.
- Create `tests/lib/runtimeStore.test.ts`: runtime store and invocation routing tests.
- Create `tests/components/RuntimeTargetSwitcher.test.tsx`: status and switch behavior.
- Modify `tests/msw/tauriMocks.ts`: mock remote Tauri commands and events.

---

### Task 1: Framed Protocol Codec

- [ ] **Step 1: Add failing protocol round-trip and size-limit tests**

In `src-tauri/src/remote/tests.rs`, cover a fragmented reader, multiple consecutive frames, invalid magic, unsupported major version and payloads above 16 MiB. Use a request payload like:

```rust
let request = RpcRequest {
    id: "req-1".into(),
    command: "provider.list".into(),
    args: serde_json::json!({ "app": "codex" }),
    timeout_ms: 30_000,
    operation_id: None,
};
```

- [ ] **Step 2: Run the focused Rust test and confirm red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::protocol -- --nocapture`

Expected: compilation fails because `remote::protocol` does not exist.

- [ ] **Step 3: Implement protocol types and codec**

Define `FrameKind`, `Frame`, `RpcRequest`, `RpcResponse`, `RpcError`, `Hello`, and `HelloAck` in `remote/protocol.rs`. Use an 11-byte header: `CCS1` magic, one-byte kind, two-byte major version and four-byte big-endian payload length. Reject frames above `MAX_FRAME_BYTES = 16 * 1024 * 1024` before allocation. Keep all maintenance comments in Chinese and require Agent logs to use stderr.

- [ ] **Step 4: Run protocol tests and formatting**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::protocol -- --nocapture`

Expected: all protocol tests pass.

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`

Expected: exit code 0.

### Task 2: Explicit Provider Capability Registry

- [ ] **Step 1: Add failing whitelist tests**

Assert that these commands exist and have correct mutation semantics:

```rust
assert!(registry.require("provider.list")?.read_only);
assert!(!registry.require("provider.add")?.read_only);
assert!(matches!(
    registry.require("unknown.command"),
    Err(RemoteError::CommandNotExposed(_))
));
```

The whitelist for this phase is `provider.list`, `provider.current`, `provider.add`, `provider.update`, `provider.delete`, `provider.switch`, and `provider.update_sort_order`.

- [ ] **Step 2: Run the registry test and confirm red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::capability_registry`

Expected: fail because the registry is absent.

- [ ] **Step 3: Implement the deny-by-default registry**

Add `CommandCapability { name, read_only, idempotent, timeout_ms }` and `CommandCapabilityRegistry::provider_phase()`. Do not infer capabilities from Tauri command names; every exposed command must appear in the static registry.

- [ ] **Step 4: Run the registry tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::capability_registry`

Expected: pass.

### Task 3: Headless Provider Dispatcher

- [ ] **Step 1: Add failing dispatcher tests with an in-memory database**

Create an `AppState` backed by `Database::memory()`. Exercise list, add, update, current, switch, sorting and delete through serialized RPC arguments, not direct service calls. Verify malformed app IDs and missing providers return stable error codes.

- [ ] **Step 2: Run dispatcher tests and confirm red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::provider_dispatch -- --nocapture`

Expected: fail because `ProviderDispatcher` is absent.

- [ ] **Step 3: Implement command-to-service dispatch**

`ProviderDispatcher::execute(&AppState, &str, Value)` must parse small typed argument structs and call existing `ProviderService` methods. It must return camelCase JSON matching the frontend API, map `AppError` to `REMOTE_BUSINESS_ERROR`, and never call Tauri window/dialog APIs.

- [ ] **Step 4: Run dispatcher and existing provider tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::provider_dispatch services::provider -- --nocapture`

Expected: new and existing provider tests pass.

### Task 4: Agent Handshake and Stdio Loop

- [ ] **Step 1: Add a failing duplex-stream Agent test**

Start `run_session(reader, writer, state)` against `tokio::io::duplex`. Verify the client must send `hello` first, receives `hello_ack`, can issue `provider.list`, receives `pong`, and gets `PROTOCOL_ORDER` for a request before handshake.

- [ ] **Step 2: Run the Agent session test and confirm red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::agent_session -- --nocapture`

Expected: fail because the Agent loop is absent.

- [ ] **Step 3: Implement Agent session and binary**

Add `remote::agent::run_stdio()` that initializes `Database::init()` and `AppState::new`, then serves one stdin/stdout session. Add `src/bin/cc-switch-agent.rs` containing only:

```rust
#[tokio::main]
async fn main() {
    if let Err(error) = cc_switch_lib::remote::agent::run_stdio().await {
        eprintln!("cc-switch-agent: {error}");
        std::process::exit(1);
    }
}
```

Add the required Tokio features and ensure protocol output never shares stdout with logs.

- [ ] **Step 4: Run Agent tests and build the binary**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::agent_session`

Expected: pass.

Run: `cargo build --manifest-path src-tauri/Cargo.toml --bin cc-switch-agent`

Expected: `target/debug/cc-switch-agent` or `.exe` exists.

### Task 5: Local Target Store and SSH Argument Safety

- [ ] **Step 1: Add failing persistence and validation tests**

Cover unique IDs, trimmed display names, OpenSSH Host aliases, optional username/port/key overrides, atomic save/load, corrupt JSON recovery, and rejection of aliases or paths containing control characters. Verify arguments remain separate array items for aliases containing no shell syntax.

- [ ] **Step 2: Run store and SSH tests and confirm red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::target_store remote::tests::ssh_args`

Expected: fail because models/store/SSH builder are absent.

- [ ] **Step 3: Implement target models, atomic store and command builder**

Persist `RemoteTargetConfig` and `active_target_id` in `~/.cc-switch/remote-targets.json`. Use temporary-file plus rename writes. Build OpenSSH and scp commands with `Command::args`; never concatenate a local shell string. Always include `BatchMode=yes` and `StrictHostKeyChecking=yes`.

- [ ] **Step 4: Implement preflight result mapping**

Map common stderr patterns to `SSH_NOT_FOUND`, `HOST_KEY_NOT_TRUSTED`, `AUTH_FAILED`, `REMOTE_UNREACHABLE`, `REMOTE_PLATFORM_UNSUPPORTED` and `REMOTE_ARCH_UNSUPPORTED`. Sanitize stderr before returning it to the frontend.

- [ ] **Step 5: Run focused tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::target_store remote::tests::ssh_args`

Expected: pass.

### Task 6: Desktop Remote Client and Tauri Commands

- [ ] **Step 1: Add failing fake-process client tests**

Use a `RemoteProcess` trait so tests can supply duplex streams. Verify request correlation, timeout cleanup, ping/pong, child exit propagation, cancellation and rejection of mutation replay after disconnect.

- [ ] **Step 2: Run client tests and confirm red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::client -- --nocapture`

Expected: fail because the client and manager are absent.

- [ ] **Step 3: Implement `RemoteConnectionManager`**

Store one active session behind Tokio synchronization. Emit `remote-runtime-status` with a monotonically increasing generation. A target switch must close the old session, fail its pending requests, connect the new target, perform handshake, and only then publish `online`.

- [ ] **Step 4: Add local Tauri commands**

Expose target CRUD, test connection, set active target, get runtime snapshot, disconnect and `remote_invoke`. `remote_invoke` must consult the capability registry before sending. Register the manager in Tauri state during setup.

- [ ] **Step 5: Run client and command serialization tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remote::tests::client remote::tests::command_dto`

Expected: pass.

### Task 7: Runtime-Aware Frontend Invocation

- [ ] **Step 1: Add failing runtime store and invocation tests**

Mock Tauri `invoke`. Verify local mode calls the original command, remote online mode wraps provider calls in `remote_invoke`, remote offline mode rejects mutations with `REMOTE_OFFLINE`, and connection-management commands always use `localInvoke`.

- [ ] **Step 2: Run Vitest and confirm red state**

Run: `pnpm test:unit -- tests/lib/runtimeStore.test.ts`

Expected: fail because runtime modules are absent.

- [ ] **Step 3: Implement the runtime store and wrappers**

Use `useSyncExternalStore` compatibility: the plain store exports `getSnapshot`, `subscribe`, and an internal `setSnapshot`. `appInvoke` reads the current snapshot synchronously so existing API objects remain usable outside React components.

- [ ] **Step 4: Migrate provider API only**

Replace imports of Tauri `invoke` in `src/lib/api/providers.ts` with `appInvoke`. Keep `updateTrayMenu` and `openTerminal` local-only in this phase; route provider CRUD, current, sorting and switching remotely.

- [ ] **Step 5: Run focused and existing provider tests**

Run: `pnpm test:unit -- tests/lib/runtimeStore.test.ts tests/hooks/useProviderActions.test.tsx tests/components/ProviderList.test.tsx`

Expected: pass.

### Task 8: Existing-Style Runtime UI

- [ ] **Step 1: Add failing switcher tests**

Test local/remote labels, online/connecting/offline/incompatible indicators, disabled switching while a transition is active, and the “manage servers” command. Use existing Button, DropdownMenu, Dialog, Input and status colors.

- [ ] **Step 2: Run the component test and confirm red state**

Run: `pnpm test:unit -- tests/components/RuntimeTargetSwitcher.test.tsx`

Expected: fail because the component is absent.

- [ ] **Step 3: Implement context and compact header switcher**

Mount the provider under QueryClientProvider. On generation changes call `queryClient.clear()` before rendering remote data. Add the switcher beside the existing profile/application controls without changing header height or established spacing.

- [ ] **Step 4: Implement server management in Settings**

Add list, add/edit dialog, delete confirmation and test-connection action. The basic form contains display name and Host alias; username, port and private-key override live under an advanced collapsible section. Do not add password fields.

- [ ] **Step 5: Add all four locale sets and run tests**

Run: `pnpm test:unit -- tests/components/RuntimeTargetSwitcher.test.tsx tests/integration/SettingsDialog.test.tsx tests/integration/App.test.tsx`

Expected: pass with no missing translation warnings.

### Task 9: Provider Vertical-Slice Integration Verification

- [ ] **Step 1: Add Agent process integration test**

Launch the built Agent against an isolated `CC_SWITCH_TEST_HOME`, perform handshake, add a Codex provider, list it, switch it, verify current, update it, sort it and delete it. Assert the Agent exits after stdin closes.

- [ ] **Step 2: Run the integration test**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test remote_agent_provider -- --nocapture`

Expected: pass.

- [ ] **Step 3: Run full frontend verification**

Run: `pnpm typecheck`

Expected: exit code 0.

Run: `pnpm test:unit`

Expected: all frontend tests pass.

Run: `pnpm build:renderer`

Expected: Vite production build succeeds.

- [ ] **Step 4: Run full Rust verification**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: all commands exit 0.

- [ ] **Step 5: Manual desktop smoke test**

Start `pnpm dev`, confirm local mode behavior is unchanged, add a target using an existing trusted OpenSSH Host alias, test the connection, switch to it, complete provider CRUD/switching, disconnect the SSH session, verify offline write protection, reconnect, and switch back to local without data leakage.

---

## Explicit Follow-On Plans

After this plan passes, create separate plans in this order:

1. MCP, Prompts, Skills, Profiles and universal provider commands.
2. Remote file transfer, import/export, backups and sync.
3. Proxy, failover, usage, request logs and stream checks.
4. Sessions, workspace, OpenClaw and Hermes Linux capabilities.
5. OAuth/subscription flows, signed Agent release artifacts, upgrade rollback and complete capability audit.

No follow-on plan may bypass the capability registry or introduce a second transport path.
