# VS Code Copilot Custom Endpoint BYOK

This integration manages Custom Endpoint models for VS Code Copilot Chat.
It does not replace GitHub sign-in, inline completions, or the Copilot CLI.

1. Select **VS Code Copilot** in the sidebar.
2. Open **Configuration targets** and select the VS Code Stable/Insiders profiles to manage.
   A custom target must be an absolute path ending in `chatLanguageModels.json`.
3. Add a provider with a base URL, API key, API protocol, and at least one model.
4. Saving applies enabled models to the selected profiles. **Apply to selected profiles**
   can reapply the catalog after restoring or syncing the database.

The catalog separates added providers from available ones, like OpenCode. Add/remove
controls enable or disable a provider's models without deleting its saved credentials.
Without a selected profile, providers stay local; the notice opens Configuration targets.
Provider items follow the combined implementation's compact layout and shared
icon/action components. Existing display metadata is retained: notes take precedence
over the website URL and endpoint, and saved built-in/custom PNG icons are displayed
with safe fallbacks. The header search button or Ctrl+F / Cmd+F filters provider
names, notes, website URLs, and base URLs. This does not add an icon-upload workflow.

Provider/model catalogs use the existing SQLite provider database, so normal export/import
and database synchronization include them. Profile selections and custom paths are device-local,
stored in `~/.cc-switch/vscode-copilot-byok.json`; no schema migration is required.

CC Switch retains providers it does not manage. Before the first change to an existing file,
it creates `chatLanguageModels.json.cc-switch.bak`. All selected files are validated before
writing; failed writes roll back model files, backups and device state. Credential-bearing
files and backups use private writes. Restoring the backup stops management for that target.

MCP, Instructions/Skills, session browsing, usage statistics and advanced import/editing
are intentionally outside this first integration. They can be added independently afterward.
Older combined-preview device state is kept untouched. Do not run an intermediate build
against a newer preview database; use isolated test data when validating the split branches.
