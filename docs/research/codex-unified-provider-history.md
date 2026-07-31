# Codex 统一 Provider 历史方案调研

调研日期：2026-07-22

## 结论摘要

GitHub 上不但有现成方案，`farion1231/cc-switch` 上游从 v3.16 起就已经实现了用户设想的产品形态：

1. 所有 CC Switch 管理的第三方 Codex Provider 共用稳定的运行时键 `custom`，切换渠道时只替换 `[model_providers.custom]` 的路由内容，不再为 PinAI、My Codex 等生成不同的 `model_provider`。
2. 可选开启“统一 Codex 会话历史”，让官方 ChatGPT/OpenAI 路由也以 `custom` 作为会话桶；官方认证仍由 `requires_openai_auth = true` 使用 Codex 的 `auth.json`。
3. 可选把既有 `openai` 会话迁移成 `custom`；迁移同时修改 rollout JSONL 与 `state_5.sqlite`，修改前备份，关闭功能时按备份账本精确还原。

这正是“个人运行环境里不区分渠道，把线程统一到一个 Provider 标签”的成熟实现。

> **2026-07-23 修订**：本分支已实现该推荐。Managed third-party（以及启用统一历史后的官方）Provider 在每个 Target 上归一为稳定 `custom` 路由（见 `normalize_codex_managed_provider_config` 与 ADR-0002）；按渠道变化的 `cc_switch_pinai_<id>` 运行时键不再用于新投影。下文中“当前分支偏离稳定桶”的描述仅反映调研当时状态，不再代表实现。

推荐（现已落地）扩展上游设计，而不是重新发明一套会话系统：每个 Windows/WSL Target 都固定使用 `custom` 作为 CC Switch 托管的会话桶，真实渠道身份只保存在 CC Switch 自己的 Provider/Target 状态中。

## 先区分两个问题

### 历史可见性

Codex 的线程记录包含 `model_provider`；当前 app-server 的线程列表路径会按 Provider 过滤。OpenAI Codex 当前源码中，`modelProviders: []` 会取消过滤，而省略/null 在普通列表路径仍会被转换成当前 `model_provider` 过滤器（关系查询除外）。这就是切换 Provider 后历史“消失”的直接原因。[Codex 当前源码（固定提交）](https://github.com/openai/codex/blob/65ae4c26e088913176a50d6daeb742d00942caee/codex-rs/app-server/src/request_processors/thread_processor.rs#L4358-L4368)；[上游问题 #24648](https://github.com/openai/codex/issues/24648)

因此，“列表里都看得见”可以有两类解法：

- 不改历史，调用列表 API 时明确传 `modelProviders: []`。
- 把所有历史的 `model_provider` 统一成同一个稳定键。

OpenAI 当前 Codex 手册也把 `model_provider` 定义为用户级路由选择，并明确说明：若只是让内建 OpenAI Provider 经过代理/路由器，可以使用 `openai_base_url`，不必创建新的 Provider；自定义 Provider 则需要独立的非保留 ID。这说明“实际端点”与“会话采用哪个 Provider ID”本来就是两个可以分离的概念。[Codex 配置手册](https://developers.openai.com/codex/config-advanced#custom-model-providers)

### 跨 Provider 继续会话

“看得见”不等于“能安全续聊”。OpenAI 维护者明确说明，不同 Provider/模型之间通常不能共享同一线程，历史中包含无法跨模型/后端转移的 CoT/推理数据，建议一条线程固定一个模型/Provider。[openai/codex #9054 维护者答复](https://github.com/openai/codex/issues/9054#issuecomment-3734992454)；[后续答复](https://github.com/openai/codex/issues/9054#issuecomment-3739440607)

OpenAI 随后合并了 PR #19287：`thread/resume` 默认恢复线程持久化的原始 `model_provider`，避免把带 `encrypted_content` 的旧历史发送给错误端点；只有调用者明确覆盖 model/provider/reasoning 时才不采用持久化值。[PR #19287](https://github.com/openai/codex/pull/19287)

这意味着：

- 保留每个渠道独立 Provider key 时，现代 Codex 倾向于把旧线程送回原 Provider。
- 把所有渠道统一为 `custom` 时，Codex 只知道线程属于 `custom`，而 `[model_providers.custom]` 此刻可能已经指向另一个后端。列表统一了，但跨后端续聊仍可能因 `encrypted_content` 失败。
- 对个人环境而言，这个取舍可以接受，但产品必须明确告知：统一的是“历史归类标签”，不是对旧推理密文做跨后端转换。

## 现成方案一：CC Switch 上游稳定 `custom` 桶

这是与本项目最接近、也最完整的现成实现。

### 第三方渠道统一

上游提交 [`b44f83f7`](https://github.com/farion1231/cc-switch/commit/b44f83f7) 将所有第三方 Codex Provider 的运行时键统一为 `custom`。提交说明直接指出，Codex 按 `model_provider` 过滤 resume 历史，`rightcode`、`aihubmix` 等 Provider 专属键会让旧历史看似消失；实现包含：

- 所有第三方 live 配置统一写 `model_provider = "custom"`。
- 将原 Provider 表重命名/投影为 `[model_providers.custom]`，保留其 `base_url`、认证和模型字段。
- 一次性迁移历史 JSONL 与 `state_5.sqlite.threads.model_provider`。
- 迁移前把原文件备份到 `~/.cc-switch/backups/codex-history-provider-migration-v1/`，用 SQLite Backup API 备份状态库。
- 在 `settings.json` 记录本地迁移完成标记，保证幂等。

后续提交继续补全旧键迁移与模板一致性：[`b15d9dfa`](https://github.com/farion1231/cc-switch/commit/b15d9dfa)、[`fc0433f2`](https://github.com/farion1231/cc-switch/commit/fc0433f2)。

当前仓库仍保留该核心常量：`CC_SWITCH_CODEX_MODEL_PROVIDER_ID = "custom"`，并把 `openai`、`ollama` 等内建键视为保留键。[本仓库 codex_config.rs](../../src-tauri/src/codex_config.rs)

### 官方与第三方也统一

上游随后加入可选的“统一 Codex 会话历史”：[`948d7627`](https://github.com/farion1231/cc-switch/commit/948d7627)、[`eab6bfd2`](https://github.com/farion1231/cc-switch/commit/eab6bfd2)。对应版本从 v3.16.3 起包含完整迁移/还原流程。

官方 live 配置被临时投影为：

```toml
model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
requires_openai_auth = true
supports_websockets = true
wire_api = "responses"
```

它不写第三方 `base_url`，所以仍走 Codex 官方后端；`requires_openai_auth = true` 继续使用 ChatGPT 登录。注入只存在于 live `config.toml`，不会污染数据库里保存的官方 Provider 模板。若用户已有显式 `model_provider`，或已有形态不同的 `[model_providers.custom]`，上游会拒绝注入，避免把官方 OAuth 流量错误发送给第三方端点。[实现提交](https://github.com/farion1231/cc-switch/commit/948d7627)

既有官方历史迁移是显式 opt-in：

- 只把 `openai` 改为 `custom`。
- 同时覆盖 `sessions`、`archived_sessions` 与 `state_5.sqlite`。
- JSONL 使用临时文件/原子替换；SQLite 使用备份与事务。
- 备份代际记录来源 Codex 目录，关闭功能时只还原“备份账本中原为 openai 且当前仍为 custom”的线程。
- 开启统一期间新建的 `custom` 会话无法再区分官方或第三方，因此不会在关闭时自动猜测来源。

上游已有三语使用说明，明确列出可见性、备份、还原与 `encrypted_content` 风险。[中文指南](https://github.com/farion1231/cc-switch/blob/main/docs/guides/codex-unified-session-history-guide-zh.md)

## 现成方案二：只统一列表，不修改历史

OpenAI app-server 的 `thread/list` 支持 `modelProviders` 过滤；显式空数组可以列出全部 Provider。社区为此准备过跨 Provider discovery 补丁，并在 [openai/codex #15494](https://github.com/openai/codex/issues/15494) 中说明，相比修改 rollout/SQLite，修正 discovery 更小、更安全。

[`codexresume`](https://github.com/daquexian/codexresume) 是可直接使用的实现：它读取 `config.toml`、`session_index.jsonl` 和 `state_*.sqlite`，默认展示所有 Provider 的本地会话，选择后仍执行原生 `codex resume <SESSION_ID>`。它不改历史元数据。

优点：

- 不改 JSONL/SQLite，没有迁移与还原风险。
- 能保留每条线程真实的原 Provider 身份。

局限：

- 只解决自定义 CLI picker；不能直接修复 Codex Desktop/VS Code 自己的列表 UI，除非客户端改为传 `modelProviders: []`。
- 现代 Codex resume 会恢复线程原 Provider，因此不满足“切换后让旧会话走当前中转”的目标。

## 现成方案三：把全部历史同步到当前 Provider

[`Dailin521/codex-provider-sync`](https://github.com/Dailin521/codex-provider-sync) 是独立 GUI/CLI 产品，目标就是在切换 Provider 后让旧会话重新可见。它会同步：

- `~/.codex/sessions`
- `~/.codex/archived_sessions`
- `~/.codex/state_5.sqlite`
- `.codex-global-state.json` 的项目根路径缓存

工具提供 `status`、`sync`、`switch <provider-id>` 与 `restore`；执行前备份配置、会话首行、SQLite/WAL/SHM 与 global state，并检测 SQLite/rollout 文件锁。[实现说明](https://github.com/Dailin521/codex-provider-sync#readme)

它与上游 CC Switch 的差别是：`codex-provider-sync` 可以把全部历史同步到任意“当前 Provider ID”，而 CC Switch 上游选择永远稳定的 `custom`，避免每次切换都重写一遍历史。

该工具也明确承认边界：只修复可见性元数据，不重加密 `encrypted_content`；跨 Provider/account 后继续或 compact 仍可能报 `invalid_encrypted_content`。

## 方案比较

| 方案 | 是否改历史 | 原生 Desktop/VS Code 可见 | 切换后旧线程可能走当前渠道 | 保留原渠道身份 | 主要风险 |
|---|---:|---:|---:|---:|---|
| 稳定 `custom` 桶（上游 CC Switch） | 一次性迁移 | 是 | 是，因为所有线程只记录 `custom` | 否，需 CC Switch 另存 | 跨后端密文失败；来源不可逆推断 |
| `modelProviders: []` / codexresume | 否 | 客户端配合时才是 | 否，默认恢复原 Provider | 是 | 原生客户端仍可能过滤 |
| codex-provider-sync 到当前键 | 每次同步 | 是 | 是 | 否，除非另存账本 | 高频改写、锁/备份复杂度、密文失败 |

## 对本项目的推荐

### 1. 恢复稳定运行时键

不要用 `cc_switch_pinai_<uuid>`、`cc_switch_mycodex_<uuid>` 作为 Codex 的 `model_provider`。这些名字适合 CC Switch 内部 Provider ID，不适合作为 Codex 会话桶。

对每个托管 Target 固定：

```toml
model_provider = "custom"
```

Provider 切换只替换 `[model_providers.custom]` 的具体路由配置。CC Switch UI 继续显示“PinAI”“My Codex”，但 Codex 只看到稳定的 `custom`。

### 2. 直接复用上游“统一历史”开关

当前仓库已经包含 `unify_codex_session_history`、官方 `custom` 注入、存量迁移、备份账本和精确还原代码。短期版本应优先让这些既有能力适配 Managed Target，而不是新增另一套迁移器。

Windows 与 WSL2 必须各自作为独立迁移域：

- 迁移函数接收明确的 `CodexTargetContext/config_dir`。
- 每个 Target 单独记录 migration marker 和 backup generation。
- 只扫描该 Target 的 `sessions`、`archived_sessions`、有效 `state_5.sqlite`。
- 不能因切换 WSL Target 去重写 Windows 历史，反之亦然。

### 3. 兼容当前分支已经生成的键

增加一次性的 Target-aware 迁移，把这些来源统一到 `custom`：

- 上游已知旧第三方键。
- 当前分支生成的 `cc_switch_<slug>_<id>` 键。
- 用户明确选择迁移的 `openai`。

迁移应沿用上游安全属性：预检查 live 确实路由到 `custom`、备份 JSONL/SQLite、原子写、SQLite 事务、目录绑定 marker、可重复执行。

### 4. 不把“统一可见”宣传成“无损跨渠道续聊”

推荐 UI 文案明确分两层：

- “所有会话将显示在同一个历史列表中。”
- “旧会话包含由原后端生成的加密推理内容；切换渠道后继续可能失败。失败时请切回原渠道或新建会话。”

对于个人环境，可以默认建议统一，但既有 `openai` 历史仍应由用户显式确认迁移。发布给其他用户时不应静默重写官方历史。

### 5. 另存真实来源，仅用于诊断

统一成 `custom` 后，Codex 历史本身不再能区分 PinAI/My Codex/OpenAI。CC Switch 可以在自己的只读 provenance 账本中记录：

```text
(target_id, session_id) -> provider_id_at_creation
```

这份账本用于 UI 标注和切回原渠道建议，不应再反向修改 Codex 对话正文。无法可靠判断的存量 `custom` 会话显示“来源未知”，不要猜测。

## 最终判断

“所有线程统一成一个 Provider”不是异想天开，而是上游 CC Switch 已经采用并产品化的方案。对当前项目最合理的方向是回到上游的稳定 `custom` 会话桶，再把它扩展为 Windows/WSL Target-aware；不要继续使用随实际渠道变化的 `cc_switch_*` 运行时键。

它能可靠解决历史列表割裂，但不能突破 Codex 的跨后端密文限制。产品上应把“统一历史可见性”和“跨 Provider 无损续聊”明确区分。
