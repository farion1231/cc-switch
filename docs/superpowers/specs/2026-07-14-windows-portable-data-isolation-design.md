# Windows Portable Data Isolation Design

## Goal

When `portable.ini` exists next to the Windows executable, CC Switch must keep all
application-owned persistent data, caches, temporary files, WebView2 data, and
plugin state under `<exe-root>\data`. The installed build must retain its current
behavior.

The portable package executable and `portable.ini` remain at the package root.
The rule applies to files created or modified at runtime.

## Allowed External Paths

Portable mode may continue to read and write the configured locations belonging
to these supported tools:

- Claude Code, including the companion `.claude.json` file
- Claude Desktop, including its `%APPDATA%` / `%LOCALAPPDATA%` configuration and
  profile directories
- Unified Skills shared storage at `~\.agents\skills`
- tool-specific Skills synchronization targets, including
  `~\.claude-desktop\skills`
- Supported CLI global installation, upgrade, and package-manager cache paths
- Codex
- Gemini
- OpenCode
- OpenClaw
- Hermes

The source-supported Skills storage modes remain `CcSwitch` and `Unified` in
portable mode. `CcSwitch` remains the default and stores its SSOT under
`data\app\skills`; `Unified` remains user-selectable and may write its SSOT to
`~\.agents\skills`. Portable mode must not force either mode or disable
migration between them.

Default and user-overridden tool paths remain unchanged. They must not be
relocated by globally replacing `APPDATA`, `LOCALAPPDATA`, `USERPROFILE`, or
`HOME`.

Windows-managed writes that an application cannot redirect, such as Prefetch,
WER, and Event Log activity, are outside this design. CC Switch, its Tauri
plugins, its WebView2 profile, and temporary work created on its behalf are in
scope.

## Portable Layout

The executable directory is resolved with `current_exe().parent()`, never the
process working directory. Portable paths are derived centrally:

```text
<exe-root>\
  cc-switch.exe
  portable.ini
  data\
    app\          database, settings, backups, logs, crash reports, CcSwitch Skills
    cache\        reserved CC Switch-owned cache data
    exports\      SQL/config exports, created on first portable export
    temp\         process and tempfile work
    tauri\        portable window state
    webview2\     WebView2 profile, cookies, cache, GPU data, Crashpad data
```

The required startup subdirectories are `data\app`, `data\cache`, `data\temp`,
`data\tauri`, and `data\webview2`. `data\exports` is created on demand.

Subdirectories may be added only beneath `data`.

## Architecture

A new backend module owns portable detection and path derivation. Detection is
cached and available before the panic hook, Tauri builder, plugins, or WebView
are initialized.

At portable startup it will:

1. Require `portable.ini` to be a regular file, reject links or Windows reparse
   points at the executable root and every managed `data` path, then verify the
   canonical `data` layout cannot escape the executable root.
2. Create the required `data` directories.
3. Point `TEMP` and `TMP` at `data\temp` before any temporary file is created.
4. Point the WebView2 user data folder at `data\webview2` before the main window
   is constructed.
5. Make the application config root `data\app` without consulting the Tauri
   Store or the legacy `~\.cc-switch` directory.
6. Do not initialize the Tauri Store or window-state plugins. Persist portable
   window state explicitly at `data\tauri\.window-state.json`.
7. Return portable export targets beneath `data\exports`. Before the database
   writes an export, normalize and resolve the target and its existing ancestors,
   rejecting relative paths, parent traversal, external destinations, and link or
   reparse-point escapes from the canonical portable `data` root.

Normal mode continues to use all existing path resolution and migration logic.
Portable mode never imports or silently migrates data from `~\.cc-switch`.

## Write Restrictions

Portable mode must reject or hide operations whose purpose requires writes
outside `data` and the allowed tool locations:

- changing the CC Switch application data directory
- enabling or disabling Windows auto-start
- installing an in-app CC Switch update
- Windows environment conflict repair that mutates registry environment values
- exporting CC Switch data to an external or link-escaped destination

Update checks that do not stage an installer may remain available. Provider
configuration, MCP, prompt, and session operations remain available when their
targets are within the allowed tool-owned locations.

These restrictions do not apply to Claude Desktop, Skills storage selection or
migration, or supported CLI installation and upgrade actions. Their writes may
continue at the allowed external paths above, including package-manager caches.

Backend commands enforce these restrictions even if called outside the UI. The
UI disables or omits unavailable controls so users do not encounter avoidable
errors.

## Packaging

The existing GitHub release workflow continues to identify portable builds with
`portable.ini`. It also includes the `data` directory in the portable archive so
the expected layout is visible immediately after extraction. No local ZIP is
required for this task. Both Windows x64 and ARM64 jobs must fail if their
architecture-specific portable executable is missing; a missing executable must
never produce a marker-only or incomplete portable archive.

## Error Handling

If `<exe-root>\data` cannot be created or written, startup stops with a clear
error rather than falling back to a user-profile path. A portable-only command
that would violate the write policy returns a stable error and performs no
partial mutation.

Any portable initialization failure, including marker or managed-path reparse
validation, aborts before the file-writing panic hook is installed. The failure
therefore cannot create a fallback crash log in the user profile or any other
filesystem location.

## Verification

Automated Rust tests cover:

- marker detection and executable-root derivation
- marker and managed-path link/reparse rejection plus canonical containment
- every derived portable path staying beneath `data`
- application config resolution bypassing Store and legacy home migration
- portable command guards for updater, auto-start, and environment repair
- portable exports defaulting to `data\exports` and rejecting external, relative,
  traversal, dangling-link, and linked-parent escapes
- `CcSwitch` remaining the default Skills mode while `Unified` remains available
- CLI lifecycle and Claude Desktop operations remaining unguarded
- x64 and ARM64 release jobs failing when their portable executable is absent
- unchanged non-portable path behavior

Frontend tests cover disabled portable-only controls. Static searches verify
that application-owned hard-coded `~\.cc-switch`, default Tauri data paths, and
unscoped temporary-file creation no longer bypass the portable path layer.

Final Windows runtime acceptance uses Procmon with a fresh user profile and a
portable build located outside `C:`. The test exercises cold start, settings,
provider switching, temporary download/extraction paths, window close/reopen,
and exit. Writes by the CC Switch process must be confined to `data` and the
allowed tool locations, including Claude Desktop, Unified Skills storage, CLI
global installation and package-manager cache paths.
