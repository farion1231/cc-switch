# Third-Party Notices — Codex Workbench Integration

This document attributes third-party code and concepts integrated into the
CC Switch **Codex Workbench** feature set. No raw API keys, prompt bodies,
encrypted reasoning payloads, or end-user conversation content are redistributed.

## Scope

Components under:

- `src-tauri/src/services/codex_*`
- `src-tauri/src/services/codex_runtime/`
- `src-tauri/src/services/codex_injection/`
- `src-tauri/src/services/codex_reasoning/`
- `src-tauri/resources/codex-workbench/` (if present)
- `src/components/codex-workbench/`

## Attribution

### CodexElves (primary ported source)

- **Repository**: [junxin367/CodexElves](https://github.com/junxin367/CodexElves)
- **License**: MIT
- **Pinned commit**: `bf1224e`
- **What was adapted** (algorithms / behavior, reimplemented natively in CC Switch):
  - Codex App launch / process attach patterns (CDP workbench shell)
  - User-script market install & hash verification flows
  - Plugin marketplace cache initialize/repair around `CODEX_HOME`
  - Degradation radar fetch + 30-minute TTL cache
  - System-prompt rewrite (fingerprint-only logging; no prompt text storage)
  - GPT reasoning continuation grid (`continue_thinking` behavioral reference):
    518-grid eligibility, multi-round continue, single-provider pin, single main usage row
  - Session JSONL enrichment for reasoning / turn token fields on unique match
- **What was *not* copied**: proprietary UI assets, raw conversation bodies,
  encrypted reasoning payloads, or closed-source binaries.
- Substantially ported modules live under `src-tauri/src/services/codex_*`,
  `src-tauri/src/services/codex_reasoning/`, `src/components/codex-workbench/`,
  and related settings/usage surfaces. File headers on those modules note the
  CodexElves behavioral lineage where applicable.

### Codex App / OpenAI Codex (ideas & protocol compatibility)

- **What was used**: Public protocol shapes for Responses API token usage fields,
  session JSONL event names (`turn_context`, `token_count` / `last_token_usage`),
  and CDP attach patterns for a desktop Chromium shell.
- **What was *not* copied**: Proprietary application UI assets, private model
  weights, encrypted reasoning bodies, or closed-source binaries.
- **License posture**: Integration is interoperability-oriented. Users must
  comply with their own OpenAI / Codex App terms when launching the external app.

### User-script market (optional remote manifests)

- Scripts are installed only on **explicit user action**.
- Hash verification rejects tampered payloads; failed updates retain the previous
  version.
- Remote HTML is never injected into the React tree for radar; only structured
  DTOs are shown.

### Plugin marketplace cache

- Operates against a temporary or user-configured `CODEX_HOME`.
- Unrelated TOML / config outside managed cache paths is retained on
  initialize/repair.

### Degradation radar data

- Fetched into a **30-minute TTL** on-disk cache.
- UI distinguishes fresh / cached / stale states; offline stale snapshots remain
  readable without auto-exfiltration.

## Privacy & safety invariants

1. Prompt replacement logs store **fingerprint / boolean flags only**, never
   prompt text.
2. Reasoning columns show token counts and continuation metadata only — **no
   reasoning body**, no `encrypted_content`.
3. Session enrichment fills NULL reasoning/turn fields on unique matches; it does
   not duplicate cost rows already attributed to proxy traffic.
4. Automated tests use temporary directories/databases only — never the
   developer’s real Codex profile paths.

## Contact

For license questions about redistributed snippets, open an issue on the CC Switch
project repository.
