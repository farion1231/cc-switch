# OmniAgent Workbench — Implementation Tasks

## Phase 1: Core Gateway (Week 1-5)

### T1: Project Skeleton + Tauri Setup
- Fork CC-Switch repo or create new Tauri 2.x project
- Set up Cargo workspace with `orchestration` crate
- Configure SQLite migrations
- Verify: `cargo build` succeeds, Tauri window opens

### T2: Dual Protocol Adapters (P0)
- Implement `anthropic_adapter.rs`: `/v1/messages` with SSE streaming
- Implement `openai_adapter.rs`: `/v1/chat/completions` with SSE streaming
- Handle: streaming, tool_use, image blocks, extended_thinking
- **10 unit tests** (Protocol Adapter test matrix)
- Verify: Claude Code can connect via ANTHROPIC_BASE_URL

### T3: Provider Service + Cost Ledger
- Provider management: API keys, base URLs, health checks
- Cost tracking: per-request token/cost recording
- Budget enforcement: daily limit, per-task limit
- **10 unit tests** (Provider + Cost)

### T4: Lifecycle + Audit (from MiroFish)
- `lifecycle.rs`: WorkflowStatus enum + SQLite persistence + transition validation
- `jsonl_logger.rs`: Async JSONL append with structured entries
- **8 unit tests** (Lifecycle + Logger)

**Phase 1 Exit Criteria:**
- [ ] Claude Code connects and receives responses through gateway
- [ ] OpenCode connects and receives responses through gateway
- [ ] Cost Ledger records all calls
- [ ] JSONL audit trail written for every request
- [ ] All 28 unit tests pass

---

## Phase 2: Smart Routing + Cascade (Week 6-9)

### T5: Task Classifier + Strategy Selector
- `task_classifier.rs`: Rule-based difficulty/risk classification
- `strategy/selector.rs`: Strategy selection based on task profile
- **12 unit tests**

### T6: ROUTE Strategy
- `strategy/route.rs`: Direct routing to cheapest suitable model
- Model matching based on task type and modality
- **4 unit tests**

### T7: CASCADE Strategy
- `strategy/cascade.rs`: Tiered escalation with quality gates
- First-token streaming passthrough (D1 fix)
- Binary quality check (D2 fix)
- **5 unit tests**

### T8: ReACT Executor (from MiroFish)
- `strategy/react_executor.rs`: Generic ReACT loop with Tool trait
- Min tool call enforcement, max iterations, tool diversity tracking
- Conflict resolution (tool call + Final Answer)
- Forced termination
- **7 unit tests**

### T9: Quality Estimator + Escalation Controller
- `quality/estimator.rs`: Binary pass/fail (Phase 1)
- `quality/escalation.rs`: Strategy/model upgrade logic
- **5 unit tests**

**Phase 2 Exit Criteria:**
- [ ] ROUTE strategy handles 60-70% of requests
- [ ] CASCADE strategy handles escalation correctly
- [ ] ReACT executor completes full loop with tool calls
- [ ] Quality gates prevent low-quality responses
- [ ] All 33 unit tests pass

---

## Phase 3: Cross-Modal Orchestration (Week 10-14)

### T10: Modality Router + Decomposer
- `modality/router.rs`: Detect required modalities from request
- `modality/decomposer.rs`: Rule-based split decisions (D3 fix)
- **7 unit tests**

### T11: Cross-Modal Pipeline
- `strategy/cross_modal.rs`: Modality split → independent execution → fusion
- Conflict resolution with tool_confidence levels (D7 fix)
- **5 unit tests**

### T12: Knowledge Graph Generator (from MiroFish)
- `context/knowledge_graph.rs`: LLM-driven ontology schema generation
- PascalCase enforcement, fallback types, deduplication, max limits
- **5 unit tests**

### T13: Vision + Speech Integration
- Connect vision models (Qwen-VL, local)
- Connect Whisper for STT
- Structured visual description protocol
- **Integration tests**

### T14: Section Writer + Incremental Output (from MiroFish)
- `workflow/section_writer.rs`: Incremental file output + progress tracking
- **3 unit tests**

**Phase 3 Exit Criteria:**
- [ ] Cross-modal tasks split correctly
- [ ] Vision + text fusion produces consistent results
- [ ] Knowledge graph schemas generated from documents
- [ ] Incremental output streams to frontend
- [ ] All 20+ unit tests pass

---

## Phase 4-6: Advanced Strategies + Eval + Release

Planned after Phase 1-3 MVP validation.

### Phase 4 Scope
- MoA strategy (multi-proposer + aggregator)
- DEBATE strategy (multi-model + judge)
- YAML DAG workflow engine
- DAG visualization (React Flow)

### Phase 5 Scope
- Evaluation system with golden task sets
- A/B testing framework
- Routing threshold auto-calibration
- Continuous quality scoring

### Phase 6 Scope
- System tray + installer packages
- Documentation
- Security audit

---

## Regression Test Checklist (Run Before Every Merge)

```bash
# R1: Protocol adapter → Claude Code tool_use
cargo test protocol_adapter_tool_use_roundtrip

# R2: Quality estimator → escalation rate
cargo test quality_estimator_no_escalation_spike

# R3: Provider failover → no double billing
cargo test provider_failover_no_double_charge

# R4: Budget enforcement → no silent overage
cargo test budget_enforcement_race_condition
```
