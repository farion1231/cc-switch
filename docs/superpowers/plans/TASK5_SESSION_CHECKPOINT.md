# Task5 Session Checkpoint — in progress

## Goal
扩展请求日志契约并添加“推理”列空态（metadata only, no body）

## Base
- Task4 commit: `de241ec feat(sync): preserve local provider credentials on cloud restore`
- Plan: Task 5 in `2026-07-15-codex-workbench-integration.md`

## Steps
1. Write table rendering tests (unknown/zero/value/continuation) — IN PROGRESS
2. Run test → expect FAIL (no 推理 column)
3. Thread nullable reasoning through logger + SQL mapping
4. Add table + detail rendering via `formatReasoning`
5. cargo + pnpm checks
6. Commit: `feat(usage): add reasoning token log contract`

## Semantics
- `reasoningTokens: undefined/null` → "—" (unknown / SQL NULL)
- `0` → "Tok 0" (known zero)
- `N` → "Tok N"
- continuationRounds > 0 → "Tok N ✨R"
- partial_failed → "Tok N ⚠"

## Files
- frontend: usage.ts, format.ts, RequestLogTable, RequestDetailPanel, tests
- backend: logger.rs, usage_stats.rs, parser.rs (CodexReasoningUsage), mod.rs, SQL columns
