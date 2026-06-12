# EC Switch — Architecture Overview

---

## System Context

```
┌─────────────────────────────────────────────────────────────────┐
│                     User's Machine                               │
│                                                                  │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐                │
│  │ Claude Code│  │  OpenCode  │  │   Codex    │                │
│  │ (Anthropic)│  │  (OpenAI)  │  │  (OpenAI)  │                │
│  └─────┬──────┘  └─────┬──────┘  └─────┬──────┘                │
│        │               │               │                         │
│        └───────────────┼───────────────┘                         │
│                        │ localhost:15721                          │
│  ┌─────────────────────▼──────────────────────┐                 │
│  │              EC Switch                      │                 │
│  │         (Tauri 2.x Desktop App)             │                 │
│  └─────────────────────┬──────────────────────┘                 │
│                        │                                         │
│        ┌───────────────┼───────────────┐                         │
│        │               │               │                         │
│  ┌─────▼──────┐  ┌─────▼──────┐  ┌────▼─────┐                  │
│  │  Anthropic  │  │   OpenAI   │  │ DeepSeek │                  │
│  │  API Cloud  │  │  API Cloud │  │   Cloud  │                  │
│  └────────────┘  └────────────┘  └──────────┘                  │
│        │               │               │                         │
│  ┌─────▼──────┐  ┌─────▼──────┐  ┌────▼─────┐                  │
│  │  Ollama    │  │  Qwen VL   │  │  Whisper │                  │
│  │  (Local)   │  │  (Vision)  │  │  (Local) │                  │
│  └───────────┘  └────────────┘  └──────────┘                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Data Flow

```
Client Request
     │
     ▼
RequestInterceptor ──→ Parse client type, messages, tools, context
     │
     ▼
ModalityRouter ──→ Determine required modalities
     │
     ▼
TaskClassifier ──→ Estimate complexity, risk, verifiability
     │
     ▼
StrategySelector ──→ Pick strategy (ROUTE/CASCADE/CROSS-MODAL/DEBATE/MoA)
     │
     ▼
StrategyExecutor ──→ Execute strategy (may call multiple models)
     │  │                    │
     │  ▼                    ▼
     │  ModelCall (cheap)   ModelCall (strong)
     │  │                    │
     │  ▼                    ▼
     │  ToolVerification   LLMResponse
     │  │                    │
     ▼  ▼                    ▼
QualityEstimator ──→ Pass/Fail → Escalate or Accept
     │
     ▼
ResponseAdapter ──→ Convert to client's expected format
     │
     ▼
Client Response (SSE streaming)
     │
     ▼
CostLedger + JsonlLogger ──→ Record everything
```

---

## Component Responsibilities

| Component | Responsibility | Phase |
|-----------|---------------|-------|
| `gateway/server.rs` | HTTP server, SSE streaming | 1 |
| `gateway/anthropic_adapter.rs` | Anthropic Messages API format | 1 |
| `gateway/openai_adapter.rs` | OpenAI Chat Completions API format | 1 |
| `provider_service.rs` | Model provider management, health checks | 1 |
| `cost/ledger.rs` | Token counting, cost tracking, budget enforcement | 1 |
| `audit/jsonl_logger.rs` | Structured JSONL audit trail (from MiroFish) | 1 |
| `workflow/lifecycle.rs` | State machine for workflow execution (from MiroFish) | 1 |
| `orchestration/engine.rs` | Request processing pipeline | 2 |
| `context/task_classifier.rs` | Difficulty/risk classification | 2 |
| `strategy/selector.rs` | Strategy selection based on task profile | 2 |
| `strategy/route.rs` | Simple routing strategy | 2 |
| `strategy/cascade.rs` | Tiered escalation strategy | 2 |
| `strategy/react_executor.rs` | Generic ReACT loop (from MiroFish) | 2 |
| `quality/estimator.rs` | Quality scoring | 2 |
| `quality/escalation.rs` | Escalation controller | 2 |
| `modality/router.rs` | Modality detection | 3 |
| `modality/decomposer.rs` | Modality split decisions | 3 |
| `strategy/cross_modal.rs` | Cross-modal orchestration | 3 |
| `context/knowledge_graph.rs` | Ontology schema generation (from MiroFish) | 3 |
| `workflow/dag_executor.rs` | YAML DAG workflow execution | 4 |
| `workflow/section_writer.rs` | Incremental output (from MiroFish) | 4 |
| `strategy/debate.rs` | Multi-model debate | 4 |
| `strategy/moa.rs` | Mixture of agents | 4 |
| `strategy/config_advisor.rs` | LLM config tuning (from MiroFish) | 5 |

---

## Database Schema

```sql
-- Request log
CREATE TABLE requests (
    id TEXT PRIMARY KEY,
    timestamp DATETIME NOT NULL,
    client TEXT NOT NULL,          -- 'claude_code', 'opencode', 'codex'
    strategy TEXT NOT NULL,        -- 'route', 'cascade', 'cross_modal', 'debate', 'moa'
    models_used TEXT NOT NULL,     -- JSON array
    input_tokens INTEGER,
    output_tokens INTEGER,
    cost_usd REAL,
    latency_ms INTEGER,
    quality_score REAL,
    escalated BOOLEAN,
    success BOOLEAN,
    error TEXT
);

-- Cost tracking
CREATE TABLE daily_costs (
    date TEXT PRIMARY KEY,
    total_usd REAL,
    budget_usd REAL,
    request_count INTEGER
);

-- Workflow state
CREATE TABLE workflows (
    workflow_id TEXT PRIMARY KEY,
    status TEXT NOT NULL,           -- WorkflowStatus enum value
    current_step TEXT,
    completed_steps TEXT,           -- JSON array
    error TEXT,
    created_at DATETIME,
    updated_at DATETIME
);

-- Evaluation results
CREATE TABLE evals (
    id TEXT PRIMARY KEY,
    eval_set TEXT NOT NULL,
    task_id TEXT NOT NULL,
    strategy TEXT NOT NULL,
    model TEXT,
    success BOOLEAN,
    cost_usd REAL,
    latency_ms INTEGER,
    timestamp DATETIME
);

-- Routing history (for learning)
CREATE TABLE routing_history (
    id TEXT PRIMARY KEY,
    task_type TEXT,
    complexity REAL,
    risk TEXT,
    selected_strategy TEXT,
    selected_model TEXT,
    was_successful BOOLEAN,
    timestamp DATETIME
);
```
