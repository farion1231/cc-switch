# AtomCode Integration

## Supported AtomCode Version

This implementation targets the AtomCode **new-schema** configuration format
(`[provider_accounts.*]` + `[models.*]`, design §3.3 / §14.1). Verified against
the AtomCode source tree at workspace version **v5.2.7**, commit `0c0512143`
(2026-11-13):

- Source: https://atomgit.com/atomgit_atomcode/atomcode (GitHub mirror:
  https://github.com/atomgit-atomcode/atomcode)
- Official Configuration Reference: https://atomcode.atomgit.com/docs/zh/configuration.html
  (en: `/docs/en/configuration.html`)

Key sources behind the selection semantics (see *Selection Semantics* below;
design section numbers are cited as in AtomCode's own source comments):

- `crates/atomcode-config/src/config/mod.rs:633-637` — `default_provider` is the
  **legacy** field (a `[providers.*]` key), superseded by `default_model`.
- `crates/atomcode-config/src/config/mod.rs:661-665` — `default_model` is the
  canonical selection (`[models.<id>]` selection id).
- `crates/atomcode-config/src/config/mod.rs:1448-1457` — `effective_model_selection()`
  resolves `default_model` first, then falls back to `default_provider`.
- `crates/atomcode-cli/src/main.rs:3215-3219` — on a programmatic selection the
  CLI writes **both** fields to the same selection id ("`default_model` is
  canonical; sync the legacy field too").
- `crates/atomcode-coding/src/config.rs:534-540` — the authoritative field
  reference: `default_model` names a `[models.<id>]` entry whose `account` is a
  `[provider_accounts.<id>]` entry; the legacy form is `default_provider` naming
  a `[providers.<name>]` entry, and `default_model` wins.
- `ATOMCODE_HOME`: `crates/atomcode-config/src/distribution.rs`
  (`HOME_ENV = "ATOMCODE_HOME"`, `HOME_DIR_NAME = ".atomcode"`).

## Selection Semantics

`default_provider` and `default_model` hold the **same model key** (selection
id) after a switch. This intentionally mirrors AtomCode's own CLI behavior
(`main.rs:3215-3219`): `default_model` is the canonical field, and the legacy
`default_provider` field is kept in sync for compatibility. `default_provider`
is **not** a `provider_accounts` id — the account is reached through
`models[<selection id>].account`. AtomCode recommends the selection id format
`<account>/<model>` (`config/mod.rs:657-660`); CC Switch writes the model key
as entered in its form, which AtomCode accepts as-is.

## Configuration Format

AtomCode uses a shared-provider model where all configured accounts coexist in `~/.atomcode/config.toml`:

```toml
default_provider = "model-key"
default_model = "model-key"

[provider_accounts.account-id]
provider = "openai"  # or "claude", "deepseek", "gemini", "ollama"
base_url = "https://api.example.com/v1"
api_key = "sk-..."  # Optional: when empty, uses AtomCode's native OAuth (auth.toml)

[models."model-key"]
account = "account-id"
model = "model-id"
context_window = 262144  # Optional
```

### Supported Provider Types

`provider_accounts.<id>.provider` accepts AtomCode's built-in preset ids from
`crates/atomcode-config/src/config/provider_preset.rs` (`PRESETS`). The
canonical set lives in `src-tauri/src/atomcode_config.rs` (`PROVIDER_IDS`) and
is the source of truth for this integration; unknown ids fall back to
`openai-compatible` inside AtomCode. Older wire names written by earlier
versions of this integration (`claude` / `openai_responses` / `gemini`) are
normalized on write.

### Environment Variables

- `ATOMCODE_HOME` - Custom data directory (default: `~/.atomcode`)

## CC Switch Implementation

### Architecture

- **Mode**: Coexist (like Kimi/Dsh/Mcode) - all accounts present in live config simultaneously
- **Live Config**: `~/.atomcode/config.toml`
- **Operations**:
  - **Add**: Write `provider_accounts.<id>` and `models.*` nodes, set `liveConfigManaged: true`
  - **Switch**: Update `default_provider` and `default_model` to point to the account's model
  - **Remove**: Delete the account's nodes from config.toml
  - **Edit**: Update in live config first, then commit to database

### Edge Cases Handled

1. **Default Model Protection**: Cannot delete or rename the currently selected default model
2. **Account Removal**: Cannot remove an account if its model is currently selected as default
3. **Key Collision**: Model keys must not collide with models owned by other accounts
4. **Atomic Writes**: All config.toml writes use temp file + rename for atomicity

### Whitelist Integration

AtomCode is added to the following whitelists:

- **ProviderList.tsx**: `isInConfig` - checks `meta.liveConfigManaged` flag
- **App.tsx**: 
  - Switch handler - calls `handleAddToLiveConfig` (not `switchProvider`)
  - Remove handler - `removeFromConfig` available
  - SetAsDefault handler - calls `switchProvider` (changes default_provider/default_model)
  - Duplicate - sets `addToLive: false` (don't auto-add copy to config.toml)

### Database Schema

```sql
CREATE TABLE providers (
  id TEXT PRIMARY KEY,
  app TEXT NOT NULL,
  name TEXT NOT NULL,
  settings_config JSON NOT NULL,
  meta JSON,  -- { liveConfigManaged: boolean }
  ...
);
```

## Known Limitations

1. **Import from Legacy Config**: Users who configured AtomCode with only the
   legacy format (top-level `default_provider` naming a `[providers.*]` table,
   as in the official configuration reference) now get a **read-only
   projection** on import (`atomcode_config.rs::project_legacy_provider`):
   each `[providers.<name>]` entry (`type` = openai / claude / ollama, `model`
   required) is projected as an account + one model keyed by the provider
   name, so legacy `default_provider` resolves through the same rule. New
   schema entries win: ids already present in `[provider_accounts.*]` are
   never overwritten, and nothing is written back to config.toml (AtomCode
   itself migrates legacy entries in place, `config/mod.rs:1459`).

2. **Version Compatibility**: Verified against AtomCode source at v5.2.7
   (`0c0512143`, 2026-11-13). Future versions may introduce breaking changes to
   the configuration format.

3. **OAuth Accounts**: AtomCode's native OAuth (stored in `auth.toml`) is not directly managed by CC Switch - only the API key-based accounts in `provider_accounts`.

## Testing

Run backend tests with isolated home directory to avoid polluting real config:

```bash
cd src-tauri
CC_SWITCH_TEST_HOME="$(mktemp -d)" cargo test atomcode
```

Frontend unit tests:

```bash
pnpm test:unit atomcode
```

## Future Improvements

1. ~~Document the exact AtomCode version/commit this integration targets~~ — done: v5.2.7 (`0c0512143`), see *Supported AtomCode Version*.
2. ~~Add support for official `[providers.*]` format import~~ — done as a
   read-only projection on import (see Known Limitations 1); writes stay
   new-schema-only and AtomCode's own in-place migration remains the upgrade
   path.
3. Improve OAuth account detection and display
4. Add config validation against AtomCode's schema
