# Codex Workbench Comprehensive Review (2026-07-17)

Base: post-T15 (`e2c823d9` polish; review fix `e45870c9`)

## Scope

- `codex_runtime` (launcher / discovery / cdp / bridge)
- `session_usage_codex` (T15 enrich + dedup)
- Frontend workbench API + command registration
- Gates: clippy clean; unit tests green for runtime + usage

## Fixes applied this review

### P1 — `try_enrich_proxy_log` non-deterministic turn_id match

**Before:** `SELECT ... WHERE turn_id = ? LIMIT 1` could enrich an arbitrary row when multiple codex rows share a turn_id.

**After (`e45870c9`):** collect all matching request_ids; enrich only when exactly one match. Ambiguous → skip (safe no-op; may later insert session-only row via existing path).

**Test:** `enrich_by_turn_id_requires_unique`

### P1 — fingerprint enrich ignored model normalization

**Before:** SQL compared `model` / `request_model` with raw session model string only. Proxy rows often store normalized ids (`gpt-5.4`) while session events carry `openai/GPT-5.4-2026-03-05` → enrich miss → duplicate session-only rows.

**After:** SQL still filters by token fingerprint + time window; model match applied in Rust via `normalize_codex_model` on both columns, with raw equality fallback.

**Test:** `enrich_by_fingerprint_matches_normalized_model`

## Reviewed and accepted (not bugs)

| Item | Verdict |
|------|---------|
| `DEFAULT_CDP_PORT = 9222` | Spawn default only. Attach path prefers cmdline port (Store 9229) then scans `9222..+20`. |
| Attach without CDP → `OrdinaryRunning` | Product policy: never force-kill ordinary Codex. |
| Frontend `codexWorkbench.ts` invokes | All backend cmds registered in `lib.rs`; names match. |
| Dual-bridge / reinject | `attach_and_inject` + nav watcher reinject guarded; live smokes cover inject path (ignored). |
| Discovery sort CDP-main first | Correct for multi-process ChatGPT.exe. |
| `apply_enrich` COALESCE | Only fills NULL reasoning/turn_id; sets `session_enriched=1`. |

## Residual / manual

1. Live E2E: Store Codex with `--remote-debugging-port=9229` → launch enhanced → inject + bridge once.
2. Push `e45870c9` to fork if remote push was blocked in this session; PR #5451 should include it.
3. Optional polish (not blocking): surface enrich-skip metrics; align `launch_with_hooks` test DI more closely with production attach path comments.

## Gates run

- `cargo clippy` (earlier in session): clean for workbench paths
- `cargo test --lib session_usage_codex::tests`: **26 passed**
- Runtime unit tests: green earlier in session

## Commit

```
e45870c9 fix(usage): require unique turn_id enrich and normalize model fingerprint
```
