# CC Switch 增加 Qoder CLI 管理 — 改动分析

需要改动约 **25+ 处**，横跨 Rust 后端和 TypeScript 前端。以下按模块梳理。

---

## 1. 后端：AppType 枚举与核心注册

**文件：`src-tauri/src/app_config.rs`**

这是最核心的文件，需要改动 **8+ 处**：

| 位置 | 改动 |
|------|------|
| `AppType` 枚举（~L339） | 新增 `Qoder` variant |
| `as_str()`（~L357） | 新增 `AppType::Qoder => "qoder"` |
| `FromStr`（~L395） | 新增 `"qoder"` 解析分支 |
| `all()`（~L381） | 加入 `AppType::Qoder` |
| `is_additive_mode()`（~L373） | 决定 Qoder 是 switch 模式还是 additive 模式（取决于 Qoder 的配置结构） |
| `McpApps` 结构体（~L8） | 新增 `pub qoder: bool` 字段 |
| `McpApps::is_enabled_for/set_enabled_for`（~L24-47） | 新增 Qoder match arm |
| `SkillApps` 结构体（~L77） | 新增 `pub qoder: bool` 字段 |
| `SkillApps::is_enabled_for/set_enabled_for`（~L93-116） | 新增 Qoder match arm |
| `McpRoot` 结构体（~L253） | 新增 `pub qoder: McpConfig` 字段 |
| `PromptRoot` 结构体（~L310） | 新增 `pub qoder: PromptConfig` 字段 |
| `CommonConfigSnippets`（~L418） | 新增 `pub qoder: Option<serde_json::Value>` |
| `MultiAppConfig::default()`（~L496） | 插入 Qoder 初始值 |
| `MultiAppConfig::mcp_for/mcp_for_mut()`（~L659） | 新增 Qoder match arm |

---

## 2. 后端：新建 Qoder 配置模块

创建 `src-tauri/src/qoder_config.rs`：

```rust
// 需要实现的核心函数：
pub fn get_qoder_dir() -> Result<PathBuf, AppError>       // 配置目录，如 ~/.qoder/
pub fn get_qoder_config_path() -> Result<PathBuf, AppError>  // 配置文件路径
pub fn read_qoder_config() -> Result<String, AppError>     // 读取配置
pub fn write_qoder_config(...) -> Result<(), AppError>     // 写入配置
```

关键取决于 Qoder CLI 的配置格式：

- 如果是 JSON → 参考 `opencode_config.rs`
- 如果是 YAML → 参考 `hermes_config.rs`
- 如果是 TOML → 参考 `codex_config.rs`

然后在 `src-tauri/src/main.rs` / `lib.rs` 中注册模块：

```rust
mod qoder_config;
```

---

## 3. 后端：Provider 读写

**文件：`src-tauri/src/services/provider/live.rs`**

| 函数 | 改动 |
|------|------|
| `write_live_snapshot()`（~L726-867） | 新增 `AppType::Qoder` match arm，调用 `qoder_config` 写入逻辑 |
| `read_live_settings()`（~L1004-1115） | 新增 `AppType::Qoder` match arm，读取 Qoder 当前生效配置 |

---

## 4. 后端：Prompt 文件路径

**文件：`src-tauri/src/prompt_files.rs`**

在 `prompt_file_path()`（~L12-40）中添加两个 match arm：

```rust
// base_dir
AppType::Qoder => get_qoder_dir(),
// filename
AppType::Qoder => "AGENTS.md",  // 或 Qoder 使用的 prompt 文件名
```

---

## 5. 后端：MCP 管理

**目录：`src-tauri/src/mcp/`**

新建 `src-tauri/src/mcp/qoder.rs`，参考 `hermes.rs` 或 `opencode.rs`：

- `read_mcp_servers()` — 从 Qoder 配置读取 MCP servers
- `write_mcp_servers()` — 将 MCP servers 写入 Qoder 配置
- 在 `src-tauri/src/mcp/mod.rs` 中注册模块
- 在 `services/mcp.rs` 的 `sync_server_to_app_no_config()`（~L110）和 `remove_server_from_app()`（~L157）中新增 Qoder 分支

---

## 6. 后端：Skills 管理

**文件：`src-tauri/src/services/skill.rs`**

在 skill 目录映射函数（~L505+）中添加：

```rust
AppType::Qoder => get_qoder_dir()?.join("skills"),
```

---

## 7. 后端：数据库 Schema

**目录：`src-tauri/src/database/`**

| 文件 | 改动 |
|------|------|
| `schema.rs` | `mcp_servers` 表新增 `enabled_qoder BOOLEAN` 列；`skills` 表新增 `enabled_qoder BOOLEAN` 列 |
| 迁移逻辑 | 新增数据库迁移，ALTER TABLE 添加新列 |

---

## 8. 后端：Proxy 支持（可选）

如果需要 Proxy takeover：

- `src-tauri/src/proxy/server.rs`（~L292）— 新增 Qoder 路由
- `src-tauri/src/services/proxy.rs`（~L588）— `ProxyTakeoverStatus` 新增 Qoder 字段
- 新增 `src-tauri/src/proxy/qoder_*.rs` 处理请求转发

---

## 9. 前端：类型定义

| 文件 | 改动 |
|------|------|
| `src/lib/api/types.ts`（~L1-9） | `AppId` 联合类型新增 `"qoder"` |
| `src/types.ts`（~L260-268） | `VisibleApps` 接口新增 `qoder: boolean` |
| `src/types.ts`（~L462-470） | `McpApps` 接口新增 `qoder: boolean` |
| `src/types.ts`（~L367-378） | `Settings` 新增 `qoderConfigDir?: string` |

---

## 10. 前端：App 切换器

**文件：`src/components/AppSwitcher.tsx`**

```typescript
// ALL_APPS（~L21）
const ALL_APPS: AppId[] = [..., "qoder"];

// appIconName（~L44）
qoder: "qoder",  // 需要新增图标

// appDisplayName（~L53）
qoder: "Qoder CLI",
```

---

## 11. 前端：Provider 预设

新建 `src/config/qoderProviderPresets.ts`：

- 定义 `QoderProviderPreset` 接口
- 编写内置预设（官方 API、第三方兼容端点等）
- 参考哪个现有预设取决于 Qoder 的配置格式

如需支持 Universal Provider，还需更新 `src/config/universalProviderPresets.ts` 中的 `defaultApps`。

---

## 12. 前端：可见性设置

**文件：`src/components/settings/AppVisibilitySettings.tsx`**

在 `APP_CONFIG` 数组（~L14-30）中新增：

```typescript
{ id: "qoder", name: "Qoder CLI", icon: "qoder" },
```

---

## 13. 前端：App 入口

**文件：`src/App.tsx`**

在 `VALID_APPS` 数组（~L120-128）中新增 `"qoder"`。

---

## 14. 前端：图标资源

在图标目录中添加 Qoder CLI 的 SVG/PNG 图标文件。

---

## 15. 前端：Provider 表单组件

根据 Qoder 的配置结构，可能需要新建 `src/components/providers/forms/QoderProviderForm.tsx`，或在现有表单中增加 Qoder 分支。

---

## 建议的功能矩阵

| 功能 | 是否支持 | 说明 |
|------|---------|------|
| Provider 切换 | **必须** | 核心功能 |
| MCP | 看 Qoder 是否支持 MCP | 需要研究 Qoder 的 MCP 配置方式 |
| Skills | 看 Qoder 是否有 skills 概念 | 可能对应 Qoder 的 skills/agents |
| Prompts | 大概率支持 | 看 Qoder 是否读取类似 CLAUDE.md 的文件 |
| Proxy Takeover | 可选 | 需要知道 Qoder 的 API 协议 |

---

## 总结

核心工作量在于：

1. **搞清楚 Qoder CLI 的配置格式和目录结构** — 这决定了后端 `qoder_config.rs` 的实现方式
2. **AppType 枚举的全量 match arm 更新** — 约 15 处 Rust 代码
3. **前端类型和 UI 注册** — 约 8 处 TypeScript 代码
4. **Provider 预设定义** — 1 个新文件
5. **数据库迁移** — 新增列

如果 Qoder 的配置格式是 JSON 且目录结构类似 `~/.qoder/`，预估改动量约 **800-1200 行代码**（含预设数据），横跨 **15-20 个文件**。
