# OmniAgent Workbench — Test Plan

---

## Framework Selection

| Component | Framework | Reason |
|-----------|-----------|--------|
| Rust core engine | `cargo test` + `rstest` | Parameterized test cases |
| Integration tests | `cargo test` (tests/ directory) | Cross-module integration |
| API E2E | `reqwest` + Rust test binary | HTTP tests against daemon |
| SSE Streaming | `tokio-test` | Async stream verification |
| Frontend | Vitest + React Testing Library | Component tests |

## Test Architecture

```
omniagent-workbench/
├── src-tauri/
│   ├── src/
│   │   ├── gateway/
│   │   │   └── *_test.rs
│   │   ├── orchestration/
│   │   │   ├── strategy/
│   │   │   │   └── *_test.rs
│   │   │   ├── context/
│   │   │   │   └── *_test.rs
│   │   │   ├── workflow/
│   │   │   │   └── *_test.rs
│   │   │   ├── quality/
│   │   │   │   └── *_test.rs
│   │   │   ├── audit/
│   │   │   │   └── *_test.rs
│   │   │   └── cost/
│   │   │       └── *_test.rs
│   │   ├── provider_service_test.rs
│   │   └── session_manager_test.rs
│   └── tests/
│       ├── integration/
│       │   ├── gateway_e2e.rs
│       │   ├── cascade_flow.rs
│       │   ├── cross_modal_flow.rs
│       │   └── protocol_roundtrip.rs
│       └── fixtures/
└── evals/
```

## Module Test Matrix

### P0 — Must Pass Before Any Merge

| Module | Cases | Key Coverage |
|--------|-------|-------------|
| Protocol Adapter | 10 | Anthropic/OpenAI roundtrip, tool_use, SSE streaming, extended_thinking, image block, error responses |
| Quality Estimator | 5 | Tool pass/fail, schema valid/invalid, no-tool unverifiable, candidate order randomization |
| ReactExecutor | 7 | Min tool calls enforcement, max iterations, tool diversity, conflict resolution, forced termination, empty response, correct Final Answer extraction |
| Lifecycle State Machine | 5 | Valid transitions, invalid transitions rejected, error capture, timestamp updates, SQLite roundtrip |

### P1 — Must Pass Before Phase Complete

| Module | Cases | Key Coverage |
|--------|-------|-------------|
| Modality Router | 7 | Text/image/audio/mixed input, unknown MIME fallback, tightly coupled context detection |
| Difficulty Classifier | 5 | Simple/complex/ultra-long context/no-tool scenarios |
| Strategy Selector | 7 | 5 strategy trigger conditions, boundary values, budget exhaustion fallback |
| Workflow Executor (DAG) | 7 | Linear/parallel/dynamic insert/timeout/error/loop/cancel |
| Cross-Modal Aggregator | 5 | Consistent/contradictory/timeout/all-failed/weighted merge |
| Provider Manager | 5 | Health check/failover/circuit breaker/recovery/all-down |
| KnowledgeGraphGenerator | 5 | Valid schema generation, PascalCase enforcement, fallback insertion, dedup, max limits |
| JsonlLogger | 3 | Append entries, structured format, concurrent safety |
| SectionWriter | 3 | Save section, progress update, full assembly |

### P2 — Before Release

| Module | Cases | Key Coverage |
|--------|-------|-------------|
| Cost Ledger | 5 | Record/budget check/overage/reset/concurrent safety |
| MCP Manager | 3 | Start/timeout/crash restart |

**Total: 79 minimum test cases**

## 4 Critical Regression Tests

| # | Test | Why Critical |
|---|------|-------------|
| R1 | Protocol adapter change → Claude Code tool_use still works | tool_use breakage = Claude Code core loop crash |
| R2 | Quality estimator threshold → escalation rate doesn't spike | +0.05 threshold = daily cost doubles |
| R3 | Provider failover → no double billing | Both providers succeed = user pays twice |
| R4 | Budget enforcement → no silent overage | Race condition = daily cost out of control |

## E2E Integration Tests

### Test 1: Claude Code → Gateway → DeepSeek → Response

```
1. Start gateway on localhost:15721
2. Set ANTHROPIC_BASE_URL=http://127.0.0.1:15721/v1
3. Send Anthropic Messages API request with tool_use
4. Verify: ROUTE strategy selected (simple task)
5. Verify: Response in Anthropic format
6. Verify: SSE streaming works
7. Verify: Cost Ledger records the call
```

### Test 2: CASCADE Full Flow

```
1. Send complex coding task
2. Verify: CASCADE strategy selected
3. Verify: First model attempted (cheap)
4. Verify: Quality check fails (tool test fails)
5. Verify: Escalation to mid model
6. Verify: Quality check passes
7. Verify: Response returned with escalation metadata
```

### Test 3: ReACT Loop (DEBATE Strategy)

```
1. Send high-risk fact-checking task
2. Verify: DEBATE strategy selected
3. Verify: 3 models called in parallel
4. Verify: Judge model evaluates
5. Verify: Tool verification runs
6. Verify: JSONL audit trail written
```

### Test 4: Cross-Modal Flow

```
1. Send request with image + text
2. Verify: CROSS-MODAL strategy selected
3. Verify: Vision path extracts structured description
4. Verify: Text path runs reasoning
5. Verify: Aggregator merges results
6. Verify: Consistency check runs
```
