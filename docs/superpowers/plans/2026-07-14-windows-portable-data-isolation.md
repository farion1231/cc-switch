# Windows Portable Data Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Windows portable build keep every CC Switch-owned runtime file under the executable-adjacent `data` directory while preserving the supported tools' existing configuration locations, Claude Desktop, both source-supported Skills modes, and CLI global installation and upgrade behavior.

**Architecture:** Add a small, cached `portable` path policy initialized before Tauri. Route CC Switch state through that policy, explicitly create the WebView with a portable data directory, replace the window-state plugin only in portable mode, and place defense-in-depth guards at every operation that inherently writes outside the allowed roots.

**Tech Stack:** Rust 1.85 / Tauri 2.10, React 18 / TypeScript / Vitest, PowerShell GitHub Actions.

**Repository note:** `F:\1\cc-switch` is a source snapshot without a working `.git` directory. Commit steps are replaced by file and test checkpoints.

---

## File Map

- Create `src-tauri/src/portable.rs`: marker detection, executable-root path derivation, startup initialization, temp environment, and operation guards.
- Create `src-tauri/src/portable_window_state.rs`: portable-only window geometry persistence in `data\tauri`.
- Modify `src-tauri/src/lib.rs`: initialize portable paths before the panic hook, choose plugins by mode, suppress automatic WebView creation, create/restore the portable window, and save portable window state.
- Modify `src-tauri/src/lightweight.rs`: use the same explicit WebView2 data directory when rebuilding the main window.
- Modify `src-tauri/src/config.rs`, `app_store.rs`, `settings.rs`, `panic_hook.rs`, and `services/env_manager.rs`: remove remaining CC Switch-owned profile writes in portable mode.
- Modify backend command and service files for updater, auto-start, environment mutation, and application-data override guards while preserving Skills and CLI lifecycle behavior.
- Modify `src-tauri/src/commands/import_export.rs`: fix portable save targets under `data\exports` and reject backend export-path escapes.
- Modify `src/App.tsx`, settings components, and `src/hooks/useSettings.ts`: hide or lock operations that the backend rejects.
- Modify `.github/workflows/release.yml`: include the initial `data` layout in future portable archives.

### Task 1: Portable Path Policy

**Files:**
- Create: `src-tauri/src/portable.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing pure path and guard tests**

Add tests in `portable.rs` that construct paths from `D:\Portable\CC-Switch\cc-switch.exe` and assert:

```rust
assert_eq!(paths.root(), Path::new(r"D:\Portable\CC-Switch"));
assert_eq!(paths.data_dir(), Path::new(r"D:\Portable\CC-Switch\data"));
assert_eq!(paths.app_dir(), paths.data_dir().join("app"));
assert_eq!(paths.cache_dir(), paths.data_dir().join("cache"));
assert_eq!(paths.temp_dir(), paths.data_dir().join("temp"));
assert_eq!(paths.tauri_dir(), paths.data_dir().join("tauri"));
assert_eq!(paths.webview2_dir(), paths.data_dir().join("webview2"));
assert!(require_external_write_for_mode(false, "test").is_ok());
assert!(require_external_write_for_mode(true, "test").is_err());
```

Add marker and managed-layout cases proving regular files/directories are
accepted while marker symlinks, dangling marker links, Windows reparse points,
linked `data` directories, and canonical paths escaping the executable root are
rejected.

- [ ] **Step 2: Run the focused Rust test and observe RED**

Run:

```powershell
pwsh.exe -NoProfile -Command '$env:CARGO_HOME="F:\1\.cargo"; $env:RUSTUP_HOME="F:\1\.rustup"; cargo test --manifest-path F:\1\cc-switch\src-tauri\Cargo.toml portable::tests --lib'
```

Expected: failure because `portable` and `PortablePaths` do not exist.

- [ ] **Step 3: Implement the policy**

Implement a `PortablePaths` value derived only from `current_exe().parent()`, cached as `OnceLock<Option<PortablePaths>>`. `initialize()` must detect a sibling regular file named `portable.ini` without following links, reject links and Windows reparse points at the executable root and managed `data` paths, set the cache before filesystem work, create `data`, `data\app`, `data\cache`, `data\temp`, `data\tauri`, and `data\webview2`, validate their canonical containment, verify every derived directory is writable with create-new probe files, then set `TEMP` and `TMP` to `data\temp`.

Expose these stable functions:

```rust
pub fn initialize() -> Result<(), String>;
pub fn is_portable() -> bool;
pub fn paths() -> Option<&'static PortablePaths>;
pub fn require_external_write(operation: &str) -> Result<(), String>;
fn require_external_write_for_mode(portable: bool, operation: &str) -> Result<(), String>;
```

The error must start with `portable mode blocks external write:` so UI and logs receive one predictable message.

- [ ] **Step 4: Initialize before the panic hook**

Declare `mod portable;` in `lib.rs`. At the first line of `run()`, call `portable::initialize()`. Only after successful initialization, call `panic_hook::init_app_config_dir(paths.app_dir().to_path_buf())` when portable paths exist and install the file-writing panic hook. On any initialization failure, panic before installing that hook so the failure cannot write a fallback crash log anywhere.

- [ ] **Step 5: Run the focused test and observe GREEN**

Run the Task 1 test command again. Expected: all `portable::tests` pass.

### Task 2: Route CC Switch-Owned State

**Files:**
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/app_store.rs`
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/panic_hook.rs`
- Modify: `src-tauri/src/services/env_manager.rs`

- [ ] **Step 1: Add failing path-selection tests**

Add pure helper tests proving that portable mode selects `data\app` even when a Store override and a legacy `HOME\.cc-switch` database exist. Add a settings-path test proving `settings.json` is derived from `get_app_config_dir()`. Add an environment-backup test proving the path is `<app_config_dir>\backups`.

- [ ] **Step 2: Run focused tests and observe RED**

Run:

```powershell
pwsh.exe -NoProfile -Command '$env:CARGO_HOME="F:\1\.cargo"; $env:RUSTUP_HOME="F:\1\.rustup"; cargo test --manifest-path F:\1\cc-switch\src-tauri\Cargo.toml --lib'
```

Expected: new assertions fail against the profile-based paths.

- [ ] **Step 3: Apply portable routing**

Make `config::get_app_config_dir()` return `portable.paths().app_dir()` before Store and legacy migration logic. Make `AppSettings::settings_path()` return `get_app_config_dir().join("settings.json")`. Make `env_manager::get_backup_dir()` return `get_app_config_dir().join("backups")`.

In `app_store.rs`, portable mode must:

```rust
// refresh: never construct a Tauri Store
update_cached_override(Some(paths.app_dir().to_path_buf()));

// set: reject before constructing or saving a Store
crate::portable::require_external_write("change app data directory")?;
```

Keep all existing Store and `~` resolution behavior unchanged in normal mode. Update panic-hook path tests so the fallback remains profile-based in normal mode while initialized portable paths point at `data\app`.

- [ ] **Step 4: Run focused tests and observe GREEN**

Run the Task 2 test command again. Expected: all selected tests pass.

### Task 3: WebView2 and Window State

**Files:**
- Create: `src-tauri/src/portable_window_state.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/lightweight.rs`

- [ ] **Step 1: Add failing portable window-state tests**

Test JSON round-tripping for a `PortableWindowState { width, height, x, y, maximized }` and assert `state_path()` equals `data\tauri\.window-state.json` in portable mode.

- [ ] **Step 2: Run the focused test and observe RED**

```powershell
pwsh.exe -NoProfile -Command '$env:CARGO_HOME="F:\1\.cargo"; $env:RUSTUP_HOME="F:\1\.rustup"; cargo test --manifest-path F:\1\cc-switch\src-tauri\Cargo.toml portable_window_state::tests --lib'
```

Expected: failure because the module does not exist.

- [ ] **Step 3: Implement explicit window persistence**

Read and write `.window-state.json` only through `portable.paths().tauri_dir()`. Save size, position, and maximized state using `config::atomic_write`. Restore only valid nonzero sizes and only an on-screen position; malformed or absent state is logged and ignored.

- [ ] **Step 4: Split Tauri plugin registration by mode**

Keep `tauri-plugin-store` and `tauri-plugin-window-state` registered only when `!portable::is_portable()`. In `save_window_state_before_exit`, call the new portable saver in portable mode and `AppHandleExt::save_window_state` otherwise.

- [ ] **Step 5: Create the first and rebuilt WebViews with an absolute data directory**

Before `builder.build(context)`, create a mutable `tauri::Context`. In portable Windows mode, set the configured main window's `create` field to `false`. At the beginning of `.setup`, build it explicitly with:

```rust
WebviewWindowBuilder::from_config(app.handle(), window_config)?
    .data_directory(paths.webview2_dir().to_path_buf())
    .build()?;
```

Extract the builder into a shared helper and use it from `lightweight::exit_lightweight_mode` too, so recreating the window cannot return to the default WebView2 profile. Restore portable window state after the first window is built and before it is shown.

- [ ] **Step 6: Run focused tests and observe GREEN**

Run the Task 3 test command. Expected: all portable window-state tests pass.

### Task 4: Backend External-Write Guards

**Files:**
- Modify: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/commands/env.rs`
- Modify: `src-tauri/src/commands/misc.rs`
- Modify: `src-tauri/src/commands/import_export.rs`
- Modify: `src-tauri/capabilities/default.json`
- Review: `src-tauri/src/services/skill.rs`
- Review: `src-tauri/src/settings.rs`

- [ ] **Step 1: Add failing guard-policy tests**

Extend the guard tests for the prohibited operations: app data override, auto-start, update installation, environment deletion, and environment restoration. Assert normal mode permits them and portable mode rejects them with the stable prefix. Add regression coverage proving that `CcSwitch` remains the default Skills mode, `Unified` remains distinct and selectable, and portable auto-launch status returns `false` without reading the system registration.

- [ ] **Step 2: Run guard tests and observe RED**

Run the Task 1 focused test command. Expected: failures for unimplemented operation coverage.

- [ ] **Step 3: Guard commands before side effects**

Call `portable::require_external_write(...)` as the first executable statement in:

- `install_update_and_restart`
- `set_auto_launch`
- `commands::env::delete_env_vars`
- `commands::env::restore_env_backup`

Return `false` from `get_auto_launch_status` in portable mode without touching auto-launch APIs.

Replace the broad `updater:default` capability with `updater:allow-check`. The
frontend directly needs only update checks; all installation remains behind the
guarded Rust command, preventing a direct updater IPC call from bypassing the
portable restriction while preserving normal installed-build updates.

- [ ] **Step 4: Preserve allowed Skills and tool operations**

Do not add a portable guard to `SkillService::get_ssot_dir`, `migrate_storage`, or `run_tool_lifecycle_action`. Preserve the source-supported `CcSwitch` and `Unified` storage modes and their migration path. `CcSwitch` remains the default and resolves beneath `data\app\skills` because the portable application root is `data\app`; `Unified` remains user-selectable and resolves to the allowed `~\.agents\skills` directory. Claude Desktop configuration and profile operations also remain available.

- [ ] **Step 5: Constrain portable exports**

In portable mode, make `save_file_dialog` skip the native save dialog and return
a sanitized filename beneath `data\exports`, creating that directory on demand.
Before `export_config_to_file` writes, require an absolute normalized target,
canonicalize the `data` root and the nearest existing parent, and reject parent
traversal, external targets, dangling target links, and target or parent links
that resolve outside `data`. Keep normal-mode save dialogs and export destinations
unchanged.

- [ ] **Step 6: Run focused and service tests**

```powershell
pwsh.exe -NoProfile -Command '$env:CARGO_HOME="F:\1\.cargo"; $env:RUSTUP_HOME="F:\1\.rustup"; cargo test --manifest-path F:\1\cc-switch\src-tauri\Cargo.toml --lib; cargo test --manifest-path F:\1\cc-switch\src-tauri\Cargo.toml --test skill_sync'
```

Expected: all tests pass and no guard test performs a side effect.

### Task 5: Portable UI Boundaries

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/hooks/useSettings.ts`
- Modify: `src/components/settings/SettingsPage.tsx`
- Modify: `src/components/settings/AppVisibilitySettings.tsx`
- Modify: `src/components/settings/WindowSettings.tsx`
- Modify: `src/components/settings/DirectorySettings.tsx`
- Review: `src/components/settings/SkillStorageLocationSettings.tsx` (retain both modes)
- Modify: `src/components/settings/AboutSection.tsx`
- Modify: `tests/hooks/useSettings.test.tsx`
- Modify: `tests/components/SettingsDialog.test.tsx`
- Modify: `tests/integration/SettingsDialog.test.tsx`
- Modify: `tests/integration/App.test.tsx`

- [ ] **Step 1: Add failing frontend tests**

Add tests proving that portable mode:

- never calls `setAutoLaunch` or `setAppConfigDirOverride` during settings saves
- locks the CC Switch data directory while keeping all six tool-directory inputs enabled
- keeps both `CcSwitch` and `Unified` Skills storage choices available, with `CcSwitch` as the default
- keeps external tool install/update actions available
- keeps Claude Desktop controls and operations available
- does not mount the environment-repair banner

- [ ] **Step 2: Run the focused Vitest files and observe RED**

```powershell
pwsh.exe -NoProfile -Command 'Set-Location F:\1\cc-switch; pnpm test:unit -- tests/hooks/useSettings.test.tsx tests/components/SettingsDialog.test.tsx tests/integration/SettingsDialog.test.tsx tests/integration/App.test.tsx'
```

Expected: the new assertions fail.

- [ ] **Step 3: Propagate portable state without a permissive loading flash**

In `SettingsPage`, keep the page in its loading state until portable metadata is resolved. Pass `isPortable` only to settings sections that own prohibited controls. In `useSettings`, skip auto-start and app-data-override side effects when portable.

In `App`, hold portable status as `boolean | null`. Until resolved, do not run environment-conflict scans. Claude Desktop remains available because its configuration directories are explicitly allowed.

- [ ] **Step 4: Lock only prohibited controls**

Hide the auto-start/silent-start controls and the CC Switch data-directory editor. Keep both Skills storage modes and migration controls, all tool lifecycle actions, Claude Desktop, provider directory overrides, version checks, diagnostics, portable release-page updates, and normal Skills features available. Do not expose environment-repair mutations in portable mode.

- [ ] **Step 5: Run focused tests and observe GREEN**

Run the Task 5 command again. Expected: all selected Vitest files pass.

### Task 6: GitHub Portable Layout

**Files:**
- Modify: `.github/workflows/release.yml`

- [ ] **Step 1: Add a workflow source assertion**

Add focused workflow source assertions that require the portable block to create `CC-Switch-Portable\data` before `Compress-Archive` and to contain explicit failure branches for missing x64 and ARM64 portable executables:

```powershell
pwsh.exe -NoProfile -Command '$text=Get-Content F:\1\cc-switch\.github\workflows\release.yml -Raw; if($text -notmatch "Join-Path.*portableDir.*data"){ throw "portable data layout missing" }'
```

Expected before the change: command fails.

- [ ] **Step 2: Include the data directory in future archives**

Create the required `data\app`, `data\cache`, `data\temp`, `data\tauri`, and `data\webview2` directories with archive placeholders in the portable packaging block. Validate the ZIP contains the executable, `portable.ini`, and every required data entry. For both x64 and ARM64 matrix jobs, throw an error when the matching portable executable is absent instead of silently skipping ZIP creation. Do not build a ZIP locally.

- [ ] **Step 3: Re-run the workflow assertion**

Expected: exit code 0.

### Task 7: Full Verification

**Files:**
- Review all modified files against `docs/superpowers/specs/2026-07-14-windows-portable-data-isolation-design.md`.

- [ ] **Step 1: Run formatting and static checks**

```powershell
pwsh.exe -NoProfile -Command 'Set-Location F:\1\cc-switch; pnpm typecheck; pnpm format:check; $env:CARGO_HOME="F:\1\.cargo"; $env:RUSTUP_HOME="F:\1\.rustup"; cargo fmt --manifest-path src-tauri\Cargo.toml --check; cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets -- -D warnings'
```

- [ ] **Step 2: Run all automated tests**

```powershell
pwsh.exe -NoProfile -Command 'Set-Location F:\1\cc-switch; pnpm test:unit; $env:CARGO_HOME="F:\1\.cargo"; $env:RUSTUP_HOME="F:\1\.rustup"; cargo test --manifest-path src-tauri\Cargo.toml'
```

- [ ] **Step 3: Run write-path static audits**

Search application source for `.cc-switch`, `store_builder`, `save_window_state`, unscoped `tempdir`, `NamedTempFile::new`, `APPDATA`, and `LOCALAPPDATA`. Classify every remaining occurrence as normal-mode-only, a supported tool or Claude Desktop path, Unified Skills storage, CLI lifecycle/package-manager storage, a comment/test, or a portable-safe path.

- [ ] **Step 4: Verify the release chain without packaging**

Confirm version `3.17.0`, sibling `portable.ini`, and the workflow-created `data` directory are still the inputs for `CC-Switch-v3.17.0-Windows-Portable.zip` and `CC-Switch-v3.17.0-Windows-arm64-Portable.zip`. Confirm each architecture fails its release job when its matching portable executable is absent. Do not create either archive locally.

- [ ] **Step 5: Record runtime acceptance limits**

Document that final Procmon acceptance requires a Windows executable produced by GitHub Actions and a non-`C:` extraction directory. Filter the CC Switch process tree and verify controlled writes are under `data` or the permitted tool roots, including Claude Desktop, Unified Skills storage, CLI global installation, and package-manager cache paths; treat Windows Prefetch, WER, and Event Log writes as OS-managed.
