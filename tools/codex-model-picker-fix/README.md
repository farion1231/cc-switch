# Codex Desktop Model Picker Fix

Codex Desktop hides custom-provider models returned by `model/list` because the
renderer applies a Statsig allowlist (`use_hidden_models: true` +
`available_models` containing only OpenAI stock models). This is tracked
upstream as [openai/codex#19694](https://github.com/openai/codex/issues/19694).

This project patches the cached Statsig evaluations in the Codex Desktop
`Local Storage` LevelDB so that:

- `use_hidden_models` becomes `false`
- the cache timestamp is pushed 30 days into the future
- the Statsig endpoints (`ab.chatgpt.com`, `statsigapi.net`, `api.statsigcdn.com`,
  `prodregistryv2.org`, `featureassets.org`) are blocked in `hosts` and added to the
  system proxy bypass list so the remote policy cannot be refreshed back over the network

No model whitelist is written. With `use_hidden_models=false`, the renderer
shows every non-hidden model returned by `model/list`, so models added to
CC Switch later appear automatically after restarting Codex.

## Requirements

- Windows with the MSIX Codex Desktop app (`OpenAI.Codex_*` under
  `%LOCALAPPDATA%\Packages`)
- Node.js
- Administrator rights (for the `hosts` file update)

## Usage

1. Fully quit Codex Desktop, including the system tray icon.
2. Run the script as administrator:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\fix-codex-model-picker.ps1
   ```

   The script automatically elevates itself when needed.

3. Relaunch Codex Desktop. The model picker shows every model from your
   `model_catalog_json`, including models added later.

`-ModelIds` is optional and only adds extra model IDs to the Statsig
`available_models` list as a belt-and-suspenders measure:

```powershell
.\fix-codex-model-picker.ps1 -ModelIds "model-a,model-b"
```

## What it changes

- `%LOCALAPPDATA%\Packages\OpenAI.Codex_*\LocalCache\Roaming\Codex\web\Codex\Default\Local Storage\leveldb`
  (a backup is created next to it before patching)
- `C:\Windows\System32\drivers\etc\hosts` (`127.0.0.1 <statsig-domain>` for every
  Statsig endpoint used by the renderer)
- `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings\ProxyOverride`
  (Statsig domains are added when a system proxy is active)

## Rollback

- Remove the `127.0.0.1 <statsig-domain>` lines added by the script from `hosts`.
- Remove the Statsig domains from `ProxyOverride` in
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` if you no
  longer want them bypassed.
- Replace the patched `leveldb` folder with the `leveldb.bak-<timestamp>`
  backup created by the script.

## Why a script instead of a Codex setting

The filter lives in the Desktop renderer and is driven by a remote Statsig
dynamic config (`107580212`). There is no user-facing setting to disable it,
and CC Switch's upstream fix
([farion1231/cc-switch#5265](https://github.com/farion1231/cc-switch/pull/5265))
has not been merged yet.
