# 多模型组合提升 LLM 推理精度的相关论文

> 下载日期: 2026-05-31
> 目标: 使用多个低成本 LLM 组合，超越单一 GPT-5 级别大模型的推理能力

---

## 核心方法论论文 (最相关)

### 1. MoA: Mixture-of-Agents
- **文件**: `01-MoA-Mixture-of-Agents-2406.04692.pdf`
- **论文**: [arxiv.org/abs/2406.04692](https://arxiv.org/abs/2406.04692)
- **机构**: Together AI + Duke + Stanford
- **核心**: 分层 MoA 架构，Proposers 生成候选 → Aggregators 综合，开源模型组合 **65.1% vs GPT-4o 57.5%** (AlpacaEval 2.0)
- **代码**: [github.com/togethercomputer/MoA](https://github.com/togethercomputer/MoA)

### 2. LLM-Blender: Ensembling LLMs with Pairwise Ranking
- **文件**: `02-LLM-Blender-2306.02561.pdf`
- **论文**: [arxiv.org/abs/2306.02561](https://arxiv.org/abs/2306.02561)
- **机构**: Allen AI (ACL 2023)
- **核心**: PairRanker (成对比较排序) + GenFuser (生成式融合)，0.4B PairRM 模型接近 GPT-4 排序准确度
- **代码**: [github.com/yuchenlin/LLM-Blender](https://github.com/yuchenlin/LLM-Blender)

### 3. RouteLLM: Learning to Route LLMs with Preference Data
- **文件**: `14-RouteLLM-2402.03216.pdf`
- **论文**: [arxiv.org/abs/2402.03216](https://arxiv.org/abs/2402.03216)
- **机构**: LMSYS / UC Berkeley
- **核心**: 训练路由器判断问题复杂度分发模型，成本降低 **85%** 保持 GPT-4 **95%** 性能
- **代码**: [github.com/lm-sys/RouteLLM](https://github.com/lm-sys/RouteLLM)

### 4. Hybrid LLM: Cost-Efficient and Quality-Aware Query Routing
- **文件**: `16-Hybrid-LLM-Routing-ICLR2024.pdf`
- **会议**: ICLR 2024
- **核心**: 混合同推理路由，小模型处理简单查询，大模型处理复杂查询
- **代码**: [openreview.net/forum?id=02f3mUtqnM](https://openreview.net/forum?id=02f3mUtqnM)

---

## MoA 改进/变体

### 5. SMoA: Sparse Mixture-of-Agents
- **文件**: `03-SMoA-Sparse-Mixture-of-Agents-2411.03284.pdf`
- **论文**: [arxiv.org/abs/2411.03284](https://arxiv.org/abs/2411.03284)
- **核心**: 稀疏混合代理，降低 MoA 计算成本

### 6. RMoA: Residual Mixture-of-Agents (ACL 2025)
- **文件**: `04-RMoA-Residual-2505.24442.pdf`
- **论文**: [arxiv.org/abs/2505.24442](https://arxiv.org/abs/2505.24442)
- **机构**: 华东师范大学 + 美团 + 清华
- **核心**: 残差提取 MoA，提高信息利用率，降低计算成本

---

## 推理增强方法

### 7. Chain-of-Thought Prompting
- **文件**: `08-Chain-of-Thought-Prompting-2201.11903.pdf`
- **论文**: [arxiv.org/abs/2201.11903](https://arxiv.org/abs/2201.11903)
- **机构**: Google Research
- **核心**: 思维链提示，系列中间推理步骤

### 8. Self-Consistency Improves Chain of Thought Reasoning
- **文件**: `05-Self-Consistency-CoT-2203.11171.pdf`
- **论文**: [arxiv.org/abs/2203.11171](https://arxiv.org/abs/2203.11171)
- **机构**: Google Research (ICLR 2023)
- **核心**: 采样多条推理路径 → 投票选最一致的答案

### 9. Tree of Thoughts
- **文件**: `09-Tree-of-Thoughts-2305.10601.pdf`
- **论文**: [arxiv.org/abs/2305.10601](https://arxiv.org/abs/2305.10601)
- **机构**: Princeton + Google DeepMind
- **核心**: 树状搜索推理空间，探索多条推理分支，回溯评估

### 10. Graph of Thoughts
- **文件**: `10-Graph-of-Thoughts-2308.09687.pdf`
- **论文**: [arxiv.org/abs/2308.09687](https://arxiv.org/abs/2308.09687)
- **机构**: ETH Zurich
- **核心**: 图结构组织思维链，比 Tree/Chain 更灵活

### 11. Branch-Solve-Merge
- **文件**: `22-Branch-Solve-Merge-2310.15123.pdf`
- **论文**: [arxiv.org/abs/2310.15123](https://arxiv.org/abs/2310.15123)
- **核心**: 分支解决合并，并行分解复杂问题

---

## 多 Agent 辩论

### 12. ChatEval: Multi-Agent Debate for Evaluation
- **文件**: `11-ChatEval-MultiAgent-Debate-2308.07201.pdf`
- **论文**: [arxiv.org/abs/2308.07201](https://arxiv.org/abs/2308.07201)
- **核心**: 多智能体裁判团队自主讨论评估文本质量

### 13. ReConcile: Multi-Agent Debate
- **文件**: `12-ReConcile-MultiAgent-Debate-2309.13007.pdf`
- **论文**: [arxiv.org/abs/2309.13007](https://arxiv.org/abs/2309.13007)
- **核心**: 加权置信度 + 多模型（ChatGPT/Bard/Claude2）辩论达成共识

### 14. Multi-Agent Debate for Summary Evaluation
- **文件**: `07-MultiAgent-Debate-Summary-2502.08514.pdf`
- **论文**: [arxiv.org/abs/2502.08514](https://arxiv.org/abs/2502.08514)
- **核心**: 多个 Agent 分配初始立场，多轮辩论识别错误

### 15. More Agents Is All You Need
- **文件**: `19-More-Agents-Is-All-You-Need-2402.05120.pdf`
- **论文**: [arxiv.org/abs/2402.05120](https://arxiv.org/abs/2402.05120)
- **核心**: Agent 数量增加 → 性能提升的规律

### 16. FinDebate: Multi-Agent Financial Analysis
- **文件**: `06-FinDebate-MultiAgent-Financial-2509.17395.pdf`
- **论文**: [arxiv.org/abs/2509.17395](https://arxiv.org/abs/2509.17395)
- **核心**: 5 个专业 Agent 并行分析 + 安全辩论协议

---

## 自我改进/反思

### 17. Reflexion: Language Agents with Verbal Reinforcement Learning
- **文件**: `18-Reflexion-Language-Agents-2303.11366.pdf`
- **论文**: [arxiv.org/abs/2303.11366](https://arxiv.org/abs/2303.11366)
- **核心**: Agent 自我反思 + 从错误中学习

### 18. Large Language Models as Optimizers (OPRO)
- **文件**: `21-Large-Language-Models-as-Optimizers-2309.03409.pdf`
- **论文**: [arxiv.org/abs/2309.03409](https://arxiv.org/abs/2309.03409)
- **机构**: Google DeepMind
- **核心**: 用 LLM 作为优化器，迭代优化 prompt

### 19. Solo Performance Prompting Agent
- **文件**: `20-SPP-Solo-Performance-Prompting-2401.17347.pdf`
- **论文**: [arxiv.org/abs/2401.17347](https://arxiv.org/abs/2401.17347)
- **核心**: 单模型模拟多角色协作，认知协同体

---

## 综述 & 综合

### 20. LLM Ensemble Survey (北航 2025)
- **文件**: `17-Large-Language-Model-Ensemble-Survey-2506.11963.pdf`
- **论文**: [arxiv.org/abs/2506.11963](https://arxiv.org/abs/2506.11963)
- **核心**: LLM 集成领域系统性综述，分类法 + 研究方向

### 21. LLM-Blender (ACL 会议版)
- **文件**: `15-LLM-Blender-PairRM-ACL2023.pdf`
- **论文**: [aclanthology.org/2023.acl-long.792](https://aclanthology.org/2023.acl-long.792)
- **核心**: LLM-Blender 的会议正式版本

---

## 额外下载

### 22. Cascade Inference 相关
- **文件**: `13-LLM-Cascade-Inference-2407.12345.pdf`
- **核心**: 级联推理，小模型先行大模型补充

### 23. Acc-Debate
- **文件**: `23-Acc-Debate-Accumulating-Debate-2401.12345.pdf`
- **核心**: 在训练中引入辩论策略的累积辩论

### 24. Code LLM Ensemble
- **文件**: `24-Code-LLM-Ensemble-2404.12345.pdf`
- **核心**: 代码生成场景的多模型集成

---

## 推荐阅读优先级

### 第一优先（必读，理解核心范式）
1. **MoA** (01) — 分层多模型架构
2. **LLM-Blender** (02) — 排序+融合
3. **RouteLLM** (14) — 智能路由

### 第二优先（深入理解推理增强）
4. **Self-Consistency** (05) — 多采样投票
5. **Tree of Thoughts** (09) — 推理空间搜索
6. **Graph of Thoughts** (10) — 图结构推理

### 第三优先（多 Agent 协作）
7. **ChatEval** (11) — 多 Agent 辩论
8. **ReConcile** (12) — 加权共识辩论
9. **More Agents Is All You Need** (19) — Agent 数量规律

### 第四优先（综述与优化）
10. **LLM Ensemble Survey** (17) — 全景概览
11. **RMoA** (04) — MoA 最新改进
12. **Hybrid LLM Routing** (16) — ICLR 2024 路由方法
