# Portable Claude provider bundles

An integration can transfer selected Claude Code or Claude Desktop providers without restoring a database or copying the client's live settings file. Open CC Switch normally on each device first to initialize its database. Before importing, quit CC Switch on the destination, including its tray process, and pause any concurrent configuration sync. Reopen it after importing to load the new rows. The standalone CLI writes through SQLite and does not notify a running app.

| Command | Standard input | Standard output |
| --- | --- | --- |
| `cc-switch provider-bundle list` | Empty | Provider names, native IDs, apps, modes and credential status |
| `cc-switch provider-bundle export` | Selection and optional credential permission | Versioned portable provider bundle |
| `cc-switch provider-bundle preview` | Portable bundle | Provider summaries with importability and a reason when unavailable |
| `cc-switch provider-bundle import` | Portable bundle | Imported and skipped counts, and reasons for entries that could not be prepared |

An export selection uses the native ID reported by `list`:

```json
{"profiles":[{"app":"claude","id":"example-provider"}]}
```

`export` removes recognized API key fields by default. Endpoint paths and model labels can contain private text supplied by the user, so treat every bundle as private. For a user-approved credential transfer an integration may set `"includeCredentials":true` and route standard output directly to its private transfer channel. Keep bundles out of shared or synchronized folders and logs. Account sessions, usage scripts, hooks, executable helpers and proxy placeholders are excluded.

Version 1 uses the envelope `{"version":1,"source":"cc-switch","profiles":[...]}`. Each entry includes a namespaced ID, display name, mode (`direct`, `routing` or `aggregation`), and `config`. The config holds `app` and the native `provider` shape, with the original ID and portable settings under `settingsConfig`. When a selected Claude aggregation model matches the native provider's published model list, `config.selectedModel` records its upstream model ID without the `[1m]` suffix and a separate `oneM` flag.

Import saves prepared providers in one database transaction. Existing IDs and names within the same app are skipped. Entries marked `credential-required`, `account-authorization-required` or `invalid-provider-settings` are reported and skipped. Import leaves client settings, the proxy and active providers alone. Select the imported provider and its routing mode in CC Switch.

The packaged Windows GUI executable uses the GUI subsystem. Standard input and output through inherited pipes still need packaged Windows verification.
