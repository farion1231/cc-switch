# Orchestration M2 Follow-Up TODOs

Findings from the pre-landing `/review` of the 13-task orchestration release
core (commits `8f009cbf..e8119326`), deferred from M1 per user decision.
The five CRITICAL findings (C1–C5) were addressed in two commits:

- `455ee23a` — C1 (SSRF guard), C2 (judge rubric strip), C3 (exact-format
  short-circuit), C5 (expensive-strategy gating)
- `bb0f0542` — C4 (production wiring: ProviderModelResolver, TraceLedger,
  band-based QualityDecision)

Everything below is HIGH or INFO severity and slated for M2.

## HIGH — fix before M2 ships

### H1 — Debate revision phase discards revised candidates when judge fails

`src/orchestration/executor.rs:286-313`

When `revision=true`, each debater revises its answer based on the critique.
The revised responses are stored in `revised_responses`, but if the judge call
or the score extraction fails, the function returns the error and the revised
work is lost. Worse, the unrevised `responses` vector is never compared
against the revised one, so callers cannot tell whether revision actually
improved anything.

**Fix direction:** when the judge fails, fall back to the best revised
candidate by structural / pattern quality (via `QualityGate`) instead of
bubbling the error. Track `revision_improved: bool` in `ExecutionResult`.

### H2 — MoA quality_threshold wired to the generic verifier, not the ranker

`src/orchestration/executor.rs:500-509`

MoA's `quality_threshold` is applied to the **post-aggregation** structural +
pattern check (`QualityGate::verify`), not to the **per-proposal ranker
scores** parsed from `SCORES_JSON`. The 0.50 cutoff at line 474 is hardcoded
and ignores the user-configured threshold.

**Fix direction:** thread `quality_threshold` into `parse_moa_scores` /
the filter step so users can tune strictness; document that the post-aggregate
verifier is a *secondary* check, not the primary gate.

### H3 — Cascade/Debate/MoA budget and fallback config fields are dead

`src/orchestration/config.rs:232-300` (StrategyAction variants)

`max_rounds`, `critique`, `revision` flow through to execution, but
`max_latency_ms`, `max_total_tokens`, `fallback_strategy` (if/when added)
are read nowhere in the executor. Configs validate them as parseable but
silently ignore them at runtime.

**Fix direction:** either wire them (preferred) or remove them and document
the M1 surface explicitly. Adding fields users can configure but that do
nothing is a worse footgun than not having them.

### H4 — Mutex poison recovery is silent

`src/orchestration/engine.rs` (multiple sites)

Every `Mutex::lock().unwrap_or_else(|e| e.into_inner())` recovers from poison
without logging. Poisoning means another thread panicked while holding the
lock — usually indicating corrupt state. Recovering silently hides this.

**Fix direction:** log at `warn!` with the lock name when recovering from
poison so operators can correlate with downstream symptoms.

### H5 — `join_all` on debater / proposer futures is unbounded

`src/orchestration/executor.rs:238, 302, 437`

Every parallel debate / MoA call dispatches all model calls concurrently
via `join_all`. With N debaters across M concurrent requests, upstream
providers see N*M in-flight requests with no admission control. A 10-debate
batch could trip rate limits or DoS a small provider.

**Fix direction:** cap concurrency via `futures::stream::iter(...).buffer_unordered(k)`
where `k` is read from config; default to e.g. 4.

### H6 — HistoryStore is not Send+Sync but TraceLedger must be shared

`src/orchestration/history.rs:73`, `src/orchestration/trace_ledger.rs:10`

`HistoryStore` wraps `rusqlite::Connection` (which contains `RefCell<_>`,
not `Sync`). The M1 C4 wiring wrapped `TraceLedger` in `Arc<Mutex<_>>`
inside `StrategyExecutor` so the engine remains shareable — but every
trace write now serializes through that one mutex.

**Fix direction:** open a fresh `rusqlite::Connection` per write (cheap
with WAL mode), or move the ledger onto a sender-style queue with a
dedicated writer task. Measure contention under load before deciding.

## INFO — opportunistic improvements

### Security

- **I1** — `model_caller.rs:222-243`: `build_target_url` accepts arbitrary
  `base_url` strings from `ModelCallTarget`. The C1 SSRF guard catches the
  provider-resolved path, but if any future call site constructs a
  `ModelCallTarget` from user-controlled input directly, the guard is
  bypassed. Add a `ModelCallTarget::new(url)` constructor that validates.
- **I2** — `provider_resolver.rs:155-186`: capability inference via
  substring match (`"gpt"` in `gpt-handler-service` ⇒ `Json` capability)
  produces false positives. Replace with a structured `capabilities` field
  on `Provider` when that field is added.
- **I3** — `executor.rs:607-617`: `extract_score_from_judge` accepts the
  first `SCORE:` line; if a model emits `SCORE: 0.9 (revised)` it parses
  to 0.9 silently. Tighten the parser to reject trailing tokens.

### Testing

- **I4** — `engine.rs` integration tests use `create_engine_with_yaml`
  which doesn't attach an executor. Most `decide()` tests therefore never
  touch `execute()`. Add at least one test that wires a mock executor and
  runs end-to-end through `decide_and_execute`.
- **I5** — No tests cover the new `with_provider_map_and_ledger` /
  `call_model_resolved` path end-to-end. The unit tests cover the SSRF
  guard and the bands; an integration test that exercises resolve → call
  → trace-record with a mock HTTP server would catch regressions in the
  wiring.
- **I6** — `executor.rs` debate / MoA paths have no failure-mode tests
  (judge timeout, debater 5xx, malformed SCORES_JSON). All current tests
  hit the happy path.

### Maintainability

- **I7** — `executor.rs` is approaching 800 lines. The MoA helpers
  (`build_moa_*_prompt`, `parse_moa_scores`) and the debate helpers
  (`build_debate_*_prompt`, `extract_*_from_judge`) belong in their own
  `strategy/moa.rs` and `strategy/debate.rs` modules.
- **I8** — The `ExecutionResult` struct has 9 fields and is constructed
  inline at 5 sites with copy-pasted `strategy: "..."` / `cascade_attempts`
  / `total_*_tokens` plumbing. A builder or `From<ModelResponse>` impl
  would shrink each site to one line.
- **I9** — `quality_gate.rs:490-548`: the `AntiPattern` list is a magic
  data structure. Consider loading from a config file so users can add
  project-specific patterns (e.g. banned APIs) without code changes.

### Performance

- **I10** — `quality_gate.rs:550-573`: `run_pattern_match` recompiles every
  regex on every call. The patterns are static — memoize them with
  `std::sync::OnceLock<Vec<(Regex, f64)>>`.
- **I11** — `executor.rs:228-238`: debater `messages.clone()` happens N
  times for N debaters. Wrap in `Arc<Vec<Value>>` and pass references.
- **I12** — `selector.rs:53`: `config.strategies.keys().collect()` then
  sort allocates a Vec on every `select_detailed` call. Cache the sorted
  key list inside `OrchestrationConfig` at load time.

### Documentation

- **I13** — `docs/superpowers/specs/2026-06-13-orchestration-debate-moa-release-core-design.md`
  describes the intended C4 wiring as "production-ready on day one". The
  actual M1 wiring is "reachable from production with fallback semantics"
  — narrower. Update the spec or add a follow-up note.
- **I14** — Add an ADR documenting why `trace_ledger` is `Arc<Mutex<_>>`
  and not, e.g., a sender-style queue (H6).

## Process notes (not actionable code)

- The pre-landing review caught 5 CRITICAL findings — 3 from Codex
  adversarial pass (C2, C5, partial C1) and 2 from multi-specialist
  confirmation (C3, C4). Worth keeping the Codex step in the review
  pipeline for M2.
- The release-core plan (`docs/superpowers/plans/2026-06-13-orchestration-debate-moa-release-core.md`)
  checked off all 13 tasks cleanly. The review findings came almost
  exclusively from the *integration* of those tasks, not the tasks
  themselves. Suggests the plan should add a 14th "wire everything
  together" task explicitly next time.
