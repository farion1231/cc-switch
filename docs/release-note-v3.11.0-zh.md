# CC Switch v3.11.0

> OpenClaw 支持、会话管理器、备份管理与 50+ 项改进

**[English →](release-note-v3.11.0-en.md) | [日本語版 →](release-note-v3.11.0-ja.md)**

---

## 概览

CC Switch v3.11.0 是一次大规模更新，新增第五个应用 **OpenClaw** 的完整管理支持，同时带来全新的**会话管理器**和**备份管理**功能。此外，**Oh My OpenCode (OMO) 集成**、供应商切换的**部分键值合并**架构升级、**设置页面重构**等多项改进使整体体验更加完善。

**发布日期**：2026-02-26

**更新规模**：147 commits | 274 files changed | +32,179 / -5,467 lines

---

## 重点内容

- **OpenClaw 支持**：第五个受管理应用，含 13 个供应商预设、Env/Tools/AgentsDefaults 配置编辑器、Workspace 文件管理
- **会话管理器**：浏览五个应用的历史会话，支持目录导航和会话内搜索
- **备份管理**：独立备份面板，可配置策略、定时备份、迁移前自动备份
- **Oh My OpenCode 集成**：完整 OMO 配置管理，支持 OMO Slim 轻量模式
- **部分键值合并（⚠️ 破坏性变更）**：供应商切换改为仅替换供应商相关字段，保留用户的其余设置；"通用配置片段"功能因此移除
- **设置页面重构**：5 标签页布局，代码量减少约 40%
- **6 组新供应商预设**：AWS Bedrock、SSAI Code、CrazyRouter、AICoding 等
- **Thinking Budget Rectifier**：代理矫正器，更精细的 thinking budget 控制
- **主题切换动画**：圆形揭示过渡动画，视觉体验升级
- **WebDAV 自动同步**：支持自动同步与大文件防护

---

## 主要功能

### OpenClaw 支持（新增第五应用）

CC Switch 新增对 OpenClaw 的完整管理支持，这是继 Claude Code、Codex、Gemini CLI、OpenCode 之后的第五个受管理应用。

- **供应商管理**：新增、编辑、切换、删除 OpenClaw 供应商，含 13 个内置预设
- **配置编辑器**：Env（环境变量）、Tools（工具）、AgentsDefaults（代理默认值）三个专属配置面板
- **Workspace 面板**：支持 HEARTBEAT/BOOTSTRAP/BOOT 文件管理及每日记忆
- **Additive 叠加模式**：支持配置叠加而非覆盖
- **默认模型按钮**：一键填充推荐模型，添加供应商时自动将建议模型注册到 allowlist
- **品牌与交互**：专属品牌图标、应用切换淡入淡出过渡动画
- **深链接支持**：通过 URL 导入 OpenClaw 供应商配置
- **完整国际化**：中/英/日三语全面支持

### 会话管理器 Sessions

全新的会话管理器，帮助你浏览和检索历史会话记录。

- 支持浏览 Claude Code、Codex、Gemini CLI、OpenCode、OpenClaw 五个应用的历史会话（#867，感谢 @TinsFox）
- 目录导航和会话内搜索
- 进入会话页面时默认过滤为当前应用，快速定位
- 并行目录扫描 + 头尾 JSONL 读取，优化加载性能

### 备份管理 Backup

独立的备份管理面板，让数据安全更有保障。

- 可配置备份策略：最大备份数量、自动清理规则
- 运行时每小时定期自动备份
- 数据库迁移前自动备份，带回填警告提示
- 支持备份重命名和删除（含确认对话框）
- 备份文件名使用本地时间，更直观

### Oh My OpenCode (OMO) 集成

完整的 Oh My OpenCode 配置文件管理。

- Agent 模型选择、Category 配置、推荐模型填充（#972，感谢 @yovinchen）
- 改进 Agent 模型选择 UX，修复 lowercase key 问题（#1004，感谢 @yovinchen）
- OMO Slim 轻量模式支持
- OMO 与 OMO Slim 互斥切换（数据库层级强制保证一致性）

### 工作空间 Workspace

- 每日记忆文件全文搜索，按日期排序
- 目录路径可点击跳转，快速打开文件位置

### 工具栏 Toolbar

- AppSwitcher 根据窗口宽度自动折叠为紧凑模式
- 紧凑模式切换平滑过渡动画

### 设置 Settings

- 代理和用量功能新增首次使用确认对话框，避免误操作
- 新增 `enableLocalProxy` 开关，控制主页代理 UI 显示
- 更精细的本地环境检查：CLI 工具版本检测（#870，感谢 @kv-chiu）、Volta 路径检测（#969，感谢 @myjustify）

### 供应商预设 Preset

- **AWS Bedrock**：支持 AKSK 和 API Key 两种认证方式（#1047，感谢 @keithyt06）
- **SSAI Code**：合作伙伴预设，覆盖五端
- **CrazyRouter**：合作伙伴预设及专属图标
- **AICoding**：合作伙伴预设及推广文案
- 更新国内模型供应商预设至最新版本
- Qwen Coder 重命名为百炼 (Bailian)（#965，感谢 @zhu-jl18）

### 其他新功能

- **Thinking Budget Rectifier**：代理矫正器，更精细地控制 thinking budget 分配（#1005，感谢 @yovinchen）
- **WebDAV 自动同步**：支持自动同步配置，并增加大文件防护（#923，感谢 @clx20000410；#1043，感谢 @SaladDay）
- **主题切换动画**：圆形揭示过渡动画，视觉体验更流畅（#905，感谢 @funnytime75）
- **Claude 配置编辑器快速开关**：快速切换常用配置项（#1012，感谢 @JIA-ss）
- **动态端点提示**：根据 API 格式选择动态显示端点提示文本（#860，感谢 @zhu-jl18）
- **用量仪表盘增强**：自动刷新、更强健的数据格式化（#942，感谢 @yovinchen）
- **新增定价数据**：claude-opus-4-6 和 gpt-5.3-codex（#943，感谢 @yovinchen）
- **静默启动优化**：静默启动选项仅在开机启动开启时显示

---

## 架构改进

### 部分键值合并（⚠️ 破坏性变更）

供应商切换从全量配置覆写改为部分键值合并策略（#1098）。

**变更前**：切换供应商时，整个 `settings_config` 会覆写到 live 配置文件。这意味着用户在 live 文件中手动添加的非供应商设置（插件配置、MCP 配置、权限设置等）会在每次切换时丢失。为了弥补这个问题，之前版本提供了"通用配置片段"功能，让用户定义每次切换时都会合并的公共配置。

**变更后**：切换供应商时，仅替换供应商相关的键值（API Key、端点、模型等），用户的其余设置完整保留。因此"通用配置片段"功能不再需要，已被移除。

**影响与迁移**：
- 如果你之前**没有使用**通用配置片段功能，此变更对你完全透明，切换体验只会更好
- 如果你之前**使用了**通用配置片段功能来保留自定义设置（如 MCP 配置、权限等），升级后这些设置会在切换时自动保留，无需额外操作
- 如果你利用通用配置片段做其他用途（如在切换时注入额外配置），请在升级后手动将这些配置写入 live 配置文件中

此次重构删除了 6 个前端文件（3 个组件 + 3 个 hooks）、约 150 行后端死代码。

### 手动导入替代自动导入

启动时不再自动导入外部配置，改为手动点击"导入当前配置"按钮，避免意外覆盖用户数据。

### OMO Variant 参数化

通过 `OmoVariant` 结构体参数化消除 OMO 模块约 250 行重复代码。

### OMO 公共配置移除

删除二层合并系统，减少约 1,733 行代码，简化架构。

### ProviderForm 拆分

ProviderForm 组件从 2,227 行减至 1,526 行，提取 5 个独立模块（opencodeFormUtils、useOmoModelSource、useOpencodeFormState、useOmoDraftState、useOpenclawFormState），可维护性显著提升。

### MCP/Skills 共享组件

提取 AppCountBar、AppToggleGroup、ListItemRow 等共享组件，减少 MCP 和 Skills 面板的重复代码（#897，感谢 @PeanutSplash）。

### 设置页面重构

设置页面重构为 5 标签页布局（通用 | 代理 | 高级 | 用量 | 关于），SettingsPage 代码从约 716 行减至约 426 行。

### 其他改进

- 终端统一：全局设置统一终端选择，新增 WezTerm 支持
- Claude 模型引用从 4.5 更新到 4.6

---

## Bug 修复

### 严重修复

- **Windows 主目录回归**：恢复默认主目录解析，防止 Git/MSYS 环境下数据库路径变更导致数据"丢失"
- **Linux 白屏**：禁用 AMD GPU 的 WebKitGTK 硬件加速，解决部分 Linux 系统启动白屏问题（#986，感谢 @ThendCN）
- **OpenAI Beta 参数**：不再为 `/v1/chat/completions` 添加 `?beta=true`，修复 Nvidia 等使用 OpenAI Chat 格式的供应商请求失败（#1052，感谢 @jnorthrup）
- **健康检查认证**：尊重供应商 `auth_mode` 设置，避免仅支持 Bearer 认证的代理服务健康检查失败（#824，感谢 @Jassy930）

### 供应商预设修复

- 修复 OpenClaw `/v1` 前缀双重路径问题
- Opus 定价修正（$15/$75 → $5/$25）并升级到 4.6
- AIGoCode URL 统一为 `https://api.aigocode.com`
- Zhipu GLM 移除过时合作伙伴状态
- 新建 Claude 供应商时 API Key 输入框可见性恢复
- 非活跃供应商隐藏快速开关，显示上下文感知的 JSON 编辑器提示

### OMO 修复

- omo-slim 分类检查补齐（add/form/mutation 路径）
- OMO Slim 供应商变更后正确失效查询缓存
- OMO agent/category 推荐模型与上游源同步
- "填充推荐"按钮失败时增加 toast 反馈
- 移除 OMO/OMO Slim 最后一个供应商的删除限制
- OpenCode 未配置模型时拒绝保存（#932，感谢 @yovinchen）

### OpenClaw 修复

- 修复 25 个缺失 i18n key、替换 key={index} 为稳定 ID、深链接 additive 合并等代码审查问题
- EnvPanel 健壮性增强（NaN 守卫、使用条目键名而非数组索引）
- i18n 重复键合并，恢复供应商表单翻译

### 平台修复

- Windows 静默启动时窗口闪烁（#901，感谢 @funnytime75）
- 标题栏暗黑模式跟随主题（#903，感谢 @funnytime75）
- Windows Skills 路径分隔符匹配（#868，感谢 @stmoonar）
- WSL 辅助函数条件编译

### UI 修复

- 工具栏高度裁切导致 AppSwitcher 被遮挡
- 有新版本时显示更新徽章而非绿色对勾
- 仅 Claude/Codex 应用显示会话管理器按钮
- SQL 导入/导出卡片暗黑模式样式统一（#1067，感谢 @SaladDay）

### 其他修复

- 会话管理器硬编码中文字符串替换为 i18n key
- Skill 文档 URL 分支和路径修正（#977，感谢 @yovinchen）
- OpenCode install.sh 安装路径检测补齐（#988，感谢 @zhu-jl18）
- Skill ZIP 符号链接解析修复（#1040，感谢 @yovinchen）
- MCP 表单补齐 OpenCode 复选框（#1026，感谢 @yovinchen）
- useProvidersQuery 中自动导入副作用移除

---

## 性能优化

- 会话面板并行目录扫描 + 头尾 JSONL 读取，大幅提升会话列表加载速度
- 移除 Tauri 本地 IPC 不必要的 query cache，减少内存占用

---

## 文档

- 赞助商更新：SSSAiCode、Crazyrouter、AICoding、Right Code、MiniMax
- 新增用户手册（#979，感谢 @yovinchen）

---

## 说明与注意事项

- **OpenClaw 为新支持的应用**：需要先安装 OpenClaw CLI 才能使用相关功能。
- **⚠️ 通用配置片段功能已移除**：由于供应商切换改为部分键值合并（仅替换 API Key、端点、模型等字段），用户的其余设置会自动保留，"通用配置片段"功能不再需要。详见上方"架构改进"章节的迁移说明。
- **自动导入已改为手动**：启动时不再自动导入外部配置，请在需要时手动点击"导入当前配置"。
- **OMO 与 OMO Slim 互斥**：同一时间只能启用其中一个，切换时另一个会自动禁用。
- **备份功能默认开启**：运行时每小时自动备份，可在备份面板调整策略。

---

## 特别感谢

感谢以下贡献者为本版本做出的贡献！

@TinsFox @keithyt06 @kv-chiu @SaladDay @jnorthrup @JIA-ss @clx20000410 @ThendCN @yovinchen @zhu-jl18 @myjustify @funnytime75 @PeanutSplash @Jassy930 @stmoonar

---

## 下载与安装

访问 [Releases](https://github.com/farion1231/cc-switch/releases/latest) 下载对应版本。

### 系统要求

| 系统    | 最低版本                      | 架构                                |
| ------- | ----------------------------- | ----------------------------------- |
| Windows | Windows 10 及以上             | x64                                 |
| macOS   | macOS 10.15 (Catalina) 及以上 | Intel (x64) / Apple Silicon (arm64) |
| Linux   | 见下表                        | x64                                 |

### Windows

| 文件                                     | 说明                                |
| ---------------------------------------- | ----------------------------------- |
| `CC-Switch-v3.11.0-Windows.msi`          | **推荐** - MSI 安装包，支持自动更新 |
| `CC-Switch-v3.11.0-Windows-Portable.zip` | 便携版，解压即用，不写入注册表      |

### macOS

| 文件                             | 说明                                                      |
| -------------------------------- | --------------------------------------------------------- |
| `CC-Switch-v3.11.0-macOS.zip`    | **推荐** - 解压后拖入 Applications 即可，Universal Binary |
| `CC-Switch-v3.11.0-macOS.tar.gz` | 用于 Homebrew 安装和自动更新                              |

> **注意**：由于作者没有苹果开发者账号，首次打开可能出现"未知开发者"警告，请先关闭，然后前往"系统设置" → "隐私与安全性" → 点击"仍要打开"，之后便可以正常打开

### Homebrew（macOS）

```bash
brew tap farion1231/ccswitch
brew install --cask cc-switch
```

更新：

```bash
brew upgrade --cask cc-switch
```

### Linux

| 发行版                                  | 推荐格式    | 安装方式                                                               |
| --------------------------------------- | ----------- | ---------------------------------------------------------------------- |
| Ubuntu / Debian / Linux Mint / Pop!\_OS | `.deb`      | `sudo dpkg -i CC-Switch-*.deb` 或 `sudo apt install ./CC-Switch-*.deb` |
| Fedora / RHEL / CentOS / Rocky Linux    | `.rpm`      | `sudo rpm -i CC-Switch-*.rpm` 或 `sudo dnf install ./CC-Switch-*.rpm`  |
| openSUSE                                | `.rpm`      | `sudo zypper install ./CC-Switch-*.rpm`                                |
| Arch Linux / Manjaro                    | `.AppImage` | 添加执行权限后直接运行，或使用 AUR                                     |
| 其他发行版 / 不确定                     | `.AppImage` | `chmod +x CC-Switch-*.AppImage && ./CC-Switch-*.AppImage`              |
