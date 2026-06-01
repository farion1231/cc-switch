# MiroFish → OmniAgent Pattern Porting Guide

Reference for porting MiroFish's production-validated Python patterns to OmniAgent's Rust/Tauri stack.

---

## Overview

MiroFish is a Python/Flask social simulation prediction engine. OmniAgent Workbench ports 6 of its architectural patterns into native Rust — no Python dependency, no code reference, only pattern reimplementation.

| # | Pattern | MiroFish Source | OmniAgent Target |
|---|---------|-----------------|------------------|
| 1 | 6-State Lifecycle | `simulation_manager.py` | `lifecycle.rs` |
| 2 | ReACT Loop | `report_agent.py` | `react_executor.rs` |
| 3 | Ontology Schema Generation | `ontology_generator.py` | `knowledge_graph.rs` |
| 4 | JSONL Audit Trail | `report_agent.py` (ReportLogger) | `jsonl_logger.rs` |
| 5 | Incremental Section Output | `report_agent.py` (ReportManager) | `section_writer.rs` |
| 6 | LLM Config Advisor | `simulation_config_generator.py` | `config_advisor.rs` |

---

## Pattern 1: 6-State Lifecycle

### MiroFish Implementation

`SimulationStatus` enum with transitions: `CREATED → PREPARING → READY → RUNNING → COMPLETED/FAILED/PAUSED/STOPPED`

State persisted to `state.json` files per simulation directory. In-memory cache + file reload.

Key behaviors:
- Valid transition enforcement (implicit — no transition validation in MiroFish code, but the workflow enforces it procedurally)
- Error state capture (`state.error = str(e)`)
- Timestamp tracking (`created_at`, `updated_at`)
- Simple dict serialization for persistence

### OmniAgent Rust Port

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
pub enum WorkflowStatus {
    Created, Preparing, Ready, Running,
    Paused, Completed, Failed,
}

pub struct WorkflowState {
    pub workflow_id: String,
    pub status: WorkflowStatus,
    pub current_step: Option<String>,
    pub completed_steps: Vec<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

**Key changes:**
- File-based `state.json` → SQLite row via `sqlx`
- Implicit transition enforcement → explicit `transition()` method with validation
- Python dict → typed Rust struct with serde

**MiroFish code reference:** `backend/app/services/simulation_manager.py:25-113`

---

## Pattern 2: ReACT Loop

### MiroFish Implementation

`ReportAgent._generate_section_react()` implements a full ReACT loop:
- `MAX_TOOL_CALLS_PER_SECTION = 5`
- `min_tool_calls = 3` (enforced — rejects Final Answer if under minimum)
- Tool diversity tracking: `used_tools: Set[str]`, prompts user to try unused tools
- Conflict handling: when LLM outputs both tool call and Final Answer simultaneously
- Forced termination: at max iterations, forces Final Answer
- LLM response parsing: XML-style `Action: {...} Observation:` format + fallback bare JSON

### OmniAgent Rust Port

```rust
pub struct ReactExecutor {
    tools: HashMap<String, Box<dyn Tool>>,
    llm_client: LlmClient,
    max_iterations: usize,    // Default: 5
    min_tool_calls: usize,    // Default: 3
    jsonl_logger: JsonlLogger,
}
```

**Key changes:**
- Python dict-based tool dispatch → `dyn Tool` trait objects
- Regex-based response parsing → Rust regex (same patterns, safer types)
- Synchronous loop → async `execute()` with `await` per iteration
- `ReportLogger` → generic `JsonlLogger`

**MiroFish code reference:** `backend/app/services/report_agent.py:1221-1530`

---

## Pattern 3: Ontology Schema Generation

### MiroFish Implementation

`OntologyGenerator` uses an LLM with a detailed system prompt to generate:
- 10 entity types (8 specific + Person + Organization fallback)
- 6-10 edge types (SCREAMING_SNAKE_CASE)
- Attribute definitions per entity
- Post-validation: force PascalCase names, deduplicate, cap at 10/10, ensure fallbacks exist

### OmniAgent Rust Port

```rust
pub struct OntologySchema {
    pub entity_types: Vec<EntityTypeDef>,
    pub edge_types: Vec<EdgeTypeDef>,
    pub analysis_summary: String,
}
```

**Key changes:**
- Pydantic-like validation → serde deserialize + manual validation
- `generate_python_code()` method → not ported (MiroFish-specific codegen)
- `_to_pascal_case()` → Rust implementation (same regex logic)

**MiroFish code reference:** `backend/app/services/ontology_generator.py:1-506`

---

## Pattern 4: JSONL Audit Trail

### MiroFish Implementation

`ReportLogger` writes one JSON object per line to `agent_log.jsonl`:
- Structured fields: `timestamp`, `elapsed_seconds`, `action`, `stage`, `details`
- Typed log methods: `log_start()`, `log_tool_call()`, `log_tool_result()`, `log_llm_response()`, `log_section_complete()`
- File handle kept open, appended per action
- `ReportConsoleLogger` writes parallel `console_log.txt` for human-readable output

### OmniAgent Rust Port

```rust
pub struct JsonlLogger {
    log_path: PathBuf,
    start_time: Instant,
}
```

**Key changes:**
- Python file I/O → `tokio::fs` async append
- Dual logger (JSONL + console) → `tracing` integration for console output, JSONL for structured
- Method-per-action → single `log(action, stage, details)` with typed actions

**MiroFish code reference:** `backend/app/services/report_agent.py:36-387`

---

## Pattern 5: Incremental Section Output

### MiroFish Implementation

`ReportManager` manages per-section file output:
- `reports/{id}/section_01.md`, `section_02.md`, ...
- `progress.json` updated after each section
- `meta.json` for report metadata
- `full_report.md` assembled at end
- Content cleaning: remove duplicate headings, convert ### to bold

### OmniAgent Rust Port

```rust
pub struct SectionWriter {
    output_dir: PathBuf,
}
```

**Key changes:**
- Synchronous `open()/write()` → async `tokio::fs::write()`
- Python `os.path` → `std::path::PathBuf`
- Content cleaning regex → same patterns in Rust `regex` crate

**MiroFish code reference:** `backend/app/services/report_agent.py:1884-2573`

---

## Pattern 6: LLM Config Advisor

### MiroFish Implementation

`SimulationConfigGenerator` takes simulation requirements + document context → LLM generates structured configuration parameters with reasoning.

### OmniAgent Rust Port

Merged into OmniAgent's strategy/config system. The LLM-driven config generation pattern is used for:
- Dynamic routing threshold tuning
- Workflow parameter optimization
- Model selection advisory

**MiroFish code reference:** `backend/app/services/simulation_config_generator.py`

---

## What NOT to Port

| MiroFish Component | Why Not |
|---|---|
| `simulation_ipc.py` | OASIS-specific subprocess communication |
| `simulation_runner.py` | OASIS-specific simulation execution |
| `oasis_profile_generator.py` | Social simulation persona generation |
| `zep_tools.py` | Zep Cloud API client (replaced by local Qdrant) |
| `zep_entity_reader.py` | Zep-specific entity filtering |
| `zep_graph_memory_updater.py` | Zep-specific graph operations |
| `graph_builder.py` | Zep Cloud graph construction |
| `text_processor.py` | MiroFish-specific text processing |
| Flask API routes (`api/`) | OmniAgent uses Rust HTTP (axum) |
| Vue 3 frontend (`frontend/`) | OmniAgent uses React + Tauri |
| Ollama integration in MiroFish | OmniAgent has its own model management |

---

## Porting Checklist

- [ ] `WorkflowStatus` enum with valid transition matrix
- [ ] `WorkflowState` struct with SQLite persistence
- [ ] `Tool` trait definition
- [ ] `ReactExecutor` with min_tool_calls enforcement
- [ ] `ReactExecutor` tool diversity tracking
- [ ] `ReactExecutor` conflict resolution (tool call + Final Answer)
- [ ] `ReactExecutor` forced termination at max iterations
- [ ] `OntologySchema` struct with serde
- [ ] `KnowledgeGraphGenerator` with LLM prompt + validation
- [ ] PascalCase / SCREAMING_SNAKE_CASE enforcement
- [ ] Person + Organization fallback insertion
- [ ] Entity type deduplication
- [ ] `JsonlLogger` async append
- [ ] `SectionWriter` incremental output
- [ ] `SectionWriter` progress.json updates
- [ ] `SectionWriter` full output assembly
