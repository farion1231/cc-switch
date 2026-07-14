# 设计：按模型上下文配置（model_list 后缀 → catalog 生成）

- 日期：2026-06-23
- 状态：已批准（阶段一原型），待实跑验证后推进
- 对应 issue：#1171 / #931
- 阶段：阶段一（原型验证）

## 1. 背景与目标

CodexPlusPlus 使用第三方模型（deepseek-v4-pro 1M、Claude 200K）时，上下文窗口被锁 258K。根因见 `docs/research/01-调研结果.md`：codex 客户端二进制硬编码 272000，custom provider slug 回落默认；CodexPlusPlus 不生成 `model_catalog_json`，用户被迫手改 config.toml。

**目标**：让 CodexPlusPlus 应用 profile 时自动生成 catalog + 写指针，按模型配置窗口，规避 #931 的路径转义坑与单模型副作用。

## 2. 范围（阶段一原型）

- **后缀语法**：复用 `model_list` 文本框，`deepseek-v4-pro[1M]` 声明窗口
- **catalog 生成**：新增一个函数，生成 catalog JSON + 写 config.toml 指针
- **实跑验证**：codex 客户端 读原型产物，确认报 1M（对拍 cc-switch 格式）

**不在阶段一**：解析回传 context_window 到前端、前端结构化展示、auto_compact 按比例 UI（后两个阶段）。

## 3. 设计

### 3.1 后缀语法（复用 model_list，不新增字段）

`model_list` 是纯文本（换行/逗号分隔 slug），不进 config.toml。后缀作为输入糖：

| 输入 | slug | context_window |
| --- | --- | --- |
| `deepseek-v4-pro[1M]` | `deepseek-v4-pro` | 1000000 |
| `claude-sonnet-4[200K]` | `claude-sonnet-4` | 200000 |
| `gpt-5.5[512k]` | `gpt-5.5` | 512000 |
| `gpt-5.5[1000000]` | `gpt-5.5` | 1000000（纯数字） |
| `gpt-5.5` | `gpt-5.5` | 无（回落顶层） |

- 单位：`K/k`=1000、`M/m`=1000000
- 后缀在生成 catalog 时剥离，slug 不带后缀进 catalog 与 codex 请求
- 无后缀条目：catalog 里 context_window 留空或回落顶层 `model_context_window`

### 3.2 catalog 生成（抄 cc-switch template 思路）

新增函数 `generate_profile_model_catalog(home, profile) -> Option<(catalog_path, config_injection)>`：

1. 解析 `profile.model_list`，提取带后缀的条目；无后缀条目则 no-op（返回 None）
2. 取 codex 自带 entry 当 template：`codex debug models --bundled` 输出，或 fallback 用内置 `assets/codex-models.json` 模板
3. 每条 clone template 后覆盖：`slug`（剥离后缀）/ `display_name` / `context_window` / `max_context_window`，保留 `priority` / `supported_in_api` / `visibility` 等字段
4. **包含 profile 全部模型**（解决 #1064 单模型副作用）：当前 model + model_list 全部条目都写进 catalog，带后缀的写窗口，无后缀的回落顶层
5. 写到 `<home>/model-catalogs/<profile-id>.json`
6. config.toml 注入 `model_catalog_json = "model-catalogs/<id>.json"`（**相对路径**）

### 3.3 插入点（中间层影响最小）

现有流程：
```
complete_relay_profile_config → merge_common → preserve_unmanaged
→ apply_context_limits_to_config（写顶层单值）
→ 落盘
```

新增独立可选步骤，插在 `apply_context_limits_to_config` 之后、落盘之前：
```
→ apply_context_limits_to_config
→ [新] apply_model_catalog_for_profile(home, profile, &config_with_limits)
→ 落盘
```

- **签名不变**：不修改任何现有 apply 函数签名
- **opt-in**：靠后缀触发，无后缀 no-op
- **旧 profile 零行为变化**：那两个「不写 catalog」测试继续过（无需改它们）

### 3.4 catalog 字段决策

| 字段 | 值 | 依据 |
| --- | --- | --- |
| `context_window` | 后缀解析值 | #931 / cc-switch |
| `max_context_window` | 同 context_window | cc-switch |
| `effective_context_window_percent` | 待验证 | cc-switch 写 95 致显示 950K；若想显示真实值写 100 或省略 |
| `auto_compact_token_limit` | null | codex 内置模型即 null（按比例算）；详见调研文档第六节 |

## 4. 兼容性

- 无后缀 → no-op，旧 profile 行为不变
- 用户已手写 `model_catalog_json` 指针 → 检测到则跳过生成，不覆盖（保 `preserves_user_model_catalog_json` 测试过）
- `model_list` 不进 config.toml，后缀不污染 codex 请求
- 顶层 `model_context_window` / `model_auto_compact_token_limit` 保留作回退默认

## 5. 验证计划（阶段一核心）

### A. 预检（手工对拍）
手抄 cc-switch 格式 catalog（deepseek-v4-pro→1000000）+ config.toml 指针，**直接起 codex 客户端（PureApi，不经 relay 代理）**，确认报 ~1M。消除「#931 是别人环境结论」疑虑，拿到已知可用样本。

### B. 原型产物对拍
实跑原型生成的 catalog + 指针，同样起 codex 客户端，确认报 1M。与 A 样本对比字段。

### 必验项
1. **Mac 相对路径**：`model-catalogs/<id>.json` 在 Mac 是否可行（#931 转义结论是 Windows 的）
2. **字段名**：`context_window`+`max_context_window` 是否让 codex 生效（#931/cc-switch 一致，但须实跑确认；不认则试 `n`）
3. **auto_compact null**：1M 窗口是否在合理比例压缩（非 220K 低值）；异常则 fallback 写 `×0.85`
4. **effective_percent**：写 100 / 省略时显示是否为真实 1M
5. **单模型副作用**：catalog 含全部模型时，列表是否完整（解决 #1064）

## 6. 测试计划（主仓风格）

新增 `crates/codex-plus-core/tests/relay_config.rs` 用例：
- 有后缀 → 生成 catalog 文件 + config.toml 指针 + slug 剥离后缀
- 无后缀 → 不生成（no-op）
- 手写指针 → 不覆盖
- 相对路径格式正确
- catalog 含 profile 全部模型

## 7. 风险

| 风险 | 应对 |
| --- | --- |
| codex 不认原型 catalog 字段 | A 预检对拍；试 `n` 字段；查 codex 版本差异 |
| Mac 相对路径不生效 | 改绝对路径；路径转义按平台处理 |
| template 取不到（codex debug models 不可用） | fallback 内置 `assets/codex-models.json` 模板 |
| 单模型副作用未解 | catalog 写全部模型；若仍只显示当前 model，查 codex 版本行为 |

## 8. 后续阶段方向

- **阶段二**：解析回传——`parse_model_catalog_json_models` 保留 `context_window` 返回前端；后端完整化
- **阶段三**：前端结构化展示（后缀解析高亮/校验）+ auto_compact 按比例 UI + 全测试 + 提 PR
