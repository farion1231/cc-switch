# 有一个特别的想法，当前的高端大模型成本高，计算复杂度大，高端大模型的优点就在于拆

**User - --**

有一个特别的想法，当前的高端大模型成本高，计算复杂度大，高端大模型的优点就在于拆解和。处理非常复杂的问题，而且幻觉要少。但是我们在大多应用的时候，尤其在本地智能体设计的时候，不是只用了的模型，对于个人工作台，我们需要的是多agent多模型相互配合。我的设想。如果我的本地独立独立agent，可以 link同时使用多款大模型又复杂的有便宜，有的大模型可以支持图像，有大模型可以支持语音。有的大模型能力强，有大模型能力弱，这样我方面组合，是否理论依据，如何做才能实现，几个不同类型的组合这种低端模型，甚至逻辑搭配，通过什么方的组合能够达到顶级Claude 4.7，甚至突破更好的negligible？有没有理论的算法和依据？如果有本地应该如何组织和框架。详细给我调查一下，给出完整的方案，如果这个方案可行，那么我要使用cc switch，开发一个本地工具，使用多个API接口组合，使用各种策略，达到最佳，最高水平的效果，请给出整个方案，和开发的claude code 的提示词。

**Assistant - --**

这是一个非常有价值的工程方向。让我先调研一下当前多模型协作（multi-agent orchestration、model routing、ensemble）的理论依据和最新实践，然后给你完整方案。我已经有了核心理论依据。让我再确认两点：cc-switch 这个工具的实际功能，以及最新的级联路由（cascade/RouteLLM）的成本节省数据。我已经收集到了足够的理论依据和工具信息。下面给你一份完整的、可落地的方案。

---

# 多模型协作本地编排系统：理论、方案与 Claude Code 开发提示词

## 一、你的设想有没有理论依据？——有，而且非常充分

你的直觉本质上踩中了 2024–2026 年 LLM 工程领域最热的三条主线：**Routing（路由）、Cascade（级联）、Mixture-of-Agents（智能体混合）**。先说结论：**用一组中低端模型 + 合理编排，确实能在很多任务上达到甚至超过单一顶级模型，同时成本大幅下降。** 但有边界条件，我会一并讲清楚。

### 依据 1：Mixture-of-Agents（MoA）——弱模型组合可超越顶级单模型

这是你设想最直接的学术支撑。MoA 模型在 AlpacaEval 2.0、MT-Bench 和 FLASK 上达到了最先进性能，超过了 GPT-4 Omni；例如仅使用开源 LLM 的 MoA 以 65.1% 对 57.5% 的明显差距领先 AlpacaEval 2.0。

关键机制是"协作性"：即使个体较弱，LLM 在能看到其他模型的回复时也表现更好；当模型使用其他模型的回复时其胜率会提升，这种协作改善了所有模型的胜率，无论其个体强弱，体现了集体智能的好处。

架构上：MoA 由多层组成，每层包含若干 LLM 智能体，每个智能体处理上一层的输出生成更精炼的回复，迭代精炼过程确保最终输出更全面、更稳健。

成本上也成立：MoA-Lite 变体证明，可以用极小的成本实现相当或更好的性能。

### 依据 2：RouteLLM——智能路由可省 80%+ 成本而几乎不掉质量

这支撑你"复杂的用贵的、简单的用便宜的"那部分设想。用 Chatbot Arena 公开数据训练的四种路由器，相比只用 GPT-4，在 MT Bench 上降本超过 85%、MMLU 上 45%、GSM8K 上 35%，同时仍达到 GPT-4 性能的 95%。

更惊人的是路由器的泛化性：RouteLLM 路由器可跨模型对迁移——在 GPT-4/Mixtral 偏好数据上训练的路由器无需重训即可泛化到 Claude 3 Opus/Llama 3，分类器学到的是与具体模型无关的"查询难度"。

而且训练成本极低：所需数据比预期少，有效路由器仅用不到 1500 个标注样本就能训练出来，不到完整 Arena 数据集的 2%。

### 依据 3：Cascade（级联）——先便宜后升级，按不确定性触发

这对应你"逻辑搭配"的想法。级联链：提示流经一系列能力递增的模型，一旦达到质量阈值即停止；便宜模型处理简单任务，仅在必要时升级。

最新的级联研究给出了非常漂亮的数字：结合质量、成本和不确定性评分的研究达到了 GPT-4 准确率的 97%，而成本只有 GPT-4 的 24%——4 倍降本且输出质量几乎相同，代价是触发升级时增加延迟。

级联的精髓在于：它先试便宜模型，质量不足才升级；这是最复杂的策略，但有真正的优势——你不需要预先预测查询难度，便宜模型自身的不确定性信号驱动升级。

### 依据 4：并行辩论 / 评审——多模型投票降低幻觉

这对应你"幻觉要少"的诉求。并行辩论：对于高风险决策，多个模型同时生成独立回复，由一个评审模型综合它们，显著降低事实任务上的幻觉率。

### ⚠️ 重要的边界条件（必须告诉你的"反例"）

你的设想不是无条件成立。2026 年初有一项重要发现挑战了"模型越多样越好"的假设：Sato 和 Ito（2025）证明 Self-MoA——把单个顶级模型多次查询——在 AlpacaEval 2.0 上比混合多样模型高出 6.6%，动摇了"模型多样性必然提升性能"的假设。

原因在于"短板效应"：往组合里塞入太弱的模型，反而会拖累聚合结果。**所以正确策略不是"模型越多越好"，而是"在能力相近的高质量候选里做多样化组合，并用强模型做聚合/评审"。**

另一个趋势性背景：多个基准套件记录了不同厂商 LLM 能力的趋同，2026 年 1 月 Open LLM Leaderboard v2 显示前 10 名模型 MMLU 聚集在 87.2–90.1 的 3 分区间内，Chatbot Arena ELO 排名也显示 OpenAI、Anthropic、Google、Meta、阿里的前沿模型置信区间相互重叠。 这意味着：当模型能力趋同时，主导的优化变量变成了智能体协调的结构拓扑；编排结构成为性能增益的首要杠杆。

---

## 二、最优组合的"理论算法"清单

把上面理论整理成你系统里可调用的 5 种核心策略（按从轻到重排序）：

| 策略 | 机制 | 何时用 | 成本/质量 |
|---|---|---|---|
| **Router（路由）** | 分类器判断难度，单选一个模型 | 海量请求、需低延迟 | 最省钱，~95% 质量 / 15–50% 成本 |
| **Cascade（级联）** | 便宜模型先试，不确定才升级 | 难度分布不均的任务流 | 97% 质量 / 24% 成本 |
| **MoA（混合智能体）** | 多 proposer 并行 → aggregator 聚合，可多层 | 追求超越单模型的最高质量 | 较贵，但可超 GPT-4o |
| **Debate/Vote（辩论投票）** | 多模型独立答 → 评审综合 | 高风险、需低幻觉的事实任务 | 贵，幻觉显著下降 |
| **Specialist Handoff（专家交接）** | 按模态/能力把子任务交给不同模型 | 多模态（图/音/文）、长流程 agent | 取决于子任务 |

**组合公式（推荐的"金字塔"编排）：**

```
                  ┌─────────────────────────┐
   用户请求 ──►   │  Router (难度+模态分类)   │
                  └─────────────┬───────────┘
            ┌──────────────┬────┴─────┬──────────────┐
            ▼              ▼          ▼              ▼
      简单/闲聊      代码/逻辑     多模态        高风险事实
      便宜小模型      Cascade      Specialist    Debate+Judge
      (单模型)      (小→大升级)   (图/音专用)   (3模型投票)
                          │
                    质量不足时升级
                          ▼
                  MoA 聚合层 (强模型 aggregator)
```

核心原则归纳：
1. **能力感知匹配**——采用能力感知匹配策略：涉及工具调用、多步推理或长程规划的复杂任务交给智能体方案，知识检索或文本生成等直接任务交给通用 LLM。
2. **角色专业化**——MoA 把 LLM 分配到它们最擅长的角色，例如 GPT-4o 既擅长提议又擅长聚合，而 WizardLM 最适合做 proposer。 → 把**最强模型留作 aggregator/judge**，便宜模型做 proposer。
3. **多样性但有下限**——proposer 之间要多样，但都要过质量门槛，避免短板拖累。
4. **不确定性驱动升级**——级联用便宜模型的置信度/自我评估分数触发升级。

---

## 三、本地系统架构与框架

### 整体架构

```
┌────────────────────────────────────────────────────────────┐
│                     本地 Orchestrator (你的工具)             │
│                                                              │
│  ┌──────────┐   ┌─────────────┐   ┌──────────────────────┐ │
│  │ Classifier│──►│ Strategy     │──►│ Execution Engine     │ │
│  │ (路由器)  │   │ Selector     │   │ (router/cascade/MoA/ │ │
│  │  embed+   │   │ (选策略)     │   │  debate/handoff)     │ │
│  │  小模型   │   └─────────────┘   └──────────┬───────────┘ │
│  └──────────┘                                  │             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ Provider Pool (统一 OpenAI/Anthropic 兼容适配层)        │  │
│  │  Claude · GPT · Gemini · DeepSeek · GLM · Qwen ·       │  │
│  │  Kimi · MiniMax · 本地 Ollama · 视觉模型 · 语音模型     │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ Health/Failover│ │ Cost/Token    │ │ Cache + Log      │  │
│  │ 熔断+故障转移  │ │ Budget 控制   │ │ (训练路由器用)   │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
└────────────────────────────────────────────────────────────┘
```

生产级系统还需要：基础设施层面的健康监控、提供商故障转移、熔断器。

### 关于 cc-switch 的现实定位（重要修正）

你提到用 cc-switch 开发。需要先厘清它的真实能力：现代 AI 编程依赖 Claude Code、Codex、Gemini CLI 等 CLI 工具，但每个都有自己的配置格式；切换 API 提供商意味着手动编辑 JSON/TOML/.env，CC Switch 给你一个统一的桌面应用来管理这些工具。

也就是说，**cc-switch 本质是"配置切换器 + 反向代理网关"，它本身不做智能编排**。但它的两个特性对你极有用：

1. 它通过应用内代理网关提供第三方 provider 切换，支持基于角色的模型映射（sonnet/opus/haiku）和 supports1m 长上下文标志。——你可以把 haiku 槽位映射到便宜模型、opus 槽位映射到强模型。
2. 部分聚合 provider 已内置智能路由能力，例如 LemonData 一个 API key 接入 300+ 模型（GPT、Claude、Gemini、DeepSeek 等），价格比官方低 30–70%，带自动故障转移和智能路由。

**正确的工程定位：** cc-switch 当"统一 provider 接入层 + 代理网关"，你真正要开发的智能编排逻辑放在它前面，作为一个本地 orchestrator 服务（暴露一个 OpenAI 兼容端点，再把 base_url 指向你自己的服务）。这样 Claude Code 调你的服务 → 你的服务做路由/级联/MoA → 再通过 cc-switch 代理打到各家 provider。

### 推荐技术栈

- **编排框架**：LangGraph（图式工作流，适合表达 cascade/debate 拓扑）或自写轻量 FastAPI 服务
- **网关层**：cc-switch（provider 管理 + failover）或 LiteLLM（统一 OpenAI 格式适配 100+ 模型）
- **路由器**：复用 RouteLLM 开源路由器（矩阵分解/BERT 分类器），矩阵分解能用 26% 的 GPT-4 调用量达到 95% 的 GPT-4 性能。
- **本地小模型**：Ollama 跑 Qwen/Llama 做分类器和便宜 proposer
- **存储**：SQLite（请求日志，后续训练自定义路由器用）

---

## 四、给 Claude Code 的开发提示词

以下是可直接粘贴给 Claude Code 的完整工程提示词。建议分阶段交付。

````markdown
# 项目：本地多模型智能编排网关 (LocalMoE-Gateway)

## 角色
你是一名资深 AI 基础设施工程师。请为我从零构建一个本地运行的多模型编排网关。

## 目标
对外暴露一个 **OpenAI 兼容的 /v1/chat/completions 端点**，内部根据请求难度、模态和风险等级，
自动在多个 LLM provider 之间选择「路由 / 级联 / 混合智能体(MoA) / 辩论投票 / 专家交接」策略，
以最低成本达到接近或超过单一顶级模型(如 Claude Opus)的质量。下游 provider 通过
cc-switch 的本地代理网关 (默认 http://localhost:port) 或 LiteLLM 统一接入。

## 技术栈
- Python 3.11 + FastAPI + uvicorn
- httpx (异步), pydantic v2
- SQLite (请求日志 + 后续训练路由器)
- 配置用 YAML (providers.yaml + strategies.yaml)
- 可选：sentence-transformers 做查询 embedding 分类

## 架构模块 (请分文件实现)
1. `gateway.py`        — FastAPI 入口，OpenAI 兼容端点，SSE 流式支持
2. `providers.py`      — Provider 抽象层，统一封装 Anthropic/OpenAI 格式，
                          支持 base_url 指向 cc-switch 代理；含健康检查与熔断
3. `classifier.py`     — 请求分类器：输出 {难度: easy/medium/hard, 模态: text/image/audio,
                          风险: low/high}。先用规则+关键词，预留接口接 RouteLLM 路由器
4. `strategies/`       — 每种策略一个文件:
     - router.py       — 单选一个最合适模型
     - cascade.py      — 便宜模型先试 → 自评分/置信度不足则升级 (阈值可配)
     - moa.py          — N 个 proposer 并行生成 → aggregator 强模型聚合，支持多层
     - debate.py       — 3 模型独立回答 → judge 模型综合，降低幻觉
     - handoff.py      — 按模态把子任务交给图像/语音/文本专用模型
5. `selector.py`       — 策略选择器：根据 classifier 结果 + 预算选择上面某个策略
6. `budget.py`         — token/成本统计与每日预算上限，超预算自动降级到便宜策略
7. `logger.py`         — 记录 (query, 选中策略, 各模型输出, 最终输出, 成本, 延迟) 到 SQLite
8. `config/`           — providers.yaml / strategies.yaml 示例

## 关键实现要求
- **级联升级信号**：用便宜模型输出的 self-consistency 或让它输出一个 confidence 字段，
  低于阈值则升级。也支持「让 judge 小模型给 0-1 打分」两种模式，可配置。
- **MoA 角色分配**：最强模型固定做 aggregator/judge，便宜模型做 proposer；
  proposer 池要可配置且支持质量下限过滤（避免弱模型拖累）。
- **故障转移**：任一 provider 超时/报错 → 自动切到 backup provider，记录熔断状态。
- **成本优先**：每个 provider 在 yaml 里标注 $/1M input/output token；
  selector 在满足质量约束下选成本最低的方案。
- **流式**：router/cascade 命中时支持流式透传；MoA/debate 因需聚合可先非流式。
- **可观测**：提供 /stats 端点返回各策略命中率、平均成本、平均延迟、升级率。

## providers.yaml 示例结构
```yaml
providers:
  - id: claude-opus
    base_url: http://localhost:17666/v1   # cc-switch 代理
    api_key_env: OPUS_KEY
    model: claude-opus-xxx
    role: [aggregator, judge, hard]
    cost_in: 15.0
    cost_out: 75.0
  - id: deepseek
    base_url: http://localhost:17666/v1
    model: deepseek-chat
    role: [proposer, easy, medium]
    cost_in: 0.27
    cost_out: 1.10
  - id: qwen-vl
    role: [vision]
  - id: whisper-local
    role: [audio]
```

## 分阶段交付
- **阶段1 (先做)**：gateway + providers + router + cascade + failover + 日志。先能跑通。
- **阶段2**：moa + debate + selector + budget。
- **阶段3**：classifier 接入真实 RouteLLM 路由器；用 SQLite 日志训练自定义难度分类器；
  加 /stats 仪表盘。

## 验收标准
- 用一组便宜模型 (DeepSeek/GLM/Qwen) + 一个强模型 (Claude/GPT) 跑一批混合难度任务，
  目标：总成本相比"全用强模型"下降 ≥60%，质量(用 judge 评分)保持 ≥90%。
- 给出一个 benchmark 脚本 `eval.py` 对比单模型 vs 各策略的成本/质量/延迟。

请先输出项目目录结构和阶段1的完整代码，并说明如何把 Claude Code 的 base_url 指向本网关。
````

---

## 五、给你的关键建议（避坑）

1. **别盲目堆模型**：基于上面的 Self-MoA 反例，proposer 池建议 3–5 个能力相近的中端模型 + 1 个强 aggregator，而不是塞 10 个杂牌模型。
2. **路由器优先于 MoA**：80% 的成本节省来自简单的难度路由（RouteLLM 数据），MoA/debate 只对真正难的、高价值的任务开启。
3. **延迟是真实代价**：级联在触发升级时增加延迟——先调便宜模型、评分、再调贵模型，会让困难情况的延迟大约翻倍；对 300ms 以内的实时聊天往往不可接受。 所以交互式场景用 router，批处理/agent 场景才用 cascade/MoA。
4. **先用现成轮子**：RouteLLM 路由器和 LiteLLM 网关都是开源的，RouteLLM 由 UC Berkeley、Anyscale 和 Canva 研究者发表于 ICLR 2025，提供可降本 85% 而保持 95% GPT-4 性能的训练好的路由器。 直接复用，别重造。
5. **拿日志训练你自己的路由器**：因为只需要 ~1500 条样本，运行一段时间后你完全可以基于自己的真实任务分布训练一个更贴合的路由器。

**一句话总结：** 你的设想理论上完全成立且有大量论文与生产数据支撑，但成功的关键不是"模型多"，而是"编排结构好 + 强模型做聚合/评审 + 难度驱动路由"。用 cc-switch 做统一接入层、在它前面自建一个 OpenAI 兼容的编排网关，是当前最务实的落地路径。

需要我把上面提示词里的**阶段1代码**直接写出来吗？或者帮你设计 `eval.py` 的具体评测方案？

**User - --**

我要是用superpower开发，请给出完整的描述提示词，我需要在这个组合中具有全部模态，包括音乐，图像，大模型，素有的常规模型的都有的能力，而且可以达到超越单一模型水平，编程，文字，理解长任务，超大山下文，记忆等。尤其对于因为大多数时间是编程问题和应用，对于长上下文、隐含规划能力、错误抑制、抽象推理、指令遵循和上下文整合，包括实现的所有的注意事项，开发注意事项。

**Assistant - --**

你提到用 **Superpower** 开发。在给出完整提示词前，我需要确认一下这个工具的具体形态——是 Claude Code 的 Superpower 框架/插件，还是别的工具，因为提示词的写法（slash command、subagent、skill 结构）取决于它。让我查一下最新信息。现在我对 Superpowers 有了准确理解。先澄清一个关键认知，再给你完整方案。

## 先厘清：Superpowers 是什么、不是什么

Superpowers 不是一个"模型"或"多模态能力包"，而是一套**工程方法论框架**。它是 Claude Code 插件和多宿主智能体技能框架，把一套固执己见的工程文化打包成一个 markdown 文件夹——同一文件夹可在 Claude Code、Cursor、Codex、GitHub Copilot CLI、Gemini CLI 和 OpenCode 上使用，没有微调模型、没有专有 SDK，只有一个 skills 目录、十四个 SKILL.md 文件和一个会话钩子。

它的核心价值在于**纪律强制**：框架说的是"你将遵循 TDD"而不是"你可以选择遵循 TDD"，它通过架构而非文档来强制纪律。具体机制包括：brainstorming（在提出可接受的设计前拒绝写代码）、writing-plans（把功能拆成 2–5 分钟的任务）、test-driven-development（强制红绿重构，把"之后再写测试"当作删除实现重来的理由）、systematic-debugging（四阶段流程，禁止修复未理解的问题）、subagent-driven-development（把实现派发给只拿到计划和测试的新子智能体，再派第二个智能体审查）。

关键的架构创新对你的"长上下文 / 错误抑制"诉求极其重要：它为计划中的每个任务派发一个全新智能体，每次两阶段审查（先规范合规，再代码质量）；每个子智能体从干净状态开始，只接收任务描述和相关上下文，而非完整对话历史，从而防止上下文污染，让协调器在不耗尽上下文窗口的情况下管理大量任务——Claude 自主工作数小时而不偏离计划并不罕见。

**这正是你要的"模型编排"和"超大上下文/记忆"的底层机制。** 而且对你"低端模型组合超越顶级单模型"的设想，官方文档直接印证：推荐用 Claude Opus 这样的强模型做头脑风暴和规划，而实现子智能体可以用更快的模型而无明显质量损失，因为每个任务都足够明确，不需要复杂推理。这就是你上一轮要的"强模型做 aggregator/planner、弱模型做 proposer/worker"的工程化落地。

## 现实校正（你必须知道的边界）

1. **Superpowers 本身不带多模态（音乐/图像/语音）能力。** 这个插件不会给 Claude Code 它原本没有的新能力，它给的是结构——定义好的、可重复的处理常见开发任务的方式。所以"全部模态"必须靠你自建的 MCP 服务器 + 模型路由网关来提供，Superpowers 负责**编排和纪律**这一层。

2. **正确的分层架构**应该是：Superpowers（方法论/纪律层）→ 你的 orchestrator 网关（路由/级联/MoA，上一轮设计的那个）→ 各模态 MCP 工具 + 多模型 provider。Superpowers 通过它的 subagent 机制天然支持"不同任务派给不同模型"。

3. **它本身是给"真实工程任务"用的**，不适合快速修复、探索性原型或快速实验等结构会限制创造力的场景。所以你的多模态闲聊/快速生成走网关直连，重型开发任务才走 Superpowers 全流程。

---

## 整体方案：三层架构

```
┌─────────────────────────────────────────────────────────────┐
│  L1  Superpowers (方法论/纪律层 — Claude Code 插件)            │
│      brainstorm → plan → subagent-dev → TDD → review → finish │
│      + 你自定义的 SKILL.md（路由策略、多模态调度、记忆）       │
└───────────────────────────┬─────────────────────────────────┘
                            │ 子智能体调用 / MCP 工具调用
┌───────────────────────────▼─────────────────────────────────┐
│  L2  Orchestrator Gateway (上一轮设计的 OpenAI 兼容网关)       │
│      Router / Cascade / MoA / Debate / Specialist-Handoff     │
└───────────────────────────┬─────────────────────────────────┘
            ┌───────────────┼───────────────┬─────────────────┐
┌───────────▼──┐  ┌─────────▼──┐  ┌─────────▼──┐  ┌──────────▼──┐
│ 文本/代码 LLM │  │ 视觉模型    │  │ 语音 STT/TTS│  │ 音乐生成    │
│ Claude/GPT/  │  │ Qwen-VL/    │  │ Whisper/    │  │ Suno/       │
│ DeepSeek/GLM │  │ Gemini-VL   │  │ ElevenLabs  │  │ MusicGen    │
└──────────────┘  └────────────┘  └────────────┘  └─────────────┘
   (以上全部封装为 MCP servers，供 Superpowers 子智能体调用)
```

你需要自己开发的部分：① 上一轮那个 L2 网关；② 把各模态封装成 **MCP server**；③ 在 Superpowers 里**写几个自定义 SKILL.md** 教 Claude 何时调用哪个模态/模型。

---

## 给 Superpowers 的完整开发提示词

下面这套提示词分两部分：**(A) 启动指令**（粘进 Claude Code），**(B) 自定义 SKILL.md 文件**（让 Superpowers 学会你的多模态多模型编排）。

### 第一步：安装

```bash
# Claude Code 2.0.13+
/plugin marketplace add obra/superpowers-marketplace
/plugin install superpowers@claude-plugins-official
# 重启 Claude Code，确认看到 SessionStart hook 成功
```

### (A) 启动提示词（粘进 Claude Code 新会话）

````markdown
/using-superpowers

我要构建一个本地「全模态多模型编排工作台」(OmniAgent Workbench)。请用 Superpowers 的
完整方法论 (brainstorm → write-plan → subagent-driven-development → TDD → review) 来开发。

## 项目目标
构建一个本地系统，让 Claude Code 通过 MCP 工具调用一组异构模型/服务，覆盖全部模态，
并用「路由/级联/混合智能体(MoA)/辩论投票/专家交接」策略，使组合效果超越单一顶级模型。

## 必须覆盖的能力
- 文本/代码：Claude、GPT、DeepSeek、GLM、Qwen 等（通过 L2 网关统一接入）
- 视觉理解+生成：Qwen-VL / Gemini-Vision / 图像生成模型
- 语音：STT (Whisper) + TTS (ElevenLabs/本地)
- 音乐生成：Suno / MusicGen
- 长上下文 + 记忆：向量记忆库 (SQLite + embeddings)，跨会话持久

## 架构约束（重要）
1. 我已有/将有一个 L2「OmniGateway」服务：OpenAI 兼容端点 (http://localhost:8088/v1)，
   内部做 router/cascade/MoA/debate。所有文本类模型调用都走它。
2. 每种模态封装为独立 MCP server，Claude Code 通过 MCP 调用。
3. 利用 Superpowers 的 subagent 机制做「能力感知任务分配」：
   - 规划/架构/审查 → 用强模型 (Claude Opus 槽位)
   - 实现子任务 → 用便宜快模型 (DeepSeek/GLM 槽位)
   - 多模态子任务 → 派给对应 MCP server
4. 用 git worktree 隔离，每个子智能体干净上下文，防止上下文污染。

## 针对编程场景的特别要求（这是我的主用例）
请确保最终系统/技能在以下维度有明确机制，并在 plan 中体现为可测试的任务：
- 长上下文：子智能体只接收任务+相关文件，协调器维护全局 plan 作为状态
- 隐含规划能力：brainstorm 阶段必须产出显式 spec，禁止跳过
- 错误抑制：TDD 红绿重构强制；systematic-debugging 四阶段；连续3次修复失败触发架构复审
- 抽象推理：复杂任务用 MoA（多个 proposer 提方案 + 强模型 aggregator 综合）
- 指令遵循：每个 task 有精确文件路径、命令、验收步骤；review 检查 spec 合规
- 上下文整合：记忆 MCP 在任务开始前检索相关历史决策，注入子智能体上下文

## 第一阶段范围（先做这个）
不要一次做全部。先 brainstorm 出整体 spec，然后只规划+实现：
(1) OmniGateway 的 router + cascade + failover；
(2) 一个「记忆 MCP server」(SQLite + 向量检索 + Haiku 摘要)；
(3) 一个自定义 Superpowers skill：model-routing-policy（教 Claude 何时用哪个模型/模态）。

现在开始 brainstorming，先问我关键问题，不要直接写代码。
````

### (B) 自定义 SKILL.md（让 Superpowers 学会你的编排逻辑）

把下面文件放到 `~/.claude/skills/model-orchestration/SKILL.md`（或项目 `skills/` 目录）。结构遵循 Superpowers 规范：每个技能是单个带 YAML frontmatter 和几百字说明的 SKILL.md。

````markdown
---
name: model-orchestration
description: >
  Decides WHICH model/modality to use for each subtask, and WHICH strategy
  (route/cascade/MoA/debate/handoff) to apply. Activates whenever a task could
  be served by a cheaper model, needs a non-text modality, or is high-risk.
when_to_use: >
  Use before dispatching any subagent task. Use when a task involves images,
  audio, music, or when reasoning is complex/high-risk enough to warrant
  multi-model aggregation.
---

# Model Orchestration Skill

## IRON LAW
Never default to the most expensive model. ALWAYS classify the task first,
then pick the cheapest strategy that meets the quality bar. Strong models are
reserved for PLANNING, AGGREGATION, and REVIEW — not for routine implementation.

## Step 1 — Classify the task
Output a JSON header before acting:
{ "modality": text|code|image|audio|music, "difficulty": easy|medium|hard,
  "risk": low|high }

## Step 2 — Pick strategy
- easy + low risk  → ROUTE to cheapest text model via OmniGateway (deepseek/glm)
- medium           → CASCADE: try cheap model, if its self-confidence < 0.7
                     or tests fail, escalate to strong model
- hard + abstract  → MoA: dispatch 3 cheap proposers in parallel (different
                     models), then a STRONG model aggregates their outputs
- high risk / facts→ DEBATE: 3 independent answers + strong judge model
- non-text modality→ HANDOFF to the matching MCP server:
                     image→vision-mcp, audio→speech-mcp, music→music-mcp

## Step 3 — Anti-shortlist (avoid the weak-link effect)
Proposers MUST pass a quality floor. Do NOT add more than 5 proposers.
More models ≠ better; a weak proposer drags down the aggregate. Prefer 3 strong-ish
diverse proposers + 1 strong aggregator over many weak ones.

## Step 4 — Memory integration
Before any hard task, call memory-mcp.search(task) and inject the top-3 relevant
past decisions into the subagent's context. After completion, call
memory-mcp.store(decision, rationale).

## Cost guard
Track cumulative token cost. If daily budget exceeded, downgrade all MEDIUM
tasks to ROUTE (cheapest) and warn the user.

## Verification
This skill is satisfied only when: every dispatched subtask logged its chosen
{model, strategy, cost}; and the /stats endpoint reflects the routing decisions.
````

类似地再写 `memory/SKILL.md`（教 Claude 用记忆 MCP，模仿 Superpowers 作者自己的记忆方案）。值得注意，Superpowers 作者的记忆方案是把转录存进 SQLite 向量索引，用 Claude Haiku 生成每段对话摘要，并提供命令行工具搜索历史记忆——而且强调必须用子智能体来搜索，避免无用搜索污染上下文窗口。你可以直接复用这个设计思路。

---

## 各模态 MCP server 开发提示词（给子智能体）

````markdown
请用 Superpowers 的 write-plan + TDD 流程，为以下每个模态实现一个 MCP server。
统一规范：Python + mcp SDK，每个 server 暴露明确工具，写失败测试先行。

1. vision-mcp:   工具 analyze_image(path|url) / generate_image(prompt)
                 后端可配 Qwen-VL / Gemini-Vision / 图像生成 API
2. speech-mcp:   工具 transcribe(audio_path) [Whisper] / synthesize(text) [TTS]
3. music-mcp:    工具 compose(prompt, duration) → 调 Suno/MusicGen，返回音频文件路径
4. memory-mcp:   工具 search(query)→top-k / store(text,meta)；
                 SQLite + sentence-transformers 向量；新条目用便宜模型生成摘要
5. gateway-mcp(可选): 把 OmniGateway 暴露为工具 route(prompt, strategy)，
                 让 Claude 显式触发 MoA/debate

每个 server 的验收：有 README、有 mcp.json 配置片段、有 pytest 测试、
能在 Claude Code 的 .mcp.json 中注册并被调用。
````

---

## 开发与实现注意事项（针对你的编程主用例）

下面这些是用 Superpowers + 多模型时最容易踩的坑，按你点名的维度组织：

<details>
<summary><b>长上下文 / 超大上下文（点开）</b></summary>

- **不要把所有东西塞进一个会话。** Superpowers 的核心就是反这个——它不在单一上下文中执行所有任务（那会导致长项目漂移），而是为每个任务启动新子智能体，每个子智能体经历两阶段审查，使 Claude 能自主工作数小时不偏离计划。
- **plan 文档就是你的"外部记忆"和恢复机制。** 计划文档是会话死亡后的恢复机制，每步有复选框，标记它们是状态日志而非形式；任务部分完成时会话中断，计划里未勾选的复选框是下个会话知道从哪恢复的唯一方式。
- 真正的超长上下文需求（整库分析）走子智能体并行检索，而非单会话硬塞。

</details>

<details>
<summary><b>隐含规划能力 / 指令遵循（点开）</b></summary>

- **让 brainstorm 阶段强制产出 spec，别手痒跳过。** 头脑风暴阶段是强制的，因为它在实现开始前解决重大架构决策。
- **plan gate 是硬门槛。** 计划门是硬的，跳过它子智能体就会漂移。每个任务必须有精确文件路径、命令、完整代码、验收步骤——这本身就是最强的"指令遵循"保障。

</details>

<details>
<summary><b>错误抑制 / 抽象推理（点开）</b></summary>

- **TDD 是物理强制，不是建议。** Superpowers 技能是带强制的流程——TDD 技能不是建议先写测试，而是用铁律强制，包含删除测试前所写代码的指令；这是限速标志 vs 减速带的区别，一个告知，另一个物理阻止。这是抑制 AI"幻觉式通过"的关键。
- **调试要追根因。** 系统化方法引导你经历根因调查、模式分析、假设检验、实现，并在三次修复失败后触发架构审查的安全机制。把这个三次失败阈值接到你的 MoA/debate——失败 3 次就升级到多模型方案。
- 抽象推理类的硬任务用 MoA（多 proposer + 强 aggregator），这是上一轮论证过的、能超越单模型的机制。

</details>

<details>
<summary><b>上下文整合 / 防漂移（点开）</b></summary>

- 子智能体**只给任务+相关上下文，不给全历史**——每个子智能体只接收任务描述和相关上下文，而非完整对话历史，防止上下文污染（累积上下文降低判断力）。
- 记忆 MCP 在任务开始前检索 top-k 相关历史决策注入，做"上下文整合"，但要用子智能体执行检索以免污染主上下文。

</details>

<details>
<summary><b>多模型/多模态特有的坑（点开）</b></summary>

- **强模型只做规划/聚合/审查，弱模型做执行**——这是官方推荐且省钱的核心。
- **别堆太多弱模型**（上一轮的 Self-MoA 反例）：proposer ≤ 5 且要过质量门槛。
- **交互式延迟**：cascade/MoA/debate 会增加延迟，只对重型开发任务开启，闲聊/快速生成走网关直连。
- **跨宿主一致性**：若你也想在 Codex/Cursor 用，注意 Superpowers 的技能更新必须能跨所有支持的编程智能体工作。你的自定义 skill 也尽量写成宿主无关的纯 markdown。
- **认知开销是真实的**：认知开销是真实的——管理头脑风暴阶段、详细规划和两阶段审查需要心力，但这种投入产出带测试覆盖、合理架构和系统化验证的生产级代码，对要交付给用户的工作而言值得。

</details>

---

## 一句话总结

**Superpowers 是"纪律+编排"层，不是模型层。** 把它当大脑的"前额叶"——负责规划、派活、审查、防漂移；把你上一轮设计的 OmniGateway + 各模态 MCP server 当"小脑和感官"——负责具体的多模型多模态执行。两者通过 Superpowers 的 subagent 机制 + 你自写的 `model-orchestration` SKILL.md 衔接，就能实现"强模型规划、弱模型执行、按模态分流、复杂任务 MoA 聚合"的完整闭环，达到甚至超越单一顶级模型的效果。

需要我现在把 **OmniGateway 的第一阶段代码**、或 **memory-mcp / vision-mcp 的完整可运行实现**直接写出来吗？我可以按 Superpowers 的 TDD 格式（先失败测试、再实现）给你完整文件。

**User - --**

我要是是一个最好的一个开源案例，作为底座持续开发，另外组合的原理是必须判断组合的结果是否是正确，那么多个弱的模型如何判断自己回答可信性，如果可信比较低，就重新生成呢，
另一个问题就是，AI如何进行科学研究，如何进行。

**Assistant - --**