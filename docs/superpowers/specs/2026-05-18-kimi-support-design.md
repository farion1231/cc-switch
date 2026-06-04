# Kimi Code CLI 支持设计文档

## 背景

在 cc-switch 中新增对 Kimi Code CLI 的 API Key 切换支持。Kimi Code CLI 是 Moonshot AI 推出的命令行 AI 助手，配置文件位于 `~/.kimi-code/config.toml`。

## 目标

- 在 cc-switch 的 App 切换栏中新增 "Kimi Code"
- 支持添加、切换多个 Kimi API provider
- 切换后影响 Kimi Code CLI 的 `~/.kimi-code/config.toml`
- 重新启动 Kimi Code CLI 时使用新的账号配置

## 非目标

- 本次不支持 MCP 配置同步到 Kimi CLI（MVP 后续迭代）
- 不支持 Kimi CLI 的用量查询（复用现有 Claude 聚合商预设即可）
- 不支持 `services.moonshot_search/fetch` 的自动切换

## 方案概述

采用**固定 Provider 名称 + `toml_edit` 精准修改**方案：

1. 在 `~/.kimi-code/config.toml` 中固定使用 provider 名称 `ccswitch`
2. 切换时只修改 `[providers.ccswitch]` 中的 `base_url` 和 `api_key`
3. 保留 `default_model`、`models`、`mcp`、`services` 等用户原有配置完全不变
4. 首次切换且 `~/.kimi-code/config.toml` 不存在时，生成最小默认配置

## 数据模型

### Provider settingsConfig 格式

```json
{
  "env": {
    "KIMI_BASE_URL": "https://api.moonshot.cn/v1",
    "KIMI_API_KEY": "sk-xxx"
  }
}
```

与 Gemini 的 `.env` 模式保持一致，后端 `kimi_config.rs` 负责映射为 TOML。

### 预设配置

| 预设名称 | base_url | category |
|---------|----------|----------|
| Kimi Official | `https://api.moonshot.cn/v1` | official |
| Kimi For Coding | `https://api.kimi.com/coding/v1` | cn_official |

### 首次使用的默认 TOML 模板

```toml
default_model = "kimi-for-coding"

[providers.ccswitch]
type = "kimi"
base_url = "<从 provider.settingsConfig.env.KIMI_BASE_URL 读取>"
api_key = "<从 provider.settingsConfig.env.KIMI_API_KEY 读取>"

[models.kimi-for-coding]
provider = "ccswitch"
model = "kimi-for-coding"
max_context_size = 262144
```

## 架构

```
前端 (React/TS)                          后端 (Rust/Tauri)
─────────────────────────────────────────────────────────────────
AppId += "kimi"        ───────────────→  AppType += Kimi
AppSwitcher 显示 Kimi  ───────────────→  ProviderService::switch()
Provider Presets       ───────────────→  Database (providers 表)
切换 Provider          ───────────────→  kimi_config::write_kimi_live()
                                         ↓
                                    ~/.kimi-code/config.toml
                                    [providers.ccswitch]
                                    base_url / api_key 更新
```

## 后端模块 `kimi_config.rs`

新建 `src-tauri/src/kimi_config.rs`，职责：

- `get_kimi_dir()` → `~/.kimi-code`（支持 `KIMI_CONFIG_DIR` 覆盖）
- `get_kimi_config_path()` → `~/.kimi-code/config.toml`
- `read_kimi_config()` → 读取并解析 TOML (`toml_edit::DocumentMut`)
- `write_kimi_config(doc)` → 原子写入
- `write_kimi_live(base_url, api_key)` → 核心：修改 `[providers.ccswitch]`
- `ensure_default_kimi_config(base_url, api_key)` → 首次使用生成默认配置
- `validate_kimi_settings(settings)` → 校验 `env.KIMI_BASE_URL` 非空

### 切换流程

1. `ProviderService::switch_normal()` 匹配 `AppType::Kimi`
2. 提取 `settingsConfig.env.KIMI_BASE_URL` 和 `KIMI_API_KEY`
3. 调用 `kimi_config::write_kimi_live(base_url, api_key)`
4. `write_kimi_live` 内部：
   - 读取 `config.toml`（不存在则生成默认配置）
   - 用 `toml_edit` 修改/创建 `[providers.ccswitch]`
   - 原子写回文件

## 前端改动

| 文件 | 改动 |
|------|------|
| `src/lib/api/types.ts` | `AppId` 增加 `"kimi"` |
| `src/components/AppSwitcher.tsx` | `ALL_APPS` / `appIconName` / `appDisplayName` 增加 `kimi` |
| `src/config/kimiProviderPresets.ts` | **新建**，两个官方预设 |
| `src-tauri/src/app_config.rs` | `AppType` 增加 `Kimi`；`McpApps`/`SkillApps` 增加 `kimi` |
| `src-tauri/src/lib.rs` | 新增 `mod kimi_config;` |
| `src-tauri/src/kimi_config.rs` | **新建**，TOML 配置读写 |
| `src-tauri/src/services/provider/mod.rs` | `validate_provider_settings` 和 `switch_normal` 增加 `Kimi` |
| `src-tauri/src/services/provider/live.rs` | `write_live_snapshot`、`read_live_settings` 增加 `Kimi` |
| `src/i18n/locales/*.json` | 添加 Kimi 相关最小翻译集 |

## 错误处理

| 场景 | 行为 |
|------|------|
| `~/.kimi-code/config.toml` 不存在 | 自动生成最小默认配置 |
| TOML 解析失败 | 备份为 `config.toml.bak`，报错提示用户 |
| `KIMI_BASE_URL` 为空 | 切换前校验失败，提示填写 |
| 文件写入权限不足 | 返回 `AppError::io`，前端友好提示 |

## 测试策略

- 后端：为 `kimi_config.rs` 添加单元测试（TOML 读写、provider 更新、默认配置生成）
- 前端：验证 `AppSwitcher` 正确显示 Kimi 图标和名称
- 集成：手动验证切换 provider 后 `~/.kimi-code/config.toml` 内容正确

## 文件清单

### 新增文件
- `src/config/kimiProviderPresets.ts`
- `src-tauri/src/kimi_config.rs`
- `docs/superpowers/specs/2026-05-18-kimi-support-design.md`

### 修改文件
- `src/lib/api/types.ts`
- `src/components/AppSwitcher.tsx`
- `src-tauri/src/app_config.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/services/provider/mod.rs`
- `src-tauri/src/services/provider/live.rs`
- `src/i18n/locales/zh.json`
- `src/i18n/locales/en.json`
- `src/i18n/locales/ja.json`
