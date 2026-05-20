# CC-Switch Claude 接管配置修复 — 项目变更重新提交

## 提交日期

2026-05-20

## 变更范围

本次重新提交聚焦 Claude Code 被 cc-switch 本地代理接管后，`~/.claude/settings.json` 被覆盖或污染的问题。

当前工作区变更：

| 状态 | 文件/目录 | 说明 |
|------|-----------|------|
| 修改 | `src-tauri/src/services/config.rs` | Claude Live 同步改为读-改-写，仅更新 `env` |
| 修改 | `src-tauri/src/services/provider/live.rs` | Provider 写入 Claude Live 时保留用户配置 |
| 修改 | `src-tauri/src/services/proxy.rs` | 代理接管/热切换改为白名单 env 更新，保留顶层配置和自定义 env |
| 新增 | `COLLABORATION_LOG.md` | 本次项目变更记录 |
| 新增 | `build-output/` | 已构建的 Windows 可执行文件和安装包 |

## 问题背景

cc-switch 启用本地代理接管 Claude Code 后，每次启动、供应商同步或代理热切换时，Claude 的 `settings.json` 存在被全量覆盖或被代理字段污染的风险，导致以下用户配置丢失或被改写：

- `enabledPlugins`（已安装插件列表）
- `hooks`（PostToolUse / SessionStart / Setup）
- `permissions`
- 顶层 `model` 等 Claude Code 原生设置
- 自定义 `env`（例如 `GITHUB_PERSONAL_ACCESS_TOKEN`、`QQMAIL_*`、用户自定义环境变量等）

## 根因

### 1. settings.json 写入路径执行全量覆盖

| 文件 | 函数 | 旧行为 |
|------|------|--------|
| `src-tauri/src/services/provider/live.rs` | `write_live_snapshot` | 将 provider 的 `settings_config` 全量写入 Claude `settings.json` |
| `src-tauri/src/services/config.rs` | `sync_claude_live` | 同样将 provider 配置全量覆盖到 Claude Live 配置 |

Provider 配置通常只包含代理所需的 `env`，因此全量写入会删除 Claude Code 原本保存的插件、钩子、权限等字段。

### 2. 代理接管逻辑改写范围过宽

`src-tauri/src/services/proxy.rs` 旧逻辑会：

- 移除顶层 `model`
- 写入 provider 根级字段
- 清理较多 token/model 相关 env key
- 热切换时用 provider 配置刷新部分 Live 设置

这些行为会让代理接管不再只是“临时接管请求路由”，而是修改了用户的 Claude Code 配置外壳。

## 修复内容

### 1. Claude Live 写入改为读-改-写

涉及文件：

- `src-tauri/src/services/provider/live.rs`
- `src-tauri/src/services/config.rs`

新逻辑：

1. 先读取现有 `~/.claude/settings.json`
2. 如果文件不存在或读取失败，则使用空对象兜底
3. 从 provider 配置中提取清洗后的 `env`
4. 只替换现有配置里的 `env` 字段
5. 保留所有其他顶层字段，例如 `enabledPlugins`、`hooks`、`permissions`、`model`

### 2. 代理接管改为 env 白名单更新

涉及文件：

- `src-tauri/src/services/proxy.rs`

新增白名单：

```rust
const CLAUDE_TAKEOVER_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ENABLE_TOOL_SEARCH",
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
    "API_TIMEOUT_MS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
];
```

接管时现在只会写入代理必需字段：

- `ANTHROPIC_BASE_URL`
- `ANTHROPIC_AUTH_TOKEN = PROXY_MANAGED`

从 provider 同步到 Live 时，也只允许同步白名单中的 env key。非白名单字段会保留原样，不再删除或覆盖用户自定义 env。

### 3. 热切换行为调整

代理运行期间切换 provider 时：

- 保留 `permissions`
- 保留 `enabledPlugins`
- 保留顶层 `model`
- 保留自定义 env
- 更新 provider 路由相关模型字段
- 不再写入 `ANTHROPIC_DEFAULT_*_MODEL_NAME` 这类展示名字段
- 保持 `ANTHROPIC_AUTH_TOKEN` 为 `PROXY_MANAGED`

## 行为变化对比

| 场景 | 修复前 | 修复后 |
|------|--------|--------|
| 启动代理接管 Claude | 可能覆盖整个 `settings.json` | 只接管必要 env，其他配置保留 |
| 供应商同步 Claude Live | provider 配置全量写入 | 只更新 `env` |
| 热切换 provider | 可能刷新顶层配置和非必要 env | 只更新白名单 env |
| 用户插件/钩子/权限 | 有丢失风险 | 保留 |
| 用户自定义 env | 有丢失风险 | 保留 |
| 代理 token | 可能沿用不同 key | 固定使用 `ANTHROPIC_AUTH_TOKEN = PROXY_MANAGED` |

## 构建产物

目录：

```text
E:\AI_ALL\_projects\CC_Switch_agent\cc-switch-Agent-scheduling-main\build-output\
```

| 文件 | 大小 | MD5 |
|------|------|-----|
| `cc-switch.exe` | 30,895,104 bytes | `C6E4EB4F535667391547DC6E6847D7DC` |
| `CC Switch_3.15.2_x64-setup.exe` | 10,014,582 bytes | `A1522DC78816A6D5A6C670DAFBD38723` |
| `CC Switch_3.15.2_x64_en-US.msi` | 15,036,416 bytes | `EEBE622C7D94CEAB7B46E328B022FECF` |

## 当前运行状态

- 安装路径：`E:\APP\Technology\switch\cc-switch.exe`
- 代理端口：`127.0.0.1:15721`
- 已观察状态：自 2026-05-20 00:56 起，未再发现 `settings.json` 被全量覆盖
- 防御脚本仍可保留作为额外保险：
  - `E:\Code_file\Claude_code\settings-watcher.ps1`
  - `C:\Users\Satanchen\.claude\settings-backups\`

## 验证重点

代码中相关测试断言已随行为一起调整，覆盖重点包括：

- `claude_takeover_preserves_env_and_exposes_haiku_one_m_role`
- `claude_proxy_lifecycle_preserves_plugins_during_takeover_and_restores_exact_snapshot`
- `claude_takeover_uses_current_provider_models_while_preserving_original_live_shell`
- `hot_switch_provider_updates_claude_live_while_preserving_takeover_fields`

这些断言现在明确要求：

- 接管不能移除顶层 `model`
- 接管必须保留 `enabledPlugins`
- 接管必须保留 `permissions`
- 热切换必须保留非白名单自定义 env
- 热切换不得写入非白名单模型展示名字段

## 本次验证结果

已在 `src-tauri` 目录执行：

```powershell
cargo test --lib claude_takeover --features test-hooks
cargo test --lib hot_switch_provider_updates_claude_live_while_preserving_takeover_fields --features test-hooks
```

结果：

- `claude_takeover` 匹配的 3 个测试通过
- `hot_switch_provider_updates_claude_live_while_preserving_takeover_fields` 通过
- 测试过程中出现 14 个既有 unused/dead_code warning，未影响测试结果

## 已知剩余风险

1. `sanitize_claude_settings_from_live` 仍会在从 Live 读取回 cc-switch 数据库时剥离 `enabledPlugins`、`mcpServers`、`projects`。这是 provider 配置隔离逻辑，当前不会导致 Live 写入丢失。
2. `update_live_backup_from_provider` 仍可能把仅含 provider env 的配置写入 DB 备份。若未来从该备份恢复，需要继续确认是否会影响用户字段。
3. MSI 构建过程曾遇到 Tauri WiX ICE38 校验问题；当前 `build-output` 中 MSI 已产出，但后续自动化构建仍建议单独复核 WiX 链路。

## 重新提交结论

本次变更已将 Claude Code 配置保护范围从“只避免全量覆盖”扩展为“代理接管期间只允许改写必要 env 白名单”。这能同时覆盖启动接管、供应商同步、代理热切换三个路径，避免插件、钩子、权限、顶层模型设置和自定义环境变量再次丢失。
