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
```

## Ownership Boundaries

### Native adapter

`src-tauri/src/deepseek_harness_config.rs` parses and atomically updates DSH-owned documents. It resolves `$DSH_HOME`, supports official and pi-ai routes, reads the current model, and writes credentials under the version-1 `refs` map while preserving `records`.

### Provider service

`src-tauri/src/services/provider/deepseek_harness.rs` mirrors native routes into the existing provider database. Native files remain authoritative. The service records route ownership as `meta.providerType` (`dsh_deepseek` or `dsh_pi_ai`) so add, edit, delete, and switch operations write back to the correct namespace.

### Frontend

The shared provider page displays DSH cards and current state. `DeepSeekHarnessProviderForm.tsx` provides the Codex-style structured editor and maps its values to the native DSH schema.

### Environment check

On macOS, DSH is detected from `/Applications/DSH Desktop.app/Contents/Info.plist`. The environment card is informational and intentionally has no install/update action until DSH publishes a stable lifecycle contract for CC Switch to invoke.

## Release Build

macOS 27 with Homebrew Rust 1.97 can corrupt proc-macro dylibs when the upstream release profile strips symbols. `scripts/build-dsh-macos.sh` disables release stripping, uses one Cargo job, and disables pipelining for this local build. It creates an ad-hoc signed `.app`, then packages and verifies a DMG. This is a local development signature, not an official Apple Developer ID release.
