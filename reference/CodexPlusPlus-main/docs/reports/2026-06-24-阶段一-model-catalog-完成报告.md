# 阶段一完成报告：按模型上下文 catalog 生成（原型）

> 日期：2026-06-24  
> 范围：CodexPlusPlus 后端核心（`codex-plus-core`）  
> 目标：让 CodexPlusPlus 应用 relay profile 时，根据 `model_list` 后缀语法自动生成 codex 原生 `model_catalog_json` 文件并写入 config.toml 指针，使 codex 客户端按模型识别真实上下文窗口。

---

## 一、计划回顾

阶段一原型的核心任务（见 `docs/plans/2026-06-23-阶段一-model-catalog-原型.md`）：

1. 实现 `model_list` 后缀解析器（`deepseek-v4-pro[1M]` → slug + context_window）。
2. 实现条目收集与 catalog JSON 构建。
3. 在 `relay_config.rs` 新增 `apply_model_catalog_to_config` 并接入 3 个 apply 入口。
4. 补充兼容性与回归测试。
5. 提供手工验证 example 工具。
6. 实跑验证（A 预检 + B 对拍）。

---

## 二、完成情况

| 任务 | 状态 | 关键交付 |
|------|------|----------|
| 1. 后缀解析器 | ✅ 完成 | `crates/codex-plus-core/src/model_suffix.rs` |
| 2. 条目收集与 catalog 构建 | ✅ 完成 | `collect_catalog_entries`、`build_model_catalog_json` |
| 3. apply 接入 3 个入口 | ✅ 完成 | `relay_config.rs` 三处接入 |
| 4. 兼容性与回归测试 | ✅ 完成 | `tests/model_suffix.rs` 9 个、`tests/relay_config.rs` 3 个新增 |
| 5. 手工验证 example | ✅ 完成 | `crates/codex-plus-core/examples/generate_model_catalog.rs` |
| 6. 实跑验证 A/B | ✅ 完成 | 结论写入 `docs/research/01-调研结果.md` 第九节 |

---

## 三、关键实现说明

### 3.1 后缀语法

复用 `model_list` 纯文本输入框，支持：

| 输入 | slug | context_window |
|------|------|----------------|
| `deepseek-v4-pro[1M]` | `deepseek-v4-pro` | 1,000,000 |
| `claude-sonnet-4[200K]` | `claude-sonnet-4` | 200,000 |
| `gpt-5.5[512k]` | `gpt-5.5` | 512,000 |
| `gpt-5.5[1000000]` | `gpt-5.5` | 1,000,000 |
| `gpt-5.5` | `gpt-5.5` | 无（回落顶层） |

- 单位：`K/k`=1000、`M/m`=1000000。
- 后缀在生成 catalog 时剥离，slug 不带后缀进入 catalog 与 codex 请求。
- 无后缀条目 catalog 中仍保留，context_window 回落顶层 `model_context_window` 或默认值 272000。

### 3.2 catalog 生成

- 采用 cc-switch 的 **template-clone** 思路：以 `assets/codex-models.json`（来自 `codex debug models --bundled`）中的 entry 为模板。
- 每条模型 clone 模板后覆盖：`slug`、`display_name`、`description`、`context_window`、`max_context_window`、`effective_context_window_percent`、`priority`、`auto_compact_token_limit` 等字段。
- 显式写入 `"effective_context_window_percent": 100`，避免 codex 默认 95% 导致 1M 显示为 950K。
- `auto_compact_token_limit` 留 `null`，与 codex 内置模型行为一致（按比例自动计算）。
- catalog 写入 `<home>/model-catalogs/<profile-id>.json`，config.toml 注入相对路径指针 `model_catalog_json = "model-catalogs/<id>.json"`。

### 3.3 插入点

在现有 apply 流程中，新增独立可选步骤：

```
complete_relay_profile_config → merge_common → preserve_unmanaged
→ apply_context_limits_to_config（写顶层单值）
→ [新] apply_model_catalog_to_config（生成 catalog + 写指针）
→ 落盘
```

- 无后缀条目时 no-op，旧 profile 行为不变。
- 用户已手写 `model_catalog_json` 指针时保留，不覆盖。
- 3 个 apply 入口全部接入：`files_to_home_with_context`、`switch_rules_and_computer_use_guard`、`config_to_home_with_context`。

---

## 四、测试结果

### 4.1 单元测试

```bash
cargo test -p codex-plus-core --test model_suffix
```

**结果：9 passed; 0 failed**

覆盖：
- `K`/`M` 单位解析
- 纯数字窗口解析
- 无后缀返回 `None`
- 非法/未闭合括号保持原串
- 零值拒绝
- 当前 model + model_list 收集与去重
- 当前 model 从 model_list 同名条目采纳后缀
- catalog JSON 字段与后缀剥离
- 无后缀条目 fallback 窗口

### 4.2 集成测试

```bash
cargo test -p codex-plus-core --test relay_config
```

**结果：83 passed; 0 failed**

新增 3 个用例：
- `apply_relay_profile_generates_model_catalog_for_suffixed_models`：有后缀时生成 catalog 并写入指针。
- `apply_relay_profile_no_catalog_when_model_list_has_no_suffix`：无后缀时 no-op。
- `apply_relay_profile_does_not_overwrite_user_model_catalog_json`：用户手写指针不覆盖。

### 4.3 桌面端编译

```bash
cd apps/codex-plus-manager
npm install
npm run vite:build
cargo build -p codex-plus-manager
cargo test -p codex-plus-manager
```

**结果：** 前端构建成功，`codex-plus-manager` 编译成功，12 个 manager 测试通过。

---

## 五、实跑验证（Mac）

### 5.1 A 预检

手写基于 bundled entry 模板的 catalog，`codex debug models` 读取后报告：

```json
{ "slug": "deepseek-v4-pro", "context_window": 1000000, "effective_context_window_percent": 100 }
```

结论：Mac 绝对路径可用，`context_window` 正确报到 1M。

### 5.2 B 对拍

使用 example 生成原型 catalog：

```bash
cargo run -p codex-plus-core --example generate_model_catalog -- \
  "deepseek-v4-pro[1M]" "claude-sonnet-4[200K]" 2>/dev/null > /tmp/catalog.json
```

`codex debug models` 读取后报告：

```json
[
  { "slug": "deepseek-v4-pro", "context_window": 1000000, "effective": 100 },
  { "slug": "claude-sonnet-4", "context_window": 200000, "effective": 100 }
]
```

结论：原型产物通过 codex 0.128.0 schema 校验，多模型同时列出，无单模型副作用。

---

## 六、已知问题与限制

| 问题 | 影响 | 计划处理 |
|------|------|----------|
| `cargo test -p codex-plus-core` 全量运行有 1 个既有 `settings` 测试失败 | 与本次改动无关 | 不阻塞阶段一，后续单独排查 |
| 模板使用 gpt-5.5 的 `base_instructions` 等人格/能力字段 | 第三方模型可能携带 GPT-5.5 专属系统提示 | 阶段二/三评估是否需要覆盖/清空 |
| Windows 路径兼容性未验证 | 相对路径使用正斜杠 | 阶段二/三在 Windows 环境补测 |
| 前端 UI 仍为纯文本 textarea，无后缀高亮/校验/结构化展示 | 用户体验一般 | 阶段三前端改造 |
| `model_catalog_json` 手写 pointer 检测只看顶层 key | 非顶层 pointer 会被覆盖 | 文档中明确约束 |

---

## 七、尚未完成的工作（阶段二/三）

根据原计划，阶段一之后的方向：

### 阶段二：解析回传

- `parse_model_catalog_json_models` 保留 `context_window` 返回前端。
- 后端完整化：让设置存储/读取能感知每模型窗口。

### 阶段三：前端结构化展示

- `model_list` 从 textarea 改为结构化列表（每行一个模型卡片/输入框）。
- 后缀实时高亮与校验。
- `auto_compact` 按比例 UI（例如 context_window × 0.85 可视化）。
- 完整测试 + 提 PR。

---

## 八、当前能否通过 CodexPlusPlus 桌面端测试

**可以，但需要手动输入后缀。**

阶段一完成后，后端 apply 流程已经支持：只要在某个 relay profile 的 **模型列表（model_list）** 里写：

```
deepseek-v4-pro[1M]
claude-sonnet-4[200K]
```

然后点击 apply/切换，CodexPlusPlus 就会自动：
1. 生成 `~/.codex/model-catalogs/<profile-id>.json`
2. 在 `~/.codex/config.toml` 写入 `model_catalog_json = "model-catalogs/<profile-id>.json"`
3. codex 客户端启动后读取 catalog，按模型使用真实窗口

但前端目前**不会**提示你 `[1M]` 是否合法、不会自动显示解析后的窗口、也不会阻止错误格式。这些体验优化属于阶段三。

---

## 九、下一步建议

如果目标是“现在就能在桌面端用”，阶段一已经够用，只需：
1. 按上面方式编译/运行 `codex-plus-manager`。
2. 在 profile 的模型列表里手写带后缀的模型 slug。
3. 应用 profile，验证 `~/.codex/config.toml` 和 catalog 文件。

如果目标是“做一个集成好功能的 app 前端”，则进入阶段三：设计结构化 model_list UI、后缀校验、窗口可视化、auto_compact 比例配置等。这需要在当前后端基础上扩展前端，工作量中等，主要是 React/TypeScript + Tauri command 的改动。
