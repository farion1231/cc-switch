# Bedrock 请求优化器 - 实施计划

**日期**: 2026-03-05
**设计文档**: `docs/plans/2026-03-05-optimizer-enhancement-pr.md`
**预估总改动**: 新建 2 文件 (~180 行) + 修改 5 文件 (~70 行)

---

## 任务拆分

### Task 1: OptimizerConfig 结构体 (types.rs)

**文件**: `src-tauri/src/proxy/types.rs`
**改动**: +15 行

在 `RectifierConfig` 之后新增：

```rust
/// 请求优化器配置
///
/// 存储在 settings 表中，key = "optimizer_config"
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerConfig {
    /// 总开关（默认关闭）
    #[serde(default)]
    pub enabled: bool,
    /// Thinking 优化子开关（总开关开启后默认生效）
    #[serde(default = "default_true")]
    pub thinking_optimizer: bool,
    /// Cache 注入子开关（总开关开启后默认生效）
    #[serde(default = "default_true")]
    pub cache_injection: bool,
    /// Cache TTL: "5m" | "1h"
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: String,
}

fn default_cache_ttl() -> String {
    "1h".to_string()
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            thinking_optimizer: true,
            cache_injection: true,
            cache_ttl: "1h".to_string(),
        }
    }
}
```

**验证**: `cargo test -p cc-switch -- types::` 通过

---

### Task 2: 模块注册 (mod.rs)

**文件**: `src-tauri/src/proxy/mod.rs`
**改动**: +2 行

在 `pub mod thinking_rectifier;` 之后新增：

```rust
pub mod thinking_optimizer;
pub mod cache_injector;
```

**验证**: 编译通过（需配合 Task 3/4 创建文件）

---

### Task 3: thinking_optimizer.rs (新建)

**文件**: `src-tauri/src/proxy/thinking_optimizer.rs`
**行数**: ~80 行

**公开接口**:

```rust
pub fn optimize(body: &mut Value, config: &OptimizerConfig)
```

**核心逻辑**:

1. 从 `body["model"]` 提取模型名称
2. 三路径分发：
   - **skip**: 模型名含 `haiku` → 直接返回，不修改
   - **adaptive**: 模型名含 `opus-4-6` 或 `sonnet-4-6` →
     - `body["thinking"] = {"type": "adaptive"}`（移除 budget_tokens）
     - `body["output_config"]["effort"] = "max"`
     - 追加 `"context-1m-2025-08-07"` 到 `body["anthropic_beta"]` 数组
   - **legacy**: 其他模型 →
     - 若 thinking 为 null/disabled: 注入 `{"type":"enabled","budget_tokens": max_tokens-1}`，追加 `"interleaved-thinking-2025-05-14"` beta
     - 若 thinking 已有但 budget < max_tokens-1: 升级 budget
     - 若 budget 已是最大: 不修改

3. 日志输出: `[OPT] thinking: adaptive(opus-4-6)` / `legacy(sonnet-4-5,budget=16383)` / `skip(haiku)`

**关键细节**:
- `anthropic_beta` 是 body 中的数组字段（非 HTTP header），在 Bedrock SDK 场景下通过 body 传递
- 需要处理 `anthropic_beta` 不存在、为 null、已有值等情况
- `config.thinking_optimizer == false` 时跳过

**单元测试** (至少 6 个):
- adaptive 路径: opus-4-6 模型
- adaptive 路径: sonnet-4-6 模型
- legacy 路径: sonnet-4-5 模型 (thinking=null)
- legacy 路径: sonnet-4-5 模型 (budget 已存在但偏小)
- skip 路径: haiku 模型
- 子开关关闭: thinking_optimizer=false 时不修改

---

### Task 4: cache_injector.rs (新建)

**文件**: `src-tauri/src/proxy/cache_injector.rs`
**行数**: ~100 行

**公开接口**:

```rust
pub fn inject(body: &mut Value, config: &OptimizerConfig)
```

**核心逻辑**:

1. `config.cache_injection == false` → 跳过
2. 计算已有断点: 遍历 body 中所有 `cache_control` 字段，计数 `existing_count`
3. 计算可注入数: `budget = 4 - existing_count`
4. TTL 升级: 将所有已有 `cache_control` 的 TTL 升级到 `config.cache_ttl`（如果当前低于配置值）
5. 若 `budget > 0`，按优先级注入新断点:
   - (a) `body["tools"]` 最后一个元素 → 添加 `cache_control`
   - (b) `body["system"]` 最后一个 block → 添加 `cache_control`（如果 system 是字符串，先转为 `[{"type":"text","text":"..."}]`）
   - (c) `body["messages"]` 逆序查找最后一条 `role=assistant` 的 `content` 中最后一个非 `thinking`/`redacted_thinking` block → 添加 `cache_control`
6. 新断点格式:
   - TTL = "5m": `{"type":"ephemeral"}`（省略 ttl 字段）
   - TTL = "1h": `{"type":"ephemeral","ttl":"1h"}`

7. 日志输出: `[OPT] cache: 3bp(tools+system+msgs,1h,pre=0)` / `ttl-upgrade(2->1h,existing=4)` / `no-op(existing=4)`

**单元测试** (至少 7 个):
- 无 tools/无 system/无 assistant: 注入 0 个
- 有 tools+system+assistant msgs: 注入 3 个
- 已有 4 个断点: 仅升级 TTL
- 已有 2 个断点: 注入 2 个新的
- system 为字符串: 自动转数组后注入
- TTL="5m" 时断点不含 ttl 字段
- 子开关关闭: cache_injection=false 时不修改

---

### Task 5: DB 层 + Tauri Command (settings.rs + commands/settings.rs + lib.rs)

**文件**:
- `src-tauri/src/database/dao/settings.rs` (+15 行)
- `src-tauri/src/commands/settings.rs` (+15 行)
- `src-tauri/src/lib.rs` (+2 行)

**改动**: 完全参照 `rectifier_config` 的模式复制

**settings.rs (DAO)**:
```rust
// --- 优化器配置 ---
pub fn get_optimizer_config(&self) -> Result<OptimizerConfig, AppError> {
    match self.get_setting("optimizer_config")? {
        Some(json) => serde_json::from_str(&json)
            .map_err(|e| AppError::Database(format!("解析优化器配置失败: {e}"))),
        None => Ok(OptimizerConfig::default()),
    }
}

pub fn set_optimizer_config(&self, config: &OptimizerConfig) -> Result<(), AppError> {
    let json = serde_json::to_string(config)
        .map_err(|e| AppError::Database(format!("序列化优化器配置失败: {e}")))?;
    self.set_setting("optimizer_config", &json)
}
```

**commands/settings.rs**:
```rust
#[tauri::command]
pub async fn get_optimizer_config(state: State<'_, AppState>) -> Result<OptimizerConfig, String> {
    state.db.get_optimizer_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_optimizer_config(state: State<'_, AppState>, config: OptimizerConfig) -> Result<bool, String> {
    state.db.set_optimizer_config(&config).map_err(|e| e.to_string())?;
    Ok(true)
}
```

**lib.rs**: 在 `invoke_handler` 中注册 `get_optimizer_config` 和 `set_optimizer_config`

**验证**: 编译通过

---

### Task 6: handler_context.rs + forwarder.rs 集成

**文件**:
- `src-tauri/src/proxy/handler_context.rs` (+5 行)
- `src-tauri/src/proxy/forwarder.rs` (+20 行)

**handler_context.rs**:
1. `RequestContext` 结构体新增字段: `pub optimizer_config: OptimizerConfig`
2. `RequestContext::new()` 中从 DB 加载: `let optimizer_config = state.db.get_optimizer_config().unwrap_or_default();`
3. `create_forwarder()` 将 `optimizer_config` 传入 `RequestForwarder::new()`

**forwarder.rs**:
1. `RequestForwarder` 新增字段: `optimizer_config: OptimizerConfig`
2. `RequestForwarder::new()` 参数新增 `optimizer_config`
3. `forward_with_retry()` 中，`for provider in providers.iter()` 循环**之前**，加入:

```rust
// PRE-SEND 优化器：仅 Bedrock provider 生效
if self.optimizer_config.enabled {
    if let Some(first_provider) = providers.first() {
        if is_bedrock_provider(first_provider) {
            if self.optimizer_config.thinking_optimizer {
                thinking_optimizer::optimize(&mut body, &self.optimizer_config);
            }
            if self.optimizer_config.cache_injection {
                cache_injector::inject(&mut body, &self.optimizer_config);
            }
        }
    }
}
```

4. 新增辅助函数（forwarder.rs 底部）:

```rust
fn is_bedrock_provider(provider: &Provider) -> bool {
    provider.settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_USE_BEDROCK"))
        .and_then(|v| v.as_str())
        .map(|v| v == "1")
        .unwrap_or(false)
}
```

**验证**: `cargo build` 通过

---

### Task 7: 前端 UI (RectifierConfigPanel.tsx + settings API)

**文件**:
- `src/lib/api/settings.ts` (+15 行)
- `src/components/settings/RectifierConfigPanel.tsx` (+30 行)
- i18n 文件 (中/英)

**settings.ts**:
```typescript
export interface OptimizerConfig {
  enabled: boolean;
  thinkingOptimizer: boolean;
  cacheInjection: boolean;
  cacheTtl: string;
}

// settingsApi 中新增:
async getOptimizerConfig(): Promise<OptimizerConfig> {
  return await invoke("get_optimizer_config");
},
async setOptimizerConfig(config: OptimizerConfig): Promise<boolean> {
  return await invoke("set_optimizer_config", { config });
},
```

**RectifierConfigPanel.tsx**:
在现有整流器区域之后，新增 Optimizer 区域，包含:
- 总开关 toggle (enabled)
- Thinking 优化 toggle (thinkingOptimizer，disabled when !enabled)
- Cache 注入 toggle (cacheInjection，disabled when !enabled)
- Cache TTL 选择 (cacheTtl: "5m" | "1h"，disabled when !enabled || !cacheInjection)

参照 `RectifierConfigPanel` 的 `handleChange` 模式，独立管理 `OptimizerConfig` 状态。

**验证**: `npm run build` 通过，UI 正确显示

---

## 实施顺序

```
Task 1 (types.rs)
  └─→ Task 2 (mod.rs) + Task 3 (thinking_optimizer.rs) + Task 4 (cache_injector.rs)  [并行]
        └─→ Task 5 (DB + Command)
              └─→ Task 6 (handler_context + forwarder 集成)
                    └─→ Task 7 (前端 UI)
```

Task 1 → 2/3/4 并行 → 5 → 6 → 7，线性依赖链中间穿插并行。

---

## 风险点

| 风险 | 缓解 |
|------|------|
| thinking_optimizer 修改 body 后触发 Rectifier | 预期行为：两者互补，Rectifier 仍会根据错误类型触发 |
| cache 断点计数错误导致超过 4 个上限 | 单元测试覆盖边界场景 |
| anthropic_beta 数组操作在不同请求格式下不一致 | 测试 null/缺失/已有值三种情况 |
| forwarder.rs 参数已经很多（clippy::too_many_arguments） | 现有代码已 allow，保持一致 |
