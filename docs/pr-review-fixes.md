# PR #5652 Review 修复说明

> 针对 Codex bot 在 [farion1231/cc-switch#5652](https://github.com/farion1231/cc-switch/pull/5652) 上提出的 4 条 review 意见，
> 按照 systematic-debugging 流程逐条定位根因、选型修复方案并验证。

## 涉及文件

| 文件 | 改动 |
|------|------|
| `src-tauri/src/codex_config.rs` | Issue 1 修复 + 测试 |
| `src-tauri/src/services/provider/live.rs` | Issue 2 + Issue 3 修复 + 测试 |
| `src-tauri/src/claude_settings_watcher.rs` | Issue 4 修复 |

---

## Issue 1 (P1): bytes 模式模板的 truncation limit 单位错配

### 现象

Codex bot review 指出：`NativeResponses` 和 `Anthropic` profile 使用 `codex_native_responses_template.json`，
其 `truncation_policy.mode` 为 `bytes`，而 `spec.context_window` 的单位是 token。
直接把 token 值写入 byte 上限，导致 128K-token 模型被截断在 128K bytes（约 32K tokens，仅 1/4 容量）。

### 根因定位

`codex_catalog_model_entry` 函数（`codex_config.rs:476`）负责为每个模型生成 catalog entry。
在 commit `1024e5ee` 中，已修复了"无条件覆盖 mode 为 bytes"的问题，改为保留模板原有 mode。

但问题在于：**保留了模板的 mode 却写入了 token 值的 limit**。

两个模板的默认 truncation_policy：

```
gpt5_5_template.json:           {"mode": "tokens", "limit": 10000}  // ProxyChat 用
codex_native_responses_template.json: {"mode": "bytes",  "limit": 10000}  // NativeResponses / Anthropic 用
```

修复前的代码逻辑：

```rust
// 只更新 limit，保留模板的 mode
if let Some(tp) = entry_obj.get_mut("truncation_policy")... {
    tp.insert("limit", json!(truncation_limit));  // ← token 值
    // mode 保留模板原值 → NativeResponses 保留 "bytes"
}
```

对 ProxyChat（mode=tokens）：limit 写入 token 值，mode 也是 tokens，单位一致，正确。

对 NativeResponses（mode=bytes）：limit 写入 token 值（如 128000），mode 是 bytes，
Codex 会按 128000 **bytes** 来截断上下文，而 128000 bytes 约 32000 tokens，
模型实际有 128K tokens 容量却只能用到 32K，浪费 3/4。

### 解决方案

**选型：统一设 mode="tokens"，而非将 token 换算为 bytes。**

两种可选方案及选型理由：

| 方案 | 做法 | 优点 | 缺点 | 选择 |
|------|------|------|------|------|
| A. token -> byte 换算 | limit = context_window * 4（近似 4 bytes/token） | 保留模板 bytes 模式 | 换算比例不精确（英文约 4，中文 1-3 token/字），引入新的不准确 | 否 |
| B. 强制 mode="tokens" | 写入 limit 时同时设 mode="tokens" | 单位精确匹配，无近似计算 | 改变 NativeResponses 默认截断行为 | 是 |

选 B 的原因：
1. `context_window` 本身就是 token 单位的值，设 mode="tokens" 语义完全正确。
2. token -> byte 换算依赖 tokenizer，不同模型比例不同，引入近似值会制造新的不确定性。
3. 模板默认的 bytes/10000 是保守初始值，实际使用中 limit 一定来自 context_window（token），
   所以 bytes 模式本身就是遗留的模板默认值，不适合承载 token 值。

### 代码改动

```rust
// codex_config.rs - codex_catalog_model_entry()
if let Some(tp) = entry_obj.get_mut("truncation_policy")... {
    tp.insert("mode".to_string(), json!("tokens"));           // 新增：强制 tokens
    tp.insert("limit".to_string(), json!(truncation_limit));
} else {
    // None 分支（模板没有 truncation_policy）也统一用 tokens
    json!({ "mode": "tokens", "limit": truncation_limit })     // bytes -> tokens
}
```

### 验证

新增测试 `catalog_entry_truncation_forces_tokens_mode_for_native_responses`：
使用 NativeResponses profile + contextWindow=128000，断言 mode="tokens" 且 limit=128000。
该测试在修复前会失败（mode 为 "bytes"），修复后通过。

现有测试 `catalog_entry_truncation_preserves_template_mode_tokens`（ProxyChat）仍通过，
因为 ProxyChat 模板本来就是 tokens，强制 tokens 不改变结果。

---

## Issue 2 (P2): 全新安装时 watcher 不启动

### 现象

Codex bot review 指出：当 `~/.claude` 目录不存在时，watcher 的创建被跳过。
`write_live_with_common_config` 在 `write_live_snapshot` 之前调用了 builder，
而目录是在后面的 JSON 写入时才创建的，所以全新安装后会有 `settings.json` 但没有 watcher。

### 根因定位

调用链：

```
write_live_with_common_config()          // live.rs:768
  |
  |-- build_effective_settings_with_common_config()  // live.rs:717
  |     |
  |     |-- [watcher spawn]              // live.rs:749
  |     |     检查: settings_path.parent().exists()
  |     |     ~/.claude 不存在 -> 跳过 watcher
  |     |
  |     └── return effective_settings
  |
  |-- write_live_snapshot()              // live.rs:1086
        |
        |-- write_json_file()
              |
              |-- atomic_write()
                    |
                    |-- create_dir_all(parent)  // 这里才创建 ~/.claude
```

`build_effective_settings_with_common_config` 中的 watcher spawn 代码检查父目录是否存在：

```rust
if settings_path.parent().map(|p| p.exists()).unwrap_or(false) {
    // spawn watcher
}
```

commit `915434a7` 曾将检查从"文件存在"改为"父目录存在"，解决了 `settings.json` 不存在但
父目录存在的情况。但如果父目录本身都不存在（真正的全新安装），watcher 仍然不会启动。

而 `write_live_snapshot` -> `atomic_write` -> `create_dir_all` 才会创建父目录，
但此时 watcher spawn 早已执行完毕并跳过了。

### 解决方案

**选型：在 watcher spawn 之前主动创建父目录。**

两种可选方案及选型理由：

| 方案 | 做法 | 优点 | 缺点 | 选择 |
|------|------|------|------|------|
| A. 将 watcher spawn 移到 write_live_snapshot 之后 | 重构 write_live_with_common_config 调用顺序 | 语义更干净 | 需要改函数结构，build_effective 返回值依赖 watcher 副作用，移动后需调整 | 否 |
| B. 在 watcher spawn 之前 create_dir_all | 在检查前先确保父目录存在 | 改动最小，create_dir_all 本身是幂等的 | build 函数多了一个副作用（创建目录） | 是 |

选 B 的原因：
1. `create_dir_all` 是幂等操作，目录已存在时不会报错。
2. `write_live_snapshot` 后续也会调用 `atomic_write` -> `create_dir_all`，提前调用只是把时机前移，不会产生多余目录。
3. 改动范围最小，不涉及函数结构调整。

### 代码改动

```rust
// live.rs - build_effective_settings_with_common_config()
if matches!(app_type, AppType::Claude) {
    let settings_path = get_claude_settings_path();
    // 新增：确保父目录存在
    if let Some(parent) = settings_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("[ClaudeSettingsWatcher] failed to create {}: {e}", parent.display());
        }
    }
    // 原有检查（此时父目录已存在，必然通过）
    if settings_path.parent().map(|p| p.exists()).unwrap_or(false) {
        // spawn watcher
    }
}
```

### 达成的效果

全新安装场景：用户首次激活 Claude provider 时，`~/.claude` 被提前创建，
watcher 立即启动监听。后续 `/model` 切换能正常同步 ACW/MAX，无需等待第二次 provider 构建。

---

## Issue 3 (P2): autoSyncContextWindow 泄露到 Claude settings.json

### 现象

Codex bot review 指出：保存 auto-sync 开关时，`autoSyncContextWindow` 被写入 provider 的
settings 对象，但 `sanitize_claude_settings_for_live` 没有移除它，导致这个 cc-switch 专用字段
泄露到 Claude Code 的 `settings.json`，可能触发 invalid-setting 告警。

### 根因定位

数据流有两条独立路径，问题出在"写入 Claude settings.json"这条路径没有剥离该字段：

```
provider.settings_config (cc-switch 内部存储)
  |
  |-- 路径 A: sanitize -> write_live_snapshot -> Claude settings.json
  |     sanitize 只移除了 api_format / openrouter_compat_mode
  |     autoSyncContextWindow 不在移除列表 -> 泄露到 Claude 文件
  |
  |-- 路径 B: watcher 直接读 provider.settings_config
        watcher 需要 autoSyncContextWindow 判断是否启用自动同步
```

`sanitize_claude_settings_for_live`（`live.rs:216`）的移除列表：

```rust
obj.remove("api_format");
obj.remove("apiFormat");
obj.remove("openrouter_compat_mode");
obj.remove("openrouterCompatMode");
// autoSyncContextWindow 缺失
```

前端 `handleAutoSyncChange`（`ClaudeFormFields.tsx:296`）把 `autoSyncContextWindow` 写进
`settingsConfig`，存入 provider 的 `settings_config`。激活 provider 时 `write_live_snapshot`
调用 `sanitize_claude_settings_for_live` 清洗后写入 Claude 的 `settings.json`，
但 `autoSyncContextWindow` 未被清洗，原样泄露。

而 watcher（`claude_settings_watcher.rs:214`）读取的是 `provider.settings_config`（cc-switch 内部对象），
不是 Claude 的 `settings.json`，所以剥离该字段不影响 watcher 功能。

### 解决方案

**选型：在 sanitize 中添加 `obj.remove("autoSyncContextWindow")`。**

这是唯一合理的方案：`autoSyncContextWindow` 是 cc-switch 专用字段，和已有的 `api_format`、
`openrouter_compat_mode` 性质完全相同，都是"cc-switch 内部使用、不写入目标工具配置文件"的字段。
watcher 从 provider 对象读取，不依赖 Claude 文件中的副本。

### 代码改动

```rust
// live.rs - sanitize_claude_settings_for_live()
obj.remove("openrouterCompatMode");
// 新增
obj.remove("autoSyncContextWindow");
```

### 验证

新增测试 `sanitize_strips_auto_sync_context_window`：
构造含 `autoSyncContextWindow: true` 的 settings，调用 sanitize 后断言该字段不存在，
同时验证其他字段（如 env.ANTHROPIC_MODEL）保留不变。

### 达成的效果

Claude Code 的 `settings.json` 不再包含 `autoSyncContextWindow`，避免 invalid-setting 告警。
watcher 功能不受影响，因为它从 provider 内部对象读取该字段。

---

## Issue 4 (P2): watcher 写入使用非原子操作

### 现象

Codex bot review 指出：watcher 回调中使用 `std::fs::write` 写入 `settings.json`，
该操作会先截断文件再写入，存在时间窗口。如果 Claude Code 在此期间读取文件，
会读到空文件或残缺 JSON。仓库的常规 JSON 写入路径使用 `config::atomic_write` 来避免此问题。

### 根因定位

`handle_settings_change`（`claude_settings_watcher.rs:195`）在监听到 `/model` 变化后，
计算新的 ACW/MAX 值并写入 `settings.json`：

```rust
if let Err(e) = std::fs::write(path, new_content) {
    log::warn!("[ClaudeSettingsWatcher] write failed: {e}");
}
```

`std::fs::write` 的内部流程：

```
1. 打开文件（O_WRONLY | O_CREAT | O_TRUNC）  → 文件被截断为 0 字节
2. 写入新内容                                 → 文件处于半写状态
3. 关闭文件                                    → 写入完成
```

步骤 1-2 之间文件是空的或残缺的。如果 Claude Code 恰好在此期间读取 `settings.json`
（例如用户在终端切换 `/model` 触发 Claude Code 重新加载配置），会读到无效 JSON。

仓库已有 `atomic_write`（`config.rs:297`），其流程：

```
1. 创建临时文件 settings.json.tmp.{timestamp}
2. 将完整数据写入临时文件
3. flush 确保落盘
4. rename 临时文件 -> 目标文件（原子操作）
```

`rename` 系统调用在同一文件系统内是原子的，读者要么看到旧的完整文件，
要么看到新的完整文件，不存在中间状态。仓库的常规写入路径（`write_json_file`）已经使用 `atomic_write`，
但 watcher 这里遗漏了。

### 解决方案

**选型：将 `std::fs::write` 替换为 `crate::config::atomic_write`。**

无需选择：仓库已有现成的 `atomic_write` 函数，且常规写入路径已在使用。
watcher 应与常规路径保持一致。

### 代码改动

```rust
// claude_settings_watcher.rs - handle_settings_change()
// 修改前
if let Err(e) = std::fs::write(path, new_content) { ... }
// 修改后
if let Err(e) = crate::config::atomic_write(path, new_content.as_bytes()) { ... }
```

### 达成的效果

watcher 写入 `settings.json` 时使用临时文件 + rename 的原子替换机制，
Claude Code 或其他终端在写入期间不会读到空文件或残缺 JSON。
与仓库常规写入路径行为一致。

---

## 验证汇总

| 验证项 | 结果 |
|--------|------|
| `cargo check` | 通过 |
| `cargo fmt --check` | 通过 |
| `cargo clippy --lib` | 无警告 |
| `catalog_entry_truncation_*` 测试（4 个） | 全部通过 |
| `sanitize_strips_auto_sync_context_window` 测试 | 通过 |
| `claude_settings_watcher` 全部测试（35 个） | 全部通过 |
| `sanitize` 相关测试（50 个） | 全部通过 |

### 改动统计

```
 src-tauri/src/claude_settings_watcher.rs      |  4 +++-
 src-tauri/src/codex_config.rs                 | 40 ++++++++++++++++++++++++++------
 src-tauri/src/services/provider/live.rs       | 30 ++++++++++++++++++++++--
 3 files changed, 64 insertions(+), 10 deletions(-)
```
