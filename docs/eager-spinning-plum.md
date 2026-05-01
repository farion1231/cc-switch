# 配置目录多租户（Profile）设计

## Context

当前每个工具只有一个配置目录覆盖路径，用户需要在 Windows 主机和 WSL 等不同环境间切换配置时，每次都要手动重新填写路径。本方案引入 **Profile（环境配置集）** 概念，一个 Profile 包含所有 6 个工具的配置目录映射，用户可一键切换环境。

## 数据结构

### 前端 `src/types.ts`

```typescript
export interface ConfigDirProfile {
  id: string;           // 唯一标识，如 "windows", "wsl-ubuntu"
  name: string;         // 显示名称，如 "Windows 本地"
  // 配置目录覆盖路径
  claude?: string;
  codex?: string;
  gemini?: string;
  opencode?: string;
  openclaw?: string;
  hermes?: string;
  // 每个工具当前正在应用的供应商配置 ID（profile 级别独立记录）
  currentProviderClaude?: string;
  currentProviderCodex?: string;
  currentProviderGemini?: string;
  currentProviderOpencode?: string;
  currentProviderOpenclaw?: string;
  currentProviderHermes?: string;
}
```

`Settings` 接口新增字段：
```typescript
configDirProfiles?: ConfigDirProfile[];     // Profile 列表
activeConfigDirProfileId?: string;           // 当前激活的 Profile ID
// 注意：currentProvider_* 字段已移入 ConfigDirProfile 内部，不再作为 Settings 顶层字段
```

同时保留原有的 `claudeConfigDir` 等 6 个字段用于向后兼容迁移。

### 后端 `src-tauri/src/settings.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDirProfile {
    pub id: String,
    pub name: String,
    // 配置目录覆盖路径
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub gemini: Option<String>,
    pub opencode: Option<String>,
    pub openclaw: Option<String>,
    pub hermes: Option<String>,
    // 每个工具当前正在应用的供应商配置 ID（profile 级别独立记录）
    pub current_provider_claude: Option<String>,
    pub current_provider_codex: Option<String>,
    pub current_provider_gemini: Option<String>,
    pub current_provider_opencode: Option<String>,
    pub current_provider_openclaw: Option<String>,
    pub current_provider_hermes: Option<String>,
}

// AppSettings 新增字段
pub config_dir_profiles: Vec<ConfigDirProfile>,
pub active_config_dir_profile_id: Option<String>,
```

注意：原有的 `current_provider_*` 顶层字段仍保留用于向后兼容（无 Profile 场景），但一旦 Profile 体系启用，`get_current_provider()` 将优先从 active profile 内部读取。

## 修改文件清单

### 后端（Rust）

1. **`src-tauri/src/settings.rs`**
   - 新增 `ConfigDirProfile` 结构体
   - `AppSettings` 新增 `config_dir_profiles` 和 `active_config_dir_profile_id` 字段
   - `normalize_paths()` 中对 profile 内各路径做 trim/去空处理
   - 修改 `get_claude_override_dir()` 等 6 个函数：从「读单个字段」改为「查找 active profile → 读 profile 中对应字段」
   - 新增 `get_active_config_dir_profile()` 内部辅助函数
   - 修改 `mutate_settings` 确保 profile 变更后内存缓存同步

2. **`src-tauri/src/commands/config.rs`**
   - 新增 `get_config_dir_profiles()` 命令 — 返回所有 Profile
   - 新增 `upsert_config_dir_profile()` 命令 — 创建/更新 Profile
   - 新增 `delete_config_dir_profile()` 命令 — 删除 Profile
   - 新增 `set_active_config_dir_profile()` 命令 — 切换激活的 Profile

3. **`src-tauri/src/commands/misc.rs`**
   - 修改 `wsl_distro_for_tool()` — 从当前 active profile 中读取覆盖路径，而非旧字段
   - 添加 profile 不存在时的 fallback 逻辑

4. **`src-tauri/src/lib.rs`**
   - 注册新命令到 `invoke_handler`

### 前端（TypeScript/React）

5. **`src/types.ts`**
   - 新增 `ConfigDirProfile` 接口
   - `Settings` 接口新增 `configDirProfiles` 和 `activeConfigDirProfileId`

6. **`src/lib/api/settings.ts`**
   - 新增 `getConfigDirProfiles()`, `upsertConfigDirProfile()`, `deleteConfigDirProfile()`, `setActiveConfigDirProfile()` API 调用

7. **`src/hooks/useDirectorySettings.ts`**
   - 新增 Profile 状态管理：加载 profile 列表、切换 profile、创建/删除 profile
   - `ResolvedDirectories` 的解析改为：从当前激活的 Profile 读取路径
   - 保留 `resetAllDirectories` 等现有 API

8. **`src/components/settings/DirectorySettings.tsx`**
   - 重写 UI：顶部为 Profile 选择器（下拉选择 + 新建/删除按钮），下方为当前选中 Profile 的 6 个工具目录编辑区
   - 切换 Profile 即时生效（不需要保存）
   - Profile 编辑即时保存到后端

## 向后兼容迁移

首次加载时，如果 `configDirProfiles` 为空但旧的 `claudeConfigDir` 等字段有值：
1. 自动创建一个名为 "默认" 的 Profile，将旧字段值迁移进去（包括 `currentProvider*` 字段如果有值的话）
2. 将 `activeConfigDirProfileId` 设为该 Profile 的 ID
3. 保存 settings，旧字段保留但不再被消费（仅作为兼容备份）

## 关键流程

### 读取配置目录
```
get_claude_config_dir()
  → settings::get_active_config_dir_profile()
    → 从 AppSettings.config_dir_profiles 中查找 activeConfigDirProfileId 匹配的 Profile
    → 返回 Profile 中的 claude 字段（经 resolve_override_path 展开 ~）
  → 无 Profile 或字段为空 → fallback 到 get_home_dir().join(".claude")
```

### 读取当前供应商（profile 级别优先）
```
get_current_provider(app_type)
  → 如果有 active profile → 返回 profile 中对应的 current_provider_* 字段
  → 否则 → fallback 到 AppSettings 顶层的 current_provider_* 字段（向后兼容）
  → 最终 fallback → 数据库 is_current 字段
```

### 切换供应商（写入 profile 级别）
```
set_current_provider(app_type, provider_id)
  → 如果有 active profile → 更新 profile 中的 current_provider_* 字段
  → 否则 → 写入 AppSettings 顶层的 current_provider_* 字段（向后兼容）
```

### 切换 Profile（即时生效，恢复各工具上次使用的配置）
```
前端调用 setActiveConfigDirProfile(newProfileId)
  → 后端写入 settings.json + 更新 SETTINGS_STORE 内存缓存
  → 前端重新拉取 resolvedDirs 和 currentProvider 列表，UI 即时刷新
  → 下次任何工具读取配置目录/当前供应商时，自动使用新 Profile 中的路径和供应商 ID
  → 每个工具自动恢复到该 Profile 上次切换前使用的供应商配置
```

## 验证方案 

1. **单元测试**：`settings.rs` 中测试 profile 查找逻辑、旧字段迁移逻辑、profile 级别 current provider 读写逻辑
2. **手动测试**：
   - 创建两个 Profile（Windows 路径 / WSL UNC 路径）
   - 在 Profile A 下切换 Claude 到供应商 X，切换到 Profile B，验证 Claude 自动恢复到 Profile B 上次使用的供应商
   - 切换 Profile，验证 UI 显示的路径和当前供应商即时更新
   - 切换后执行「切换供应商」操作，验证配置写入到正确目录且 profile 的 current_provider_* 字段被更新
   - 删除 Profile 的边界情况（删除最后一个、删除当前激活的）
3. **兼容性**：用已有旧配置启动，验证自动迁移到 Profile（包括 current_provider_* 字段）
