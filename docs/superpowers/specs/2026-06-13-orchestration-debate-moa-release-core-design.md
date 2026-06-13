# Orchestration Debate/MoA Release Core Design

Date: 2026-06-13
Audience: DeepSeek implementation handoff
Status: Approved design direction
Scope: Release-trustworthy Debate/MoA core path with minimal eval, Provider unification, observability, and staged cross-modal architecture

## 1. Executive Summary

The current orchestration implementation proves that Route, Cascade, Debate, and MoA can be wired into the proxy path, but it is not yet strong enough to support claims such as "superhuman multi-model intelligence", "complete cross-modal capability", or "small models surpass frontier models". The next release should position orchestration as a reliable experimental-to-product feature: a deterministic, observable, provider-integrated multi-model gateway that can improve quality/cost trade-offs on selected workloads and can prove those improvements through a small built-in eval harness.

This design chooses a conservative release core:

1. Make strategy selection deterministic and explainable.
2. Route all orchestration calls through the existing Provider configuration and health/fallback system.
3. Upgrade Debate from "parallel answers plus judge summary" to structured multi-round critique, score, threshold, and fallback.
4. Upgrade MoA from "parallel proposers plus aggregator" to ranked, optionally layered fusion with explicit quality gates.
5. Add a trace ledger so failures, fallbacks, cost, latency, model choices, and quality scores are visible.
6. Add a minimal eval harness with fixed test cases for regression checks before release.
7. Define the cross-modal architecture for text/image/audio, but implement it in milestones. The first release must not pretend to have full audio cross-modal intelligence if only image+text is implemented.

The public release wording should be:

> "OneAgentSwitch now includes a provider-integrated multi-model orchestration engine with deterministic routing, Debate/MoA execution, quality gates, traceability, and initial eval coverage."

It should not say:

> "Complete super-intelligent Debate/MoA", "small models universally outperform frontier models", or "full cross-modal intelligence is complete."

## 2. Current Context and Problems

Recent inspection found that base verification currently passes:

- TypeScript typecheck passes.
- Frontend unit/integration tests pass.
- Rust tests pass.
- Renderer production build passes.
- Rust build passes, but emits many unused/dead-code warnings around orchestration.

The risk is not "the project cannot build"; the risk is that the orchestration feature is not yet release-trustworthy.

Observed gaps:

1. Orchestration can be disabled by default and fall through to passthrough.
2. Debate and MoA exist, but are closer to MVP algorithms than robust orchestration methods.
3. Strategy selection can be unstable when rules tie because the current strategy map is not inherently ordered.
4. Debate quality thresholds are not consistently enforced as release gates.
5. Model calling is not fully unified with the existing desktop Provider configuration, health checks, key management, and fallback behavior.
6. The cross-modal story exists mostly in configuration and design documents; the orchestration action model does not yet represent complete image/audio/text pipelines.
7. Observability is insufficient for release claims. It must be possible to answer: why was this strategy chosen, which models ran, what failed, what did it cost, what quality score was assigned, and whether fallback occurred.
8. Tests prove many local units, but not enough end-to-end Debate/MoA behavior through realistic provider mocks.

## 3. Design Goals

### 3.1 Product Goals

The release should make orchestration credible as a product capability:

- Users can enable orchestration and see predictable behavior.
- Advanced routes do not silently fail or degrade without evidence.
- Provider settings remain the single source of truth for model endpoints, credentials, and health.
- The system can explain a decision after the fact.
- Quality improvements can be measured on a small, repeatable local eval set.

### 3.2 Engineering Goals

The implementation should:

- Keep proxy API compatibility.
- Avoid token-level streaming for Debate/MoA/Cascade in the first release; buffer complete results and return a compatible final response.
- Keep module boundaries small enough for isolated unit and integration tests.
- Avoid unrelated refactors.
- Introduce deterministic ordering wherever release behavior depends on selection.
- Preserve passthrough behavior when orchestration is disabled or ineligible.
- Make failure explicit in logs, traces, and response metadata where possible.

### 3.3 Algorithm Goals

The algorithms should borrow proven ideas from the research literature without overclaiming:

- Dynamic routing inspired by RouteLLM: choose cheap/simple paths for easy requests and stronger paths for complex/high-risk tasks.
- Ranking plus fusion inspired by LLM-Blender: judge or rank candidate answers before generation fusion.
- Layered proposer aggregation inspired by Mixture-of-Agents: allow multiple layers for hard tasks, not just one fan-out/fan-in step.
- Multi-round critique inspired by ReConcile and multi-agent debate: let agents critique, revise, and converge rather than only answer once.
- Candidate diversity and consistency inspired by Self-Consistency: compare multiple independent answers for high-risk reasoning.
- Branch-solve-merge decomposition for complex engineering tasks: split subproblems only when the classifier says decomposition is useful.

References:

- Mixture-of-Agents Enhances Large Language Model Capabilities: https://arxiv.org/abs/2406.04692
- RouteLLM: Learning to Route LLMs with Preference Data: https://arxiv.org/abs/2406.18665
- Self-Consistency Improves Chain of Thought Reasoning in Language Models: https://arxiv.org/abs/2203.11171
- ReConcile: Round-Table Conference Improves Reasoning via Consensus among Diverse LLMs: https://arxiv.org/abs/2309.13007
- Improving Factuality and Reasoning in Language Models through Multiagent Debate: https://arxiv.org/abs/2305.14325
- LLM-Blender: Ensembling Large Language Models with Pairwise Ranking and Generative Fusion: https://arxiv.org/abs/2306.02561
- Branch-Solve-Merge Improves Large Language Model Evaluation and Generation: https://arxiv.org/abs/2310.15123

## 4. Non-Goals

The first release does not need to:

- Implement true token-by-token streaming for Debate, MoA, or Cascade.
- Prove universal superiority over a frontier model.
- Implement complete audio understanding, transcription, speech generation, and audio reasoning in the same milestone.
- Replace the existing proxy adapter layer.
- Add a large benchmark platform.
- Build a new Provider configuration system separate from the current product.
- Add a large frontend observability dashboard if a backend trace ledger and simple UI hooks are enough for the release.

## 5. Milestones

### 5.1 Milestone 1: Release-Trustworthy Core

This is the first implementation target.

Deliver:

- Deterministic strategy selection.
- Provider-integrated model resolver.
- Debate threshold enforcement.
- MoA ranking and fusion.
- End-to-end provider mock tests for Route, Cascade, Debate, and MoA.
- Trace ledger for every orchestration request.
- Minimal eval harness with fixed samples.
- Compatible non-streaming final response for advanced strategies.
- Clear fallback behavior and visible failure records.

Do not deliver in this milestone:

- Full audio cross-modal execution.
- Token-level streaming for multi-model strategies.
- Large UI dashboard.

### 5.2 Milestone 2: Image+Text Cross-Modal Path

Deliver:

- Request modality detection for text-only, image-only, and image+text.
- Vision extraction step that turns images into structured descriptions.
- Text reasoning step using the extracted visual description plus original user prompt.
- Aggregator step that produces the final answer.
- Trace records for modality detection, vision extraction, reasoning, and aggregation.
- Mock tests for image+text inputs.

### 5.3 Milestone 3: Audio Interface and Stubbed Execution

Deliver:

- Audio input detection and `ModalityProfile` fields.
- Provider capability metadata for STT, audio reasoning, and TTS.
- Stubbed tests proving audio requests are not misrouted into text-only Debate/MoA.
- Explicit "audio cross-modal execution unavailable" fallback if no provider supports the required capability.

### 5.4 Milestone 4: Full Audio Cross-Modal Execution

Deliver after the first release:

- STT/transcription step.
- Optional audio event extraction.
- Text reasoning over transcript and request context.
- Optional TTS output if the client asks for audio response.
- Eval cases for audio transcription + reasoning.

## 6. Target Architecture

The orchestration path should be decomposed into the following modules.

### 6.1 RequestProfiler

Purpose:

Analyze the incoming proxy request and produce a normalized `TaskProfile`.

Responsibilities:

- Detect task type: chat, coding, reasoning, summarization, planning, extraction, translation, tool-use, unknown.
- Estimate complexity from prompt length, structure, code presence, multi-step language, and requested output constraints.
- Estimate risk: low, medium, high, critical.
- Detect modality: text, image, audio, mixed.
- Detect protocol constraints: streaming requested, tools present, function calling requested, model explicitly pinned.
- Detect whether orchestration is eligible.

Output should include:

- `task_type`
- `complexity_score`
- `risk_level`
- `has_tools`
- `is_streaming`
- `has_text`
- `has_image`
- `has_audio`
- `needs_code`
- `requires_exact_format`
- `client_model`
- `eligible_for_orchestration`
- `ineligibility_reason`

Design rule:

If tools or strict streaming make orchestration unsafe, the profiler must say so explicitly. It should not rely on scattered checks in handlers.

### 6.2 StrategySelector

Purpose:

Choose exactly one strategy in a deterministic and explainable way.

Selection order:

1. Filter strategies by enabled flag.
2. Filter by request eligibility.
3. Filter by modality support.
4. Filter by provider/model availability and health.
5. Score matching conditions.
6. Apply deterministic tie-break:
   - explicit strategy priority, lower number wins or higher number wins as configured, but must be documented;
   - higher match score;
   - lower estimated cost if quality class is equal;
   - lower estimated latency if cost class is equal;
   - stable strategy id lexicographic order as final tie-break.

Output should include:

- selected strategy id;
- selected action type;
- score;
- tie-break fields;
- rejected candidates with reasons, at least in debug trace mode.

Design rule:

Strategy selection must not depend on `HashMap` iteration order.

### 6.3 ProviderModelResolver

Purpose:

Resolve logical model roles into concrete provider/model endpoints using the existing product configuration.

Current risk:

If orchestration uses hardcoded provider URLs or environment variables while the rest of the app uses Provider settings, users will see inconsistent behavior.

Responsibilities:

- Resolve role names such as `cheap_reasoner`, `mid_coder`, `frontier_judge`, `vision_extractor`, `audio_transcriber` into configured provider model entries.
- Read base URL, API key reference, provider type, model id, timeout, health state, and fallback candidates from the existing Provider configuration.
- Verify capability requirements:
  - text generation;
  - vision input;
  - audio transcription;
  - structured JSON output;
  - tool/function support if needed.
- Return a concrete `ResolvedModelCallTarget`.

Resolver behavior:

- If the requested logical role cannot be resolved, reject the orchestration strategy and let selector choose another strategy or passthrough.
- If the primary provider is unhealthy, use configured fallback only if it satisfies the same capability class.
- If no fallback exists, return a structured failure reason.

Design rule:

Orchestration config should refer to logical roles and constraints, not raw secret-bearing endpoints.

### 6.4 ModelCaller

Purpose:

Execute a provider call through unified request/response adaptation.

Responsibilities:

- Build provider-specific requests from normalized model call inputs.
- Support OpenAI-compatible and Anthropic-compatible request shapes already used by the proxy.
- Preserve image blocks when the provider supports vision.
- Parse response content, refusal/error content, token usage, finish reason, and provider error.
- Enforce per-call timeout and retry policy.
- Return structured `ModelCallResult`.

Output should include:

- `ok`
- `content`
- `raw_response_ref` or redacted debug payload;
- `usage`
- `latency_ms`
- `provider`
- `model`
- `finish_reason`
- `error_kind`
- `error_message`

Design rule:

The caller should not decide strategy fallback. It should report facts. Executor and QualityGate decide what to do next.

### 6.5 StrategyExecutor

Purpose:

Execute the selected orchestration strategy.

Supported action types in Milestone 1:

- Route
- Cascade
- Debate
- MoA

Future action types:

- CrossModal
- BranchSolveMerge

Execution rule:

Advanced strategies buffer full internal results and return a normal compatible final response. They may record internal stage events in trace, but they should not claim token-level streaming.

### 6.6 QualityGate

Purpose:

Turn judge output into enforceable decisions.

Responsibilities:

- Score candidate or final answer using a structured rubric.
- Validate required output format if specified.
- Enforce quality thresholds.
- Decide whether to accept, retry, escalate, fallback, or passthrough.
- Record reasons.

Rubric dimensions:

- `correctness`
- `completeness`
- `instruction_following`
- `evidence_quality`
- `safety_or_risk`
- `format_compliance`
- `confidence`

Decision output:

- `accept`
- `retry_same_strategy`
- `escalate_model`
- `fallback_to_single_model`
- `passthrough_original`
- `fail_with_visible_error`

Design rule:

A quality threshold that does not affect control flow is not a real quality gate.

### 6.7 TraceLedger

Purpose:

Persist enough information to debug and evaluate orchestration behavior.

Each orchestration record should include:

- request id;
- timestamp;
- client route;
- request model;
- task profile summary;
- selected strategy;
- selector score and tie-break info;
- provider/model calls;
- per-step latency;
- token usage;
- estimated cost;
- quality scores;
- fallback decisions;
- final status;
- user-visible response model;
- redacted errors.

Privacy:

- Do not persist full prompt/response by default unless an explicit debug setting enables it.
- Store hashes, lengths, task metadata, model names, and redacted snippets if needed.
- Any persisted payload must respect existing privacy settings.

### 6.8 MiniEvalHarness

Purpose:

Provide a small, stable regression suite that can prove the orchestration path works and catch quality regressions.

Initial eval set:

- 3 simple text tasks where direct route should win on latency/cost.
- 3 coding tasks where Debate or Cascade may improve correctness.
- 3 reasoning tasks where multi-candidate consistency matters.
- 2 format-constrained tasks where QualityGate catches non-compliance.
- 2 adversarial/conflicting-answer tasks where ranking matters.
- 2 image+text stubbed cases for Milestone 2.
- 2 audio stubbed cases for Milestone 3.

Metrics:

- pass/fail;
- quality score;
- strategy selected;
- model calls;
- total latency;
- total tokens;
- estimated cost;
- fallback rate;
- judge agreement if multiple judges are used.

Design rule:

The eval harness is not a research benchmark. It is a product regression guardrail.

## 7. Debate Algorithm Design

### 7.1 Current MVP Pattern to Replace

The weak version is:

1. Ask multiple models for answers.
2. Give answers to a judge.
3. Return judge summary.

This is not enough because:

- agents do not critique each other;
- judge may summarize weak answers without detecting errors;
- quality threshold may be decorative;
- failures can be hidden;
- there is no structured confidence or fallback path.

### 7.2 Release Debate Flow

Recommended flow:

1. Candidate generation.
2. Cross-critique.
3. Candidate revision.
4. Judge scoring.
5. Threshold decision.
6. Final synthesis or fallback.

Detailed steps:

#### Step 1: Candidate Generation

Run 2-3 proposer models in parallel.

Each proposer receives:

- original user request;
- system constraints;
- required output format;
- risk level;
- instruction to answer independently.

Proposer output should be normalized:

- `answer`
- `assumptions`
- `confidence`
- `known_risks`
- `format_notes`

#### Step 2: Cross-Critique

Each proposer sees anonymized peer answers.

Critique prompt asks for:

- strongest answer;
- factual or reasoning errors;
- missing constraints;
- format violations;
- unsafe suggestions;
- suggested correction.

Critique output should be structured:

- `target_candidate_id`
- `major_errors`
- `minor_errors`
- `missing_requirements`
- `score`
- `recommended_changes`

#### Step 3: Candidate Revision

Each proposer gets its own answer plus peer critique and may produce one revised answer.

Rules:

- A proposer may keep the original answer if critique is weak.
- Revision must not merge all candidates blindly.
- Revision should explicitly fix identified defects.

#### Step 4: Judge Scoring

Judge receives:

- original request;
- candidate answers;
- critiques;
- revised answers;
- rubric.

Judge returns:

- ranked candidates;
- per-candidate scores;
- whether candidates agree on final conclusion;
- best candidate id;
- whether synthesis is needed;
- threshold pass/fail;
- risk notes.

#### Step 5: Threshold Decision

If best candidate score >= strategy threshold:

- accept best candidate or synthesize final answer.

If score is below threshold but close:

- escalate to stronger judge/aggregator if configured.

If score is clearly below threshold:

- fallback to configured single strong model or passthrough.

If all candidates fail due provider errors:

- return visible orchestration failure or passthrough according to policy.

#### Step 6: Final Synthesis

Only synthesize when:

- multiple candidates have complementary correct parts;
- judge score says synthesis is safe;
- required format can be preserved.

Otherwise return the best scored candidate.

### 7.3 Debate Defaults

Recommended first-release defaults:

- proposer count: 2
- maximum rounds: 1 generation + 1 critique + 1 revision
- judge count: 1
- threshold: 0.75 for normal high-complexity tasks
- threshold: 0.85 for critical risk tasks
- max fallback depth: 1

Rationale:

This keeps latency bounded while still moving beyond simple fan-out/fan-in.

### 7.4 Debate Failure Modes

Handle explicitly:

- proposer timeout;
- partial proposer failure;
- judge failure;
- malformed judge JSON;
- candidates disagree strongly;
- all candidates low quality;
- final answer violates requested format;
- provider capability mismatch.

Fallback policy:

- If at least one candidate succeeds and judge works, continue with degraded candidate set.
- If judge fails, try one configured backup judge.
- If both judges fail, fallback to single model and record `judge_unavailable`.
- If output format cannot be validated, retry synthesis once, then fallback.

## 8. MoA Algorithm Design

### 8.1 Current MVP Pattern to Replace

The weak version is:

1. Run proposers in parallel.
2. Ask aggregator to combine answers.
3. Return aggregate.

This is insufficient because:

- weak or hallucinated proposer outputs can contaminate the final answer;
- aggregation lacks ranking;
- there is no layer depth control;
- no quality threshold enforces whether the aggregate is good enough.

### 8.2 Release MoA Flow

Recommended flow:

1. Initial proposer layer.
2. Candidate normalization.
3. Ranking/filtering.
4. Optional second layer.
5. Aggregation.
6. Quality gate.

#### Step 1: Initial Proposer Layer

Run 2-4 models selected for diversity:

- one cheap/fast model;
- one coding/reasoning model;
- one high-quality general model if cost allows;
- one domain-specific model if configured.

Each proposer receives the same original request and must answer independently.

#### Step 2: Candidate Normalization

Normalize proposer outputs:

- extract final answer;
- detect refusal or empty content;
- detect format compliance;
- estimate confidence;
- record latency and cost.

Invalid candidates should be excluded from aggregation unless the task is asking to compare failures.

#### Step 3: Ranking and Filtering

Ranker can be:

- the configured aggregator model with a ranking prompt;
- a dedicated judge role;
- a deterministic heuristic for simple tasks.

Ranking criteria:

- correctness;
- completeness;
- instruction following;
- format compliance;
- consistency with other candidates;
- risk.

Filter rules:

- Drop candidates below minimum score.
- Drop duplicates unless they increase confidence.
- Preserve minority candidate if it identifies a critical issue.

#### Step 4: Optional Second Layer

For high-complexity tasks, run a second layer where selected models see:

- original request;
- top-ranked candidate summaries;
- known disagreements;
- missing requirements.

Second-layer output should improve rather than repeat.

Layering should be disabled for simple tasks.

#### Step 5: Aggregation

Aggregator receives:

- original request;
- ranked candidates;
- disagreement summary;
- required output format;
- risk notes.

Aggregator produces:

- final answer;
- selected evidence from candidates;
- unresolved uncertainty if any;
- confidence score.

#### Step 6: Quality Gate

QualityGate checks aggregate against threshold.

If pass:

- return aggregate.

If fail:

- fallback to best candidate, stronger model, or passthrough depending on policy.

### 8.3 MoA Defaults

Recommended first-release defaults:

- proposer count: 3
- layers: 1 by default
- second layer: only if complexity >= 0.85 or risk high/critical
- ranker: aggregator model unless a dedicated judge is configured
- threshold: 0.75 normal, 0.85 critical
- max total model calls: configurable, default 5

### 8.4 MoA Failure Modes

Handle explicitly:

- proposer partial failure;
- all proposers fail;
- aggregator timeout;
- aggregator produces malformed answer;
- final answer below threshold;
- cost budget exceeded;
- latency budget exceeded.

Fallback policy:

- If aggregator fails but a high-ranked candidate passes threshold, return best candidate.
- If ranking fails, use deterministic candidate heuristic and record degraded ranking.
- If cost/latency budget exceeded, stop additional layers and aggregate current best.

## 9. Cascade Algorithm Design

Cascade remains useful for cost control.

Flow:

1. Try cheap model.
2. QualityGate scores result.
3. If pass, return cheap result.
4. If fail, try mid model.
5. If pass, return mid result.
6. If fail and risk/complexity warrants it, try strong model.
7. Record every escalation.

Rules:

- Cascade must not hide repeated failures.
- Each step must be traceable.
- Thresholds should be task-dependent.
- Budget caps must be respected.

Good use cases:

- simple Q&A;
- summarization;
- routine coding explanation;
- low-risk transformations.

Bad use cases:

- image/audio tasks without capability match;
- strict tool calls;
- tasks requiring real-time streaming;
- critical legal/medical/financial claims unless explicitly configured.

## 10. Route Algorithm Design

Route is the cheapest path and should remain the default for simple tasks.

Flow:

1. Resolve target model role from strategy.
2. Verify provider health and capability.
3. Call one model.
4. Optionally run lightweight QualityGate for high-risk tasks.
5. Return result.

Route must be used when:

- task complexity is low;
- risk is low;
- no image/audio processing is needed;
- user explicitly pins a model and orchestration policy respects pinning;
- advanced strategy budget would not be justified.

## 11. Cross-Modal Architecture

The full architecture should support text, image, audio, and mixed requests, but implementation is staged.

### 11.1 ModalityProfile

RequestProfiler should produce:

- `has_text`
- `has_image`
- `has_audio`
- `image_count`
- `audio_count`
- `mime_types`
- `requires_vision`
- `requires_transcription`
- `requires_audio_output`
- `safe_for_text_only_fallback`

### 11.2 CrossModal Action

A future `StrategyAction::CrossModal` should support:

- detector;
- extractor;
- reasoner;
- aggregator;
- output adapter.

Initial variants:

- `image_text_reasoning`
- `audio_text_reasoning`
- `image_audio_text_reasoning`

### 11.3 Image+Text Flow

Milestone 2 flow:

1. Detect image blocks.
2. Resolve vision model.
3. Ask vision model for structured visual description.
4. Combine visual description with user text.
5. Run text reasoner or Debate/MoA depending on complexity.
6. Aggregate final answer.
7. Record image extraction trace.

Structured visual description fields:

- objects;
- text visible in image;
- layout;
- relevant details;
- uncertainties;
- safety notes.

### 11.4 Audio Flow

Milestone 3 and 4 flow:

1. Detect audio input.
2. Resolve STT provider.
3. Transcribe audio.
4. Extract speaker/time/event metadata if supported.
5. Run text reasoner.
6. Optionally generate audio output if requested.

Milestone 1 requirement:

Audio requests must not be silently routed into text-only orchestration. They should passthrough or return a clear unsupported/fallback trace depending on compatibility.

### 11.5 Multimodal Fallback Rules

- If image is present and no vision provider is available, do not run text-only Debate over an incomplete prompt unless `safe_for_text_only_fallback` is true.
- If audio is present and no STT provider is available, passthrough to original provider if it supports audio; otherwise return a visible capability error.
- If mixed modalities are present, every selected model must support the required input or the pipeline must convert unsupported media into structured text first.

## 12. Configuration Design

### 12.1 Strategy Config

Strategies should include:

- id;
- enabled;
- priority;
- description;
- when conditions;
- modality support;
- budgets;
- action;
- thresholds;
- fallback policy;
- trace level.

Example shape:

```yaml
strategies:
  debate_high_risk:
    enabled: true
    priority: 80
    description: "Structured Debate for high-risk complex reasoning"
    when:
      complexity: [0.70, 1.0]
      risk: ["high", "critical"]
      modalities: ["text"]
      has_tools: false
    budgets:
      max_calls: 6
      max_latency_ms: 60000
      max_cost_usd: 0.50
    action:
      type: "debate"
      proposers: ["cheap_reasoner", "mid_reasoner"]
      judge: "frontier_judge"
      max_rounds: 1
      critique: true
      revision: true
      quality_threshold: 0.80
    fallback:
      on_quality_fail: "frontier_single"
      on_judge_fail: "backup_judge"
      on_provider_fail: "passthrough"
```

### 12.2 Model Role Config

Model roles should not store secrets.

Example:

```yaml
model_roles:
  cheap_reasoner:
    capability: ["text"]
    quality_class: "cheap"
    preferred_provider_tags: ["fast", "low_cost"]

  frontier_judge:
    capability: ["text", "json"]
    quality_class: "frontier"
    preferred_provider_tags: ["judge", "high_quality"]

  vision_extractor:
    capability: ["vision", "json"]
    quality_class: "vision"
```

ProviderModelResolver maps these roles to existing provider entries.

### 12.3 Defaults

Recommended defaults:

- Orchestration feature flag remains explicit.
- Advanced strategies default to disabled unless provider roles are resolvable.
- Release build can ship with safe config but not force users into multi-model calls.
- UI should indicate when a strategy cannot activate because provider roles are missing.

## 13. Provider Integration

### 13.1 Single Source of Truth

Existing Provider configuration should own:

- provider type;
- base URL;
- API key;
- model id;
- enabled flag;
- health status;
- timeout;
- rate limits;
- fallback model/provider.

Orchestration should own:

- logical role;
- strategy selection;
- call sequence;
- quality gates;
- trace records.

### 13.2 Capability Matching

Each provider model should advertise or be configured with capabilities:

- text generation;
- vision input;
- audio input;
- audio output;
- structured JSON;
- tool calls;
- streaming;
- context length.

Resolver must reject a model if required capability is missing.

### 13.3 Fallback

Fallback should be structured:

- provider unhealthy;
- model unavailable;
- timeout;
- rate limited;
- auth failed;
- malformed response;
- quality failed.

Each fallback should record:

- original target;
- fallback target;
- reason;
- success/failure.

## 14. Response and Streaming Semantics

Milestone 1 rule:

Advanced strategies buffer internally and return a complete compatible response.

Behavior:

- If original request is non-streaming, return normal response.
- If original request asks for streaming and selected strategy is Route, preserve streaming if existing proxy supports it.
- If original request asks for streaming and selected strategy is Debate/MoA/Cascade, either:
  - bypass orchestration and passthrough streaming; or
  - run orchestration and return a compatible final chunked SSE response only if current protocol code safely supports it.

Recommended first-release default:

- For strict streaming clients, passthrough unless user explicitly enables buffered orchestration for streaming requests.

Reason:

This avoids pretending that multi-model internal stages are real-time token streams.

## 15. Error Handling

### 15.1 Error Categories

Use structured error kinds:

- `strategy_not_eligible`
- `no_strategy_match`
- `provider_role_unresolved`
- `provider_unhealthy`
- `provider_auth_failed`
- `provider_rate_limited`
- `provider_timeout`
- `model_capability_mismatch`
- `model_response_malformed`
- `judge_failed`
- `quality_threshold_failed`
- `budget_exceeded`
- `modality_unsupported`
- `output_format_invalid`

### 15.2 Failure Visibility

Every orchestration failure must do at least one of:

- write trace ledger record;
- write structured log event;
- expose response metadata if compatible;
- surface UI status in settings/debug view.

Silent fallback is not acceptable for release.

### 15.3 User-Facing Behavior

Default user-facing behavior should be stable:

- If orchestration fails but passthrough can answer, return passthrough response and record fallback.
- If no provider can answer the requested modality, return a clear provider capability error.
- If quality threshold fails after fallback, return the best safe answer only if policy allows it; otherwise return a visible failure.

## 16. Observability Design

### 16.1 Trace Record Schema

Recommended fields:

- `id`
- `request_id`
- `created_at`
- `strategy_id`
- `strategy_type`
- `task_type`
- `complexity_score`
- `risk_level`
- `modalities`
- `selected_reason`
- `rejected_strategies`
- `steps`
- `quality_scores`
- `final_decision`
- `fallbacks`
- `total_latency_ms`
- `total_prompt_tokens`
- `total_completion_tokens`
- `estimated_cost`
- `status`
- `error_kind`
- `error_message_redacted`

### 16.2 Step Record Schema

Each step should include:

- `step_id`
- `step_type`
- `role`
- `provider`
- `model`
- `started_at`
- `latency_ms`
- `usage`
- `status`
- `score`
- `error_kind`

### 16.3 Trace Levels

Levels:

- `off`: no trace except critical errors.
- `summary`: metadata, decisions, costs, failures.
- `debug`: includes redacted snippets and rejected candidates.
- `full`: stores full prompt/response only when explicit privacy setting allows.

Default:

- `summary`.

## 17. Testing Design

### 17.1 Unit Tests

Required:

- RequestProfiler detects text/image/audio/tools/streaming.
- StrategySelector is deterministic under tie conditions.
- ProviderModelResolver rejects missing capabilities.
- Debate threshold pass/fail controls flow.
- MoA ranking drops invalid candidates.
- QualityGate parses valid judge JSON and handles malformed JSON.
- TraceLedger redacts sensitive data.

### 17.2 Integration Tests

Required:

- Route end-to-end with mock provider.
- Cascade escalates after low score.
- Debate runs proposer, critique, revision, judge, final response.
- Debate falls back when judge fails.
- MoA ranks candidates and aggregates.
- MoA returns best candidate if aggregator fails.
- Streaming request is passed through or buffered according to config.
- Image request is not routed to text-only strategy.
- Audio request is not routed to text-only strategy.

### 17.3 Provider Mock Tests

Mock provider should simulate:

- success;
- timeout;
- rate limit;
- auth failure;
- malformed JSON;
- empty content;
- low-quality answer;
- high-quality answer;
- capability mismatch.

### 17.4 Eval Harness Tests

Required:

- Eval runner can execute fixed local cases with mock providers.
- Eval output includes strategy, scores, latency, cost, and pass/fail.
- Regression threshold can fail CI or local verification command when configured.

### 17.5 Build Verification

Before release:

- `pnpm.cmd typecheck`
- `pnpm.cmd test:unit`
- `pnpm.cmd build:renderer`
- `cargo test`
- `cargo build`
- orchestration eval harness command

Warnings:

Rust unused/dead-code warnings in orchestration should be reduced where possible, especially for modules that are supposed to be release path code.

## 18. Acceptance Criteria

Milestone 1 is complete only when all of the following are true:

1. Strategy selection is deterministic and tested.
2. Debate quality threshold affects control flow and is tested.
3. MoA ranks or filters candidates before aggregation.
4. Provider calls use existing Provider configuration, not hardcoded URLs or independent environment-only secrets.
5. Provider capability mismatch prevents unsafe strategy selection.
6. Every advanced orchestration request writes a trace record.
7. Fallbacks are visible in trace/logs.
8. End-to-end mock tests cover Route, Cascade, Debate, and MoA.
9. Streaming behavior is explicit and tested.
10. Image/audio requests are not silently swallowed by text-only orchestration.
11. Minimal eval harness can compare direct, Cascade, Debate, and MoA paths.
12. Existing build and test commands pass.

## 19. Release Messaging

Allowed release language:

- "Provider-integrated multi-model orchestration."
- "Deterministic strategy selection."
- "Structured Debate and MoA execution."
- "Quality gates and fallback."
- "Traceable cost, latency, and model decisions."
- "Initial eval harness for regression checks."
- "Staged cross-modal architecture with image/audio support planned and guarded by capability detection."

Avoid release language:

- "Small models always beat large models."
- "Complete cross-modal intelligence."
- "Fully autonomous super model."
- "Universal Debate/MoA superiority."
- "Production-proven benchmark superiority" unless eval data actually supports it.

## 20. Implementation Handoff Notes

Recommended implementation order:

1. Add deterministic selector ordering and tests.
2. Add ProviderModelResolver and replace orchestration hardcoded model calls.
3. Define structured model call result and error kinds.
4. Implement QualityGate decisions and threshold enforcement.
5. Upgrade Debate execution with critique/revision/judge/fallback.
6. Upgrade MoA execution with ranking/filtering/aggregation/fallback.
7. Add TraceLedger records.
8. Add end-to-end provider mocks.
9. Add MiniEvalHarness.
10. Add modality guards for image/audio.
11. Add image+text CrossModal in a later milestone.

Critical implementation constraint:

Do not start by building the most complex cross-modal path. First make text Debate/MoA reliable, observable, and provider-integrated. Then extend the same architecture to image and audio.

## 21. Open Decisions Already Resolved

The following decisions were made during brainstorming:

- Choose "release-trustworthy core + minimal eval" as the first implementation direction.
- Keep streaming conservative for advanced strategies: buffer full result or passthrough strict streaming.
- Use existing Provider configuration as the source of truth.
- Cover full text/image/audio cross-modal architecture in the design, but deliver it by milestones.
- First release focuses on reliability, Provider integration, observability, and mock/eval proof.

## 22. Risks and Mitigations

### Risk: Latency becomes too high

Mitigation:

- Cap maximum calls.
- Disable second MoA layer except for high-complexity tasks.
- Passthrough strict streaming requests by default.
- Record latency per step.

### Risk: Cost surprises users

Mitigation:

- Add per-strategy cost budget.
- Estimate cost in TraceLedger.
- Prefer Route/Cascade for simple tasks.
- Require explicit enablement for high-cost strategies.

### Risk: Judge is wrong

Mitigation:

- Use structured rubric.
- Add backup judge for judge failure.
- Use deterministic format validation where possible.
- Compare candidate consistency.
- Add eval cases for judge mistakes.

### Risk: Provider config mismatch

Mitigation:

- Resolve roles at startup or strategy activation.
- Show missing roles/capabilities in UI or trace.
- Reject strategy before execution if required provider is missing.

### Risk: Cross-modal claims exceed implementation

Mitigation:

- Add modality guards in Milestone 1.
- Ship image+text and audio in separate milestones.
- Use release wording that reflects actual implementation.

## 23. DeepSeek Development Checklist

DeepSeek should use this checklist before coding:

- Confirm current Provider configuration APIs and data models.
- Identify exact files for selector, config, executor, model caller, proxy handler, and tests.
- Preserve existing passthrough behavior.
- Avoid touching unrelated frontend code unless needed for trace visibility.
- Do not commit unrelated existing dirty worktree changes.
- Add tests before broad refactors.
- Run the full verification commands listed in this document.
- Summarize any behavior that intentionally differs from this spec before implementation.

## 24. Definition of Done

The design is implemented when:

- A developer can enable orchestration, send a request, and inspect why the strategy was selected.
- Debate and MoA produce structured internal records and enforce quality thresholds.
- Provider calls honor the configured provider system.
- Failures and fallbacks are visible.
- Tests prove the main success and failure paths.
- The eval harness gives a repeatable local signal.
- Release notes can honestly describe the feature without exaggeration.
