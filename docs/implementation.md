# CC-Switch 项目实现文档

## 项目概述

CC-Switch 是一个基于 Tauri 2.0 框架开发的桌面应用，用于管理和切换 Claude Code 与 Codex 不同供应商配置。项目从 v3.0.0 开始完全迁移到 Tauri 架构，实现了体积减少 85%、启动速度提升 10 倍的重大性能改进。

## 技术架构

### 整体架构
- **前端**: React 18 + TypeScript + Vite
- **后端**: Rust + Tauri 2.0
- **配置管理**: JSON (Claude Code) + TOML (Codex)
- **状态管理**: Tauri 状态管理 + React Hooks

### 目录结构
```
cc-switch/
├── src/                          # 前端代码
│   ├── components/              # React 组件
│   │   ├── AppSwitcher.tsx      # 应用切换器
│   │   ├── ProviderList.tsx     # 供应商列表
│   │   ├── AddProviderModal.tsx # 添加供应商弹窗
│   │   └── ...
│   ├── config/                  # 预设供应商配置
│   ├── lib/                     # Tauri API 封装
│   └── utils/                   # 工具函数
├── src-tauri/                   # 后端代码
│   ├── src/
│   │   ├── commands.rs          # Tauri 命令定义
│   │   ├── config.rs            # Claude Code 配置管理
│   │   ├── codex_config.rs      # Codex 配置管理
│   │   ├── provider.rs          # 供应商数据结构
│   │   ├── store.rs             # 状态管理
│   │   └── app_config.rs        # 应用配置定义
│   └── capabilities/            # 权限配置
└── screenshots/                 # 界面截图
```

## 核心实现

### 1. 前端架构 (React + TypeScript)

#### 主应用组件 (`src/App.tsx`)
- **状态管理**: 使用 React Hooks 管理应用状态
  ```typescript
  const [activeApp, setActiveApp] = useState<AppType>("claude");
  const [providers, setProviders] = useState<Record<string, Provider>>({});
  const [currentProviderId, setCurrentProviderId] = useState<string>("");
  ```

- **核心功能**:
  - 供应商列表展示与管理
  - 应用切换 (Claude Code ↔ Codex)
  - 通知系统 (成功/错误提示)
  - 模态框管理 (添加/编辑供应商)

#### 组件化设计
- **AppSwitcher**: 应用类型切换器 (Claude Code / Codex)
- **ProviderList**: 供应商列表展示，支持切换、编辑、删除
- **AddProviderModal**: 添加供应商弹窗，支持预设和自定义配置
- **EditProviderModal**: 编辑供应商配置
- **ConfirmDialog**: 确认对话框组件

#### 预设供应商系统
位于 `src/config/` 目录:
- `providerPresets.ts`: Claude Code 预设供应商配置
- `codexProviderPresets.ts`: Codex 预设供应商配置

### 2. 后端架构 (Rust + Tauri)

#### 应用初始化 (`src-tauri/src/lib.rs`)
- **macOS 特殊处理**: 设置标题栏背景色为主界面蓝色 (#3498db)
- **自动配置导入**: 检测现有 Claude Code 配置并自动导入为默认供应商
- **状态初始化**: 创建全局应用状态管理器

#### 命令系统 (`src-tauri/src/commands.rs`)
所有前后端交互通过 Tauri 命令实现:

```rust
// 供应商管理命令
#[tauri::command]
pub async fn get_providers(state: State<AppState>, app_type: AppType) -> Result<HashMap<String, Provider>, String>
pub async fn add_provider(state: State<AppState>, provider: Provider, app_type: AppType) -> Result<bool, String>
pub async fn update_provider(state: State<AppState>, provider: Provider, app_type: AppType) -> Result<bool, String>
pub async fn delete_provider(state: State<AppState>, id: String, app_type: AppType) -> Result<bool, String>
pub async fn switch_provider(state: State<AppState>, id: String, app_type: AppType) -> Result<bool, String>

// 配置管理命令
pub async fn get_config_status(app_type: AppType) -> Result<ConfigStatus, String>
pub async fn import_default_config(state: State<AppState>, app_type: AppType) -> Result<bool, String>
pub async fn open_config_folder(handle: tauri::AppHandle, app_type: AppType) -> Result<bool, String>
```

#### 供应商数据结构 (`src-tauri/src/provider.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub settings_config: Value,  // JSON 配置内容
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderManager {
    pub providers: HashMap<String, Provider>,
    pub current: String,  // 当前激活的供应商 ID
}
```

### 3. 配置文件管理

#### Claude Code 配置 (`src-tauri/src/config.rs`)

**配置目录**: `~/.claude/`
- **主配置**: `settings.json` (优先) 或 `claude.json` (兼容旧版)
- **供应商副本**: `settings-{name}.json`
- **API Key 字段**: `env.ANTHROPIC_AUTH_TOKEN`

**核心函数**:
```rust
// 获取配置路径
pub fn get_claude_settings_path() -> PathBuf
pub fn get_provider_config_path(provider_id: &str, provider_name: Option<&str>) -> PathBuf

// 文件操作
pub fn read_json_file<T>(path: &Path) -> Result<T, String>
pub fn write_json_file<T>(path: &Path, data: &T) -> Result<(), String>
pub fn backup_config(from: &Path, to: &Path) -> Result<(), String>

// 配置导入
pub fn import_current_config_as_default() -> Result<Value, String>
```

#### Codex 配置 (`src-tauri/src/codex_config.rs`)

**配置目录**: `~/.codex/`
- **主配置**: `auth.json` (必需) + `config.toml` (可选)
- **供应商副本**: `auth-{name}.json` + `config-{name}.toml`
- **API Key 字段**: `auth.json` 中的 `OPENAI_API_KEY`

**核心函数**:
```rust
// 配置路径管理
pub fn get_codex_auth_path() -> PathBuf
pub fn get_codex_config_path() -> PathBuf
pub fn get_codex_provider_paths(provider_id: &str, provider_name: Option<&str>) -> (PathBuf, PathBuf)

// 配置备份与恢复
pub fn backup_codex_config(provider_id: &str, provider_name: &str) -> Result<(), String>
pub fn restore_codex_provider_config(provider_id: &str, provider_name: &str) -> Result<(), String>

// 供应商配置管理
pub fn save_codex_provider_config(provider_id: &str, provider_name: &str, settings_config: &Value) -> Result<(), String>
pub fn delete_codex_provider_config(provider_id: &str, provider_name: &str) -> Result<(), String>
```

### 4. 供应商切换核心逻辑

#### Claude Code 切换流程
1. **备份当前配置**: 如果存在当前供应商，将主配置备份到对应供应商副本文件
2. **验证目标配置**: 检查目标供应商配置文件是否存在
3. **复制配置**: 将目标供应商配置复制到主配置文件
4. **更新状态**: 更新当前供应商 ID 并保存应用状态

#### Codex 切换流程
1. **备份当前配置**: 备份 `auth.json` 和 `config.toml` 到对应供应商副本
2. **恢复目标配置**: 
   - 复制 `auth-{name}.json` 到 `auth.json`
   - 复制 `config-{name}.toml` 到 `config.toml` (如不存在则创建空文件)
3. **更新状态**: 更新当前供应商 ID 并保存应用状态

#### 关键实现代码 (`src-tauri/src/commands.rs:244-333`)
```rust
#[tauri::command]
pub async fn switch_provider(
    state: State<'_, AppState>,
    app_type: AppType,
    id: String,
) -> Result<bool, String> {
    // ... 获取供应商信息 ...
    
    match app_type {
        AppType::Codex => {
            // 备份当前配置
            if !manager.current.is_empty() {
                if let Some(current_provider) = manager.providers.get(&manager.current) {
                    codex_config::backup_codex_config(&manager.current, &current_provider.name)?;
                }
            }
            
            // 恢复目标供应商配置
            codex_config::restore_codex_provider_config(&id, &provider.name)?;
        }
        AppType::Claude => {
            // 备份当前配置
            if settings_path.exists() && !manager.current.is_empty() {
                if let Some(current_provider) = manager.providers.get(&manager.current) {
                    let current_provider_path = get_provider_config_path(&manager.current, Some(&current_provider.name));
                    backup_config(&settings_path, &current_provider_path)?;
                }
            }
            
            // 复制目标配置到主配置
            copy_file(&provider_config_path, &settings_path)?;
        }
    }
    
    // 更新当前供应商
    manager.current = id;
    state.save()?;
    Ok(true)
}
```

### 5. 状态管理系统 (`src-tauri/src/store.rs`)

```rust
#[derive(Debug)]
pub struct AppState {
    pub config: Arc<Mutex<AppConfig>>,
}

impl AppState {
    pub fn new() -> Self {
        let config = AppConfig::load().unwrap_or_default();
        Self {
            config: Arc::new(Mutex::new(config)),
        }
    }
    
    pub fn save(&self) -> Result<(), String> {
        let config = self.config.lock().unwrap();
        config.save()
    }
}
```

### 6. 文件安全性处理

#### 文件名清理 (`src-tauri/src/config.rs:41-50`)
```rust
pub fn sanitize_provider_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}
```

#### 目录创建保护
所有文件操作前都会确保父目录存在:
```rust
if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
}
```

### 7. 错误处理机制

- **统一错误类型**: 所有命令返回 `Result<T, String>`
- **详细错误消息**: 包含具体的失败原因和文件路径
- **前端错误展示**: 通过通知系统向用户展示错误信息
- **日志记录**: 使用 `log` crate 记录关键操作

### 8. 跨平台适配

#### macOS 特殊处理
- 设置窗口背景色匹配主题
- 使用 `objc2` 进行原生 API 调用

#### 路径处理
- 使用 `dirs` crate 获取标准用户目录
- 跨平台路径分隔符处理

## 性能特点

### 1. 启动速度优化
- Tauri 原生性能，相比 Electron 提升 10 倍启动速度
- 懒加载配置文件，只在需要时读取

### 2. 内存效率
- Rust 零成本抽象，内存使用最小化
- 配置文件缓存策略，避免重复 I/O

### 3. 体积优化
- 应用体积从 ~80MB 降至 ~12MB (85% 减少)
- 移除 Electron 运行时依赖

## 安全考虑

### 1. 文件系统安全
- 严格的文件名清理，防止路径遍历攻击
- 配置文件权限控制
- 原子性文件操作，防止配置损坏

### 2. 配置隔离
- 每个供应商独立配置文件
- 备份机制防止配置丢失
- 配置验证确保 JSON/TOML 格式正确

### 3. 状态管理安全
- 线程安全的状态管理 (Mutex)
- 配置更新原子性保证
- 错误恢复机制

## 扩展性设计

### 1. 新应用类型支持
通过 `app_config.rs` 中的 `AppType` 枚举轻松添加新的应用类型:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AppType {
    Claude,
    Codex,
    // 可以轻松添加新类型
    // NewApp,
}
```

### 2. 配置格式支持
模块化的配置管理器设计，支持不同的配置文件格式 (JSON, TOML, YAML 等)

### 3. 插件系统
基于 Tauri 的插件机制，可以轻松集成第三方功能

## 总结

CC-Switch 通过 Tauri 2.0 架构实现了高性能、跨平台的供应商配置管理工具。其核心优势包括:

1. **性能卓越**: Rust 后端 + 原生性能
2. **架构清晰**: 前后端分离，模块化设计
3. **安全可靠**: 完善的错误处理和文件安全机制
4. **易于扩展**: 支持新应用类型和配置格式
5. **用户友好**: 简洁的界面和自动化的配置导入

该项目展示了现代桌面应用开发的最佳实践，特别是在配置管理、状态同步和跨平台兼容性方面。