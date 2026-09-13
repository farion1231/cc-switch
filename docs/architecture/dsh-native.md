# DSH Native Architecture

## Purpose

This project makes DeepSeek Harness a first-class CC Switch application while retaining DSH as the source of truth for providers, credentials, and the current model.

## Product Reference

The DSH experience follows CC Switch's Codex design:

- explicit application identity on the provider page;
- structured provider fields rather than a raw JSON-only form;
- API key, endpoint, protocol, default model, model discovery, and model catalog;
- provider cards with endpoint, model count, current provider, and current model;
- local environment detection and a clear application version.

DSH does not inherit Codex OAuth, proxy takeover, failover routing, or protocol transformation because the DSH runtime owns those concerns differently.

## Data Flow

```text
~/.dsh/settings.yaml
  llm-deepseek ---------------------> deepseek-official card
  llm-pi-ai.providers.<route> ------> custom DSH cards
  agent-default-model --------------> current provider/model badges

~/.dsh/.credentials.yaml
  version: 1
  refs.<NAME> ----------------------> masked API key form value
  records.* ------------------------> preserved, never exposed

Native adapter -> DSH provider service -> providers table -> React query -> cards/forms

~/.dsh/sessions/<encoded-cwd>/<session-id>/session.jsonl.zstd
  session header + session/title -> session list metadata
  user/message + assistant/message -> session transcript
```

Session history is browse-and-delete only. DSH session events carry no token usage fields, so usage statistics are intentionally not implemented.

The DSH home directory resolves in this order: CC Switch directory override (`dshConfigDir` in Settings) > `$DSH_HOME` > `~/.dsh`. Under `cfg(test)` the settings override is skipped so `DSH_HOME`-based test isolation always wins.

## Commands

- `import_deepseek_harness_providers_from_live` — native-to-DB sync (also runs at startup).
- `get_dsh_current_state` — native provider ids plus the current provider/model for card badges.
- `set_dsh_current_model` — writes `agent-default-model` after validating provider/model membership; because DSH couples the two, setting a model on a non-current provider also makes that provider current.

MCP and Skills entries are hidden for DSH because DSH 2.0.5 exposes neither an MCP configuration namespace nor a skills directory.

## Ownership Boundaries

### Native adapter

`src-tauri/src/deepseek_harness_config.rs` parses and atomically updates DSH-owned documents. It resolves `$DSH_HOME`, supports official and pi-ai routes, reads the current model, and writes credentials under the version-1 `refs` map while preserving `records`.

### Provider service

`src-tauri/src/services/provider/deepseek_harness.rs` mirrors native routes into the existing provider database. Native files remain authoritative. The service records route ownership as `meta.providerType` (`dsh_deepseek` or `dsh_pi_ai`) so add, edit, delete, and switch operations write back to the correct namespace.

### Frontend

The shared provider page displays DSH cards and current state. The DSH add/edit experience uses the same shared provider form shell as OpenCode: `useDeepSeekHarnessFormState` keeps the DSH structured state in sync with the `settingsConfig` JSON, and `DeepSeekHarnessFormFields` renders the credential reference, base URL, API format, model catalog, and default model. The preset catalog is derived from the OpenCode preset list so both apps stay aligned.

### Environment check

On macOS, DSH is detected from `/Applications/DSH Desktop.app/Contents/Info.plist`. The environment card is informational and intentionally has no install/update action until DSH publishes a stable lifecycle contract for CC Switch to invoke.

## Release Build

macOS 27 with Homebrew Rust 1.97 can corrupt proc-macro dylibs when the upstream release profile strips symbols. `scripts/build-dsh-macos.sh` disables release stripping, uses one Cargo job, and disables pipelining for this local build. It creates an ad-hoc signed `.app`, then packages and verifies a DMG. This is a local development signature, not an official Apple Developer ID release.
