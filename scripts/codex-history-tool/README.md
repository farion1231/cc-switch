# Codex History Tool

Standalone Python tool for inspecting and repairing Codex Desktop history
visibility after provider switches.

The tool uses only the Python standard library. It can run directly with
Python, or be packaged as a Windows exe with the included PowerShell build
script.

## What It Repairs

- Active Codex state DB discovery, including `~/.codex/sqlite/state_5.sqlite`.
- `threads.model_provider` bucket drift after provider switches.
- rollout JSONL `payload.model_provider` values.
- `has_user_event` rows when rollout messages prove a user turn exists.
- missing or stale `session_index.jsonl` entries.
- workspace hints in `.codex-global-state.json`.
- recent-window ordering with `sourceFilter=vscode`, `maxPerProject=10`, and
  `maxTotal=300`.

`repair` is a dry run by default. Write mode requires `--apply` and creates a
timestamped backup under Codex home before mutating files.

## List Sessions

```powershell
python .\codex_history_tool.py list --limit 20 --json
```

Useful filters:

```powershell
python .\codex_history_tool.py list --codex-home "$env:USERPROFILE\.codex" --project-path "C:\path\to\project" --query "keyword"
```

## Preview Repair

```powershell
python .\codex_history_tool.py repair --project-path "C:\path\to\project" --json
```

The default target provider is the live top-level `model_provider` from
`~/.codex/config.toml`; if it cannot be read, the tool falls back to `custom`.

## Apply Repair

Close Codex Desktop first unless you intentionally pass `--force`.

```powershell
python .\codex_history_tool.py repair --project-path "C:\path\to\project" --apply
```

## Repair Selected Sessions

Use `list` to find IDs, then pass one or more `--session-id` values.

```powershell
python .\codex_history_tool.py repair --session-id "<session-id>" --apply
```

## Build Windows Exe

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\build-windows-exe.ps1
```

The build writes release-ready artifacts under `release\`:

- `CCSwitchCodexHistoryTool_<version>_python.zip`
- `CCSwitchCodexHistoryTool_<version>_windows_x64.exe`
- `SHA256SUMS-v<version>.txt`

## Session Manager Integration Notes

This tool is designed so the same behavior can later be exposed from the
in-app Session Manager:

- call `list --json` to populate repairable Codex sessions, source/provider
  distributions, and active DB diagnostics;
- call `repair --json` without `--apply` for the preview panel;
- call `repair --apply --json` only after the user confirms the preview and
  Codex Desktop has been closed;
- pass one or more `--session-id` values when the user selected exact sessions
  from the Session Manager list;
- keep the default balanced recent-window options visible as advanced controls
  rather than requiring users to type paths or provider IDs manually.
