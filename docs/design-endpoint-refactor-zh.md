# 端点引用化重构：厂商连接信息如何全局共享、条目只留策略与映射

> 读者：cc-switch 维护者。本文回答三个问题：为什么要把 Provider 里的连接信息抽成全局端点层；引用化之后 49 处消费代码为什么不炸；存量数据怎么迁。读完应能据此 1:1 开工。

## 一、解决什么问题

现状每条 Provider 是"胖对象"：连接信息（base_url、api_key）与策略（队列、排序）、映射（档位模型）揉在同一条 `settings_config.env` 里。同一个中转站要在 claude/codex/gemini 多个分组使用，就得各建一条，密钥改一次要改多处。档位模型（haiku/sonnet/opus/fable/subagent/default）也被锁死在同一条目内，无法让"主对话走 A 家、标题生成走 B 家"这类跨厂商组合。

目标：三层模型——**端点**（连接信息，全局一处）、**队列条目**（分组内引用端点 + 路由/负载策略）、**档位映射**（条目内每档指向任意端点的任意模型）。

## 二、现状锚点（源码级）

| 事实 | 位置 |
|---|---|
| providers 表：PK(id, app_type)，settings_config 为 JSON blob（env 含凭据+六档模型） | `database/schema.rs:27-45` |
| 六档模型全部从单条 provider 的 env 读取 | `proxy/model_mapper.rs:21-56` |
| 请求流程先选池（队列/粘性）、后按候选逐家做模型映射；档位从不参与选家 | `proxy/handler_context.rs:194`、`proxy/forwarder.rs:1585` |
| 分组级策略已存在：failover 开关、负载模式在 proxy_config；队列顺序在 in_failover_queue + sort_index | `database/dao/failover.rs:23-49` |
| UniversalProvider 已具端点雏形：baseUrl/apiKey/模型配置/图标备注，但 apps 硬编码仅 claude/codex/gemini，存 settings 表 JSON blob | `provider.rs:744-845`、`database/dao/universal_providers.rs:9` |
| sync_universal_to_apps：一处编辑快照复制到各分组子条目 | `services/provider/mod.rs:8391` |
| provider_endpoints 表是 per-provider 备用 URL 列表，非全局端点 | `database/schema.rs:50-58` |
| settings_config 全库消费面 49 文件（services/provider 15、proxy 8、各 vendor 写盘模块约 8、database 5、commands 4、mode/tray/deeplink 等） | grep 统计 |
| 迁移框架：SCHEMA_VERSION=24 递增式 + 迁移前自动备份 | `database/mod.rs:54,133` |
| Hermes 已是角色引用制先例（每角色独立 {provider, model}） | `live/project/hermes.rs:65` |

## 三、目标数据模型

### 端点表（新，独立表）

```sql
CREATE TABLE endpoints (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    category TEXT,              -- 厂商类型（custom/newapi/github_copilot…）
    base_url TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    auth_binding TEXT,          -- OAuth 等托管凭据（复用 ProviderMeta.auth_binding 结构）
    website_url TEXT, icon TEXT, icon_color TEXT, notes TEXT,
    meta TEXT NOT NULL DEFAULT '{}',   -- 含模型清单缓存（fetchModels 结果）
    created_at INTEGER, sort_index INTEGER
);
```

### providers 表（加列，不删列）

```sql
ALTER TABLE providers ADD COLUMN endpoint_ref TEXT;      -- NULL = 独立条目（存量兼容）
ALTER TABLE providers ADD COLUMN tier_mappings TEXT;     -- JSON，见下
```

tier_mappings 形态：

```json
{
  "haiku":    { "endpoint": "ep-cheap", "model": "gemini-3-flash" },
  "subagent": { "endpoint": "ep-openai", "model": "gpt-5.4-mini" },
  "opus":     { "model": "kimi-k2.5" }
}
```

- 无 `endpoint` 键 = 仅改模型名（现有行为，向后兼容）
- 未列出的档位 = 继承自身端点默认行为
- 带 `endpoint` 键 = 代理模式下该档分流到目标端点

策略不动：队列顺序、负载模式、熔断参数仍在现有位置（proxy_config + in_failover_queue/sort_index）。**引用化只动"连接信息"和"档位映射"两件事的归属。**

## 四、关键决策：为什么保留物化缓存层

引用化的第一直觉是"Provider 只存引用，运行时处处解析"。但 settings_config 有 49 个消费文件，proxy 每请求取凭据、9 个 vendor 写盘模块生成 CLI 配置、usage 脚本、tray、deeplink 全在读它。一次改完等于全库重写，风险不可控。

设计：**引用是 SSOT，settings_config 降级为派生缓存**。

- `save_provider` 时由 `endpoint_ref + tier_mappings` 物化出 settings_config（凭据展开进 env、无 endpoint 的档位写入对应 env 键）
- 带跨端点引用的档位**不写入 env**——CLI 配置文件单条目只有一个 base_url，直连模式物理上做不到跨家；跨端点只在代理运行时分流（见第六节）
- 全部现有读代码零改动；缓存可随时清除重算（诊断入口：强制重物化命令）

与现状 UniversalProvider sync 的本质区别：sync 是两条独立记录间的快照传播（可能漂移）；此处是同一条记录的派生字段（单一事实源）。这是方案 2 与方案 1 的分界线。

## 五、存量数据怎么迁

迁移号 SCHEMA_VERSION 25（`migrate_v25`），迁移前自动备份已内建（`database/mod.rs:133`）。

1. 建表：endpoints 独立表；UniversalProvider 全量并入（按 id 保留），apps 覆盖范围扩展到全部分组
2. 归并：存量 providers 按 `(解析后的 base_url, api_key)` **完全一致**才聚类生成 endpoint 并回填 endpoint_ref（保守线，不误并同站不同账号；激进的仅 baseUrl 归并留作后续可选）
3. 归并出的 endpoint：名称取首次出现的条目名，icon/notes 合并优先非空
4. 不匹配的条目 endpoint_ref = NULL，行为与今天完全一致（功能不回退）
5. tier_mappings 初始为空（等价现状），不猜不造

## 六、跨端点档位在代理里怎么走

请求进来 → `handler_context.rs:172` 已拿到 request_model → 复用 `map_model` 的档位识别（contains "haiku" 等）→ 查命中条目的 tier_mappings：

1. **命中且无 endpoint**：现行路径不变（forwarder 内改模型名）
2. **命中且有 endpoint**：构造影子 Provider（目标端点凭据 + 目标模型，走目标家的 transform 管线），候选池 = [影子 Provider] + [该档位在自身端点的展开形态]（回落序：目标家挂了退回自家模型，不摊给整个主队列——档位语义优先，可预期）
3. **未命中**：现行路径不变

配套语义：

- **熔断**：key 细分为 `{app}:{entry}:{tier}:{endpoint}`，B 家熔断不拖垮 A 家主档
- **usage 归因**：按实际服务的端点+模型记账（影子 Provider 携带目标端点 id，usage 现有按 provider 记账结构天然承接）——跨家组合后各档成本分账，正是"组合厂商优势"要的计价维度
- **粘性**：跨档位请求不写会话粘性表（档位流量是短请求，如标题生成）
- **直连模式**：不生效，条目直连仍是自家端点（物理约束，UI 明示）

## 七、UniversalProvider 的归宿

数据并入 endpoints 表后，前端 `src/components/universal/` 面板升级为"端点管理"页：厂商连接信息 + 模型清单获取 + 引用情况（哪些分组条目在用）。旧 universal 数据行迁移后删除，`sync_universal_to_apps` 退役——其职责由物化机制接管。

## 八、分阶段落地

| 阶段 | 内容 | 验收 |
|---|---|---|
| P1 数据层 | endpoints 表 + providers 加列 + migrate_v25 + 物化函数 `materialize_settings` | 真实库副本重放迁移（参照 Hermes v24 验证法）：值保留、可回滚；`cargo test` |
| P2 写路径收口 | save_provider 内联重物化；编辑表单改"选端点 + 档位映射"；NULL 条目表单保持原样 | 新旧条目混存时 CRUD 全通过；UI 走查 |
| P3 代理档位路由 | 档位识别前移 + 影子 Provider + 熔断细分 + usage 归因 + 不回落主队列 | 单测（路由选择/熔断 key/usage 记账）+ 本机代理实测：同会话主档走 A 家、haiku 档走 B 家 |
| P4 端点管理页 | universal 面板升级、分组页改引用选择器、四语言 | 无背景者能看懂"端点=连接、条目=策略+映射"；i18n 四文件同步 |
| P5 生态收尾 | backup.rs 导出导入、S3/WebDAV 同步、deeplink、导入旧格式兼容 | 新旧版本互导不丢数据 |

P1-P2 完成即消除冗余（改密钥一处生效）；P3 完成即支持跨厂商组合。两个用户价值各自成段可交付。

**P1 实测（2026-10-24，本机真实库 214MB 副本重放 v24→v25）**：50 条 provider → 12 个端点、43 条完成归并（86%）、0 悬空引用、归并条目凭据与所指端点 100% 一致（重放 harness：`CC_SWITCH_REPLAY_DB=<副本> cargo test --lib replay_v25 -- --ignored`）。

**P2/P3 已实现（2026-10-24）**：
- P2：save_provider/replace_provider_id 保存物化（悬空引用容忍）；save_endpoint 级联重物化引用条目（一处改全局生效）；endpoints CRUD 命令五枚；universal upsert/sync 双轨合流（子条目构造绑定 endpoint_ref）
- P3：`proxy/tier_router.rs`——档位识别与 `ModelMapping::map_model` 同序（fable→haiku→opus→sonnet、subagent 等值含 1M 剥离、default 兜底）；影子 Provider（id=端点 id，熔断/usage/在途按端点归因；`ANTHROPIC_MODEL`=目标模型使任意请求名落 default 档；`meta.api_format` 来自端点驱动 transform 管线跨家转换）；Endpoint 增 `api_format` 列（迁移从条目 meta.apiFormat 回填，首个建端点的条目捐赠）；handler 选池后影子置首、原池保留回落链；端点熔断打开时不构造影子（直接回落本地）；forwarder `tier_routed` 标志抑制会话重映射与当前供应商切换

**P4/P5 已实现（2026-10-24）**：
- P4：前端类型（Endpoint/TierMappings/Provider 扩展）+ endpointsApi；EndpointPanel/EndpointFormModal（全局端点管理，删除带引用守卫）；EndpointRefSection（条目内端点引用 + 六档映射矩阵，claude/claude-desktop 生效）；挂载于 AddProviderDialog；i18n 四语言。universal 面板保留可用（写入经 P2 合流到端点层，无漂移）
- P5：备份/同步自动覆盖 endpoints 表（dump 遍历全表）；往返测试 + 旧版无 endpoints 表导出兼容测试；deeplink/旧 JSON 导入经保存物化天然兼容
- 实机端到端走查（进代理→跨端点档位分流→usage 分账）待真实上游 Key 验证，同 Hermes 走查待办

## 九、风险与对策

1. **迁移不可逆** → 迁移前自动备份已内建；归并保守线；NULL 条目行为不变
2. **端点被删仍被引用** → 删除端点前查引用计数，被引用则拒绝并提示改用"停用"
3. **第三方同步新旧版本互导** → 导出带模型版本号，旧版导入走 legacy 解析
4. **物化缓存与引用漂移** → 单一写入路径（save_provider 重物化）+ 强制重物化诊断命令
5. **测试面扩大（跨家 transform 组合）** → P3 按 (档位命中端点的厂商类型 × 请求格式) 矩阵补集成测试

## 十、已定决策

1. **归并线：保守**——`base_url + api_key` 完全一致才归并；同站不同账号各自成端点
2. **档位回落：回落自家**——目标家挂了退回该档在自身端点的展开模型，不摊给主队列；标题生成类短请求不因 B 家故障中断
3. **节奏：P1-P3 连续完成**——P2 消除冗余、P3 支持跨厂商组合，两段价值合并交付；P4/P5 收尾另排
