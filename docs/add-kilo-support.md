# CC Switch 增加 Kilo 支持 — 设计方案

## 背景

Kilo 是基于 OpenCode 开发的 AI 编码工具，配置文件路径和 schema 有差异，其余与 OpenCode 一致。

| 差异项 | OpenCode | Kilo |
|--------|----------|------|
| 配置路径 | `~/.config/opencode/opencode.json` | `~/.config/kilo/kilo.jsonc` |
| Schema | `https://opencode.ai/config.json` | `https://app.kilo.ai/config.json` |
| 数据库 | `~/.local/share/opencode/opencode.db` | `~/.local/share/kilo/kilo.db` |

---

## 设计决策汇总

| 决策点 | 结论 |
|--------|------|
| AppType | 独立 `AppType::Kilo` variant |
| 配置路径 | `~/.config/kilo/kilo.jsonc` |
| Schema | `https://app.kilo.ai/config.json` |
| 配置格式 | json5 读，JSON 写，文件名保持 `kilo.jsonc` |
| Provider 预设 | 共享 OpenCode 预设数组，写入时替换 schema |
| MCP | 支持，格式与 OpenCode 一致（`stdio`→`local`，`sse/http`→`remote`，`env`→`environment`） |
| Skills 目录 | `~/.config/kilo/skills/` |
| Prompts 文件 | `~/.config/kilo/AGENTS.md` |
| OMO 插件 | 不支持（第一版） |
| Proxy Takeover | 不支持（第一版） |
| Provider 模式 | additive mode（所有 provider 同时写入配置） |
| DB enabled 列 | 共用 `enabled_opencode` 列，Kilo 无独立列 |
| 前端 McpApps/SkillApps | 新增 `kilo` 字段，`is_enabled_for(Kilo)` / `set_enabled_for(Kilo)` 代理到 `opencode` |
| i18n | 复用 `opencode.*` key，仅 `appDisplayName` 显示为 "Kilo" |
| Tauri 命令 | 新增 `import_kilo_providers_from_live` 和 `get_kilo_live_provider_ids` |
| Universal Provider | 不支持 |
| Settings | 新增 `kiloConfigDir` override 字段 |

---

## 改动清单

### 1. 后端：AppType 枚举与核心注册（`src-tauri/src/app_config.rs`）

- `AppType` 枚举新增 `Kilo` variant
- `as_str()` 新增 `AppType::Kilo => "kilo"`
- `FromStr` 新增 `"kilo"` 解析分支
- `all()` 加入 `AppType::Kilo`
- `is_additive_mode()` 新增 `AppType::Kilo`（与 OpenCode/OpenClaw/Hermes 同组）
- `McpApps` 结构体新增 `pub kilo: bool`，`is_enabled_for(Kilo)` 代理到 `self.opencode`，`set_enabled_for(Kilo)` 设置 `self.opencode`
- `SkillApps` 同上
- `McpRoot` 新增 `pub kilo: McpConfig`
- `PromptRoot` 新增 `pub kilo: PromptConfig`
- `CommonConfigSnippets` 新增 `pub kilo: Option<String>`
- `MultiAppConfig::default()` 插入 `"kilo"` 初始值
- `MultiAppConfig::mcp_for/mcp_for_mut()` 新增 Kilo match arm

### 2. 后端：新建 Kilo 配置模块（`src-tauri/src/kilo_config.rs`）

参考 `opencode_config.rs` 创建 thin wrapper：

- `get_kilo_dir()` → 优先 `settings.kilo_config_dir`，否则 `~/.config/kilo`
- `get_kilo_config_path()` → `<kilo_dir>/kilo.jsonc`
- `get_kilo_db_path()` → `~/.local/share/kilo/kilo.db`（支持 `KILO_DB` 环境变量覆盖）
- `get_kilo_data_dir()` → `~/.local/share/kilo`
- `read_kilo_config()` → 用 json5 解析，默认返回 `{"$schema": "https://app.kilo.ai/config.json"}`
- `write_kilo_config()` → 用标准 JSON 写入
- `get_providers()` / `set_provider()` / `remove_provider()` → 与 OpenCode 相同逻辑
- `get_typed_providers()` / `set_typed_provider()` → 与 OpenCode 相同
- `get_mcp_servers()` / `set_mcp_server()` / `remove_mcp_server()` → 与 OpenCode 相同
- 不需要 OMO 插件相关函数（`add_plugin` / `remove_plugins_by_prefixes`）

在 `src-tauri/src/lib.rs` 注册模块：
```rust
mod kilo_config;
```

### 3. 后端：Provider 读写（`src-tauri/src/services/provider/live.rs`）

- `write_live_snapshot()` 新增 `AppType::Kilo` match arm，复用 OpenCode 写入逻辑，调用 `kilo_config::set_typed_provider`
- `read_live_settings()` 新增 `AppType::Kilo` match arm，调用 `kilo_config::read_kilo_config`
- `provider_exists_in_live_config()` 新增 Kilo arm
- `sync_all_providers_to_live()` 写入时替换 `$schema` 为 `https://app.kilo.ai/config.json`
- 新增 `remove_kilo_provider_from_live()` 和 `import_kilo_providers_from_live()` 函数

### 4. 后端：Prompt 文件路径（`src-tauri/src/prompt_files.rs`）

- base_dir: `AppType::Kilo => kilo_config::get_kilo_dir()`
- filename: `AppType::Kilo => "AGENTS.md"`

### 5. 后端：MCP 管理（`src-tauri/src/mcp/`）

新建 `src-tauri/src/mcp/kilo.rs`：

- 格式转换逻辑与 `opencode.rs` 完全一致（`stdio`→`local`，`sse/http`→`remote`，`env`→`environment`）
- `sync_single_server_to_kilo()` — 调用 `kilo_config::set_mcp_server`
- `remove_server_from_kilo()` — 调用 `kilo_config::remove_mcp_server`
- `import_from_kilo()` — 从 `kilo_config::get_mcp_servers()` 导入
- 在 `src-tauri/src/mcp/mod.rs` 注册模块
- 在 `services/mcp.rs` 的 sync/remove 函数中新增 Kilo 分支

### 6. 后端：Skills 管理（`src-tauri/src/services/skill.rs`）

- Skill 目录映射：`AppType::Kilo => kilo_config::get_kilo_dir()?.join("skills")`

### 7. 后端：数据库（无需迁移）

- `mcp_servers` 和 `skills` 表不新增列
- `enabled_opencode` 列同时控制 OpenCode 和 Kilo
- DAO 层无需改动

### 8. 后端：Tauri 命令（`src-tauri/src/commands/provider.rs`）

- 新增 `import_kilo_providers_from_live` 命令
- 新增 `get_kilo_live_provider_ids` 命令
- 在 `lib.rs` 中注册命令

### 9. 后端：Settings（`src-tauri/src/settings.rs` 或相关文件）

- 新增 `kilo_config_dir` 字段的 override 支持（类似 `opencode_config_dir`）

### 10. 前端：类型定义

| 文件 | 改动 |
|------|------|
| `src/lib/api/types.ts` | `AppId` 联合类型新增 `"kilo"` |
| `src/types.ts` | `VisibleApps` 新增 `kilo: boolean` |
| `src/types.ts` | `McpApps` 新增 `kilo: boolean`（代理到 `opencode`） |
| `src/types.ts` | `Settings` 新增 `kiloConfigDir?: string` |

### 11. 前端：App 切换器（`src/components/AppSwitcher.tsx`）

```typescript
// ALL_APPS
const ALL_APPS: AppId[] = [..., "kilo"];

// appIconName
kilo: "kilo",  // 需新增图标

// appDisplayName
kilo: "Kilo",
```

### 12. 前端：App 入口（`src/App.tsx`）

- `VALID_APPS` 数组新增 `"kilo"`

### 13. 前端：可见性设置（`src/components/settings/AppVisibilitySettings.tsx`）

- `APP_CONFIG` 数组新增 `{ id: "kilo", name: "Kilo", icon: "kilo" }`

### 14. 前端：目录设置（`src/hooks/useDirectorySettings.ts`）

- `DIR_REGISTRY` 新增 `kilo: { key: "kilo", defaultFolder: ".config/kilo" }`
- `SETTINGS_FIELD` 新增 `kilo: "kiloConfigDir"`
- `ResolvedDirs` 新增 `kilo: string`

### 15. 前端：Provider 列表与表单

- `ProviderList.tsx`：新增 `kiloLiveProviderIds` 查询（类似 `opencodeLiveProviderIds`）
- `ProviderCard.tsx`：additive mode 判断新增 `appId === "kilo"`
- `AddProviderDialog.tsx`：Kilo 直接显示 OpenCode 表单（无 tabs）
- `useProviderActions.ts`：Kilo 走 additive mode 分支
- `providers.ts` API：新增 `importKiloFromLive()` 和 `getKiloLiveProviderIds()`

### 16. 前端：Provider 预设（`src/config/kiloProviderPresets.ts`）

```typescript
import { opencodeProviderPresets } from "./opencodeProviderPresets";

// 共享 OpenCode 预设数组，写入时由后端替换 schema
export const kiloProviderPresets = opencodeProviderPresets;
```

### 17. 前端：i18n

- 不新增 `kilo.*` key，复用 `opencode.*` 翻译
- 仅 `appDisplayName` 中显示 "Kilo"

### 18. 前端：图标资源

- 在图标目录中新增 Kilo 的 SVG/PNG 图标
- 在 `src/icons/extracted/index.ts` 中导出

### 19. 前端：MCP 面板（`src/components/mcp/UnifiedMcpPanel.tsx`）

- 初始状态新增 `kilo: 0`

### 20. 后端：Proxy（无需改动）

- Kilo 不支持 proxy takeover，和 OpenCode 一致
- `services/proxy.rs` 中 `AppType::Kilo` 硬编码为 `false`

---

## 预估工作量

- 新建文件：2 个（`kilo_config.rs`、`mcp/kilo.rs`）+ 可选 `kiloProviderPresets.ts`
- 修改文件：约 15-18 个
- 新增数据库列：0（共用 `enabled_opencode`）
- 预估代码量：约 500-700 行（含样板代码）
