# Claude Code & Codex 供应商切换器

[![点击下载最新版 - 主界面预览](screenshots/main.png)](docs/downloads/CC%20Switch_3.4.0_aarch64.dmg)

## 📥 [点击下载-> 即可使用测试key的版本](docs/downloads/CC%20Switch_3.4.0_aarch64.dmg)

[![Version](https://img.shields.io/badge/version-3.4.0-blue.svg)](https://github.com/farion1231/cc-switch/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/farion1231/cc-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

一个用于管理和切换 Claude Code 与 Codex 不同供应商配置的桌面应用。

> v3.4.0 ：新增 i18next 国际化（还有部分未完成）、对新模型（qwen-3-max, GLM-4.6, DeepSeek-V3.2-Exp）的支持、Claude 插件、单实例守护、托盘最小化及安装器优化等。

> v3.3.0 ：VS Code Codex 插件一键配置/移除（默认自动同步）、Codex 通用配置片段与自定义向导增强、WSL 环境支持、跨平台托盘与 UI 优化。（该 VS Code 写入功能已在 v3.4.x 停用）

> v3.2.0 ：全新 UI、macOS系统托盘、内置更新器、原子写入与回滚、改进暗色样式、单一事实源（SSOT）与一次性迁移/归档。

> v3.1.0 ：新增 Codex 供应商管理与一键切换，支持导入当前 Codex 配置为默认供应商，并在内部配置从 v1 → v2 迁移前自动备份（详见下文“迁移与归档”）。

> v3.0.0 重大更新：从 Electron 完全迁移到 Tauri 2.0，应用体积显著降低、启动性能大幅提升。

## 功能特性（v3.4.0+）

### 🆕 最新增强功能

- **🔍 智能搜索**：支持按供应商名称或 API 地址实时搜索过滤，快速定位目标供应商
- **🔄 多维度排序**：
  - **顺序排序**：按添加时间顺序显示（默认）
  - **延迟排序**：测试成功的供应商按延迟从低到高排序，快速找到最快的服务
  - **错误排序**：测试失败的供应商优先显示，便于快速定位和修复问题
- **⚡ 并发批量测试**：一键测试所有供应商，采用并发执行大幅提升测试速度，测试完成后自动按延迟排序
- **🌐 完整国际化支持**：所有新功能完整支持中英文切换，包括搜索提示、排序按钮、测试状态等

### v3.4.0 原有特性

- **国际化与语言切换**：内置 i18next，默认显示中文，可在设置中快速切换到英文，界面文文案自动实时刷新。
- **Claude 插件同步**：内置按钮可一键应用或恢复 Claude 插件配置，切换供应商后立即生效。
- **VS Code Codex 设置停用**：由于新版 Codex 插件无需修改 `settings.json`，应用不再写入 VS Code 设置，避免潜在冲突。
- **供应商预设扩展**：新增 DeepSeek--V3.2-Exp、Qwen3-Max、GLM-4.6 等最新模型。
- **系统托盘与窗口行为**：窗口关闭可最小化到托盘，macOS 支持托盘模式下隐藏/显示 Dock，托盘切换时同步 Claude/Codex/插件状态。
- **单实例**：保证同一时间仅运行一个实例，避免多开冲突。
- **UI 与安装体验优化**：设置面板改为可滚动布局并加入保存图标，按钮宽度与状态一致性加强，Windows MSI 安装默认写入 per-user LocalAppData 并改进组件跟踪，Windows 便携版现在指向最新 release 页面，不再自动更为为安装版。

## 界面预览

### 主界面

![主界面](screenshots/main.png)

### 添加供应商

![添加供应商](screenshots/add.png)

## 下载安装

### 系统要求

- **Windows**: Windows 10 及以上
- **macOS**: macOS 10.15 (Catalina) 及以上
- **Linux**: Ubuntu 20.04+ / Debian 11+ / Fedora 34+ 等主流发行版

### Windows 用户

从 [Releases](../../releases) 页面下载最新版本的 `CC-Switch-Setup.msi` 安装包或者 `CC-Switch-Windows-Portable.zip` 绿色版。

### macOS 用户

从 [Releases](../../releases) 页面下载 `CC-Switch-macOS.zip` 解压使用。

> **注意**：由于作者没有苹果开发者账号，首次打开可能出现"未知开发者"警告，请先关闭，然后前往"系统设置" → "隐私与安全性" → 点击"仍要打开"，之后便可以正常打开

### Linux 用户

从 [Releases](../../releases) 页面下载最新版本的 `.deb` 包或者 `AppImage`安装包。

## 使用说明

### 基本操作

1. 点击"添加供应商"添加你的 API 配置
2. 切换方式：
   - 在主界面选择供应商后点击切换
   - 或通过"系统托盘（菜单栏）"直接选择目标供应商，立即生效
3. 切换会写入对应应用的"live 配置文件"（Claude：`settings.json`；Codex：`auth.json` + `config.toml`）
4. 重启或新开终端以确保生效
5. 若需切回官方登录，在预设中选择"官方登录"并切换即可；重启终端后按官方流程登录

### 新功能使用指南

#### 搜索供应商
- 在供应商列表顶部的搜索框中输入关键词
- 支持搜索供应商名称和 API 地址
- 实时过滤显示匹配结果
- 点击搜索框右侧的 ❌ 按钮可快速清除搜索

#### 排序功能
点击工具栏的排序按钮可在三种模式间循环切换：
- **顺序排序**（默认）：按供应商添加顺序显示
- **延迟排序**：测试成功的供应商按响应延迟从低到高排序，帮助你找到最快的服务
- **错误排序**：测试失败的供应商优先显示，方便快速定位和修复问题

#### 批量测试
- 点击"测试所有"按钮可并发测试所有供应商
- 测试过程中会实时显示每个供应商的测试状态
- 测试完成后自动切换到延迟排序模式
- 相比逐个测试，并发模式可大幅缩短总测试时间

### 检查更新

- 在“设置”中点击“检查更新”，若内置 Updater 配置可用将直接检测与下载；否则会回退打开 Releases 页面

### Codex 说明（SSOT）

- 配置目录：`~/.codex/`
  - live 主配置：`auth.json`（必需）、`config.toml`（可为空）
- API Key 字段：`auth.json` 中使用 `OPENAI_API_KEY`
- 切换行为（不再写“副本文件”）：
  - 供应商配置统一保存在 `~/.cc-switch/config.json`
  - 切换时将目标供应商写回 live 文件（`auth.json` + `config.toml`）
  - 采用“原子写入 + 失败回滚”，避免半写状态；`config.toml` 可为空
- 导入默认：当该应用无任何供应商时，从现有 live 主配置创建一条默认项并设为当前
- 官方登录：可切换到预设“Codex 官方登录”，重启终端后按官方流程登录

### Claude Code 说明（SSOT）

- 配置目录：`~/.claude/`
  - live 主配置：`settings.json`（优先）或历史兼容 `claude.json`
- API Key 字段：`env.ANTHROPIC_AUTH_TOKEN`
- 切换行为（不再写“副本文件”）：
  - 供应商配置统一保存在 `~/.cc-switch/config.json`
  - 切换时将目标供应商 JSON 直接写入 live 文件（优先 `settings.json`）
  - 编辑当前供应商时，先写 live 成功，再更新应用主配置，保证一致性
- 导入默认：当该应用无任何供应商时，从现有 live 主配置创建一条默认项并设为当前
- 官方登录：可切换到预设“Claude 官方登录”，重启终端后可使用 `/login` 完成登录

### 迁移与归档（自 v3.2.0 起）

- 一次性迁移：首次启动 3.2.0 及以上版本会扫描旧的“副本文件”并合并到 `~/.cc-switch/config.json`
  - Claude：`~/.claude/settings-*.json`（排除 `settings.json` / 历史 `claude.json`）
  - Codex：`~/.codex/auth-*.json` 与 `config-*.toml`（按名称成对合并）
- 去重与当前项：按“名称（忽略大小写）+ API Key”去重；若当前为空，将 live 合并项设为当前
- 归档与清理：
  - 归档目录：`~/.cc-switch/archive/<timestamp>/<category>/...`
  - 归档成功后删除原副本；失败则保留原文件（保守策略）
- v1 → v2 结构升级：会额外生成 `~/.cc-switch/config.v1.backup.<timestamp>.json` 以便回滚
- 注意：迁移后不再持续归档日常切换/编辑操作，如需长期审计请自备备份方案

## 开发

### 环境要求

- Node.js 18+
- pnpm 8+
- Rust 1.75+
- Tauri CLI 2.0+

### 开发命令

```bash
# 安装依赖
pnpm install

# 开发模式（热重载）
pnpm dev

# 类型检查
pnpm typecheck

# 代码格式化
pnpm format

# 检查代码格式
pnpm format:check

# 构建应用
pnpm build

# 构建调试版本
pnpm tauri build --debug
```

### Rust 后端开发

```bash
cd src-tauri

# 格式化 Rust 代码
cargo fmt

# 运行 clippy 检查
cargo clippy

# 运行测试
cargo test
```

## 技术栈

- **[Tauri 2](https://tauri.app/)** - 跨平台桌面应用框架（集成 updater/process/opener/log/tray-icon）
- **[React 18](https://react.dev/)** - 用户界面库
- **[TypeScript](https://www.typescriptlang.org/)** - 类型安全的 JavaScript
- **[Vite](https://vitejs.dev/)** - 极速的前端构建工具
- **[Rust](https://www.rust-lang.org/)** - 系统级编程语言（后端）

## 项目结构

```
├── src/                   # 前端代码 (React + TypeScript)
│   ├── components/       # React 组件
│   ├── config/          # 预设供应商配置
│   ├── lib/             # Tauri API 封装
│   └── utils/           # 工具函数
├── src-tauri/            # 后端代码 (Rust)
│   ├── src/             # Rust 源代码
│   │   ├── commands.rs  # Tauri 命令定义
│   │   ├── config.rs    # 配置文件管理
│   │   ├── provider.rs  # 供应商管理逻辑
│   │   └── store.rs     # 状态管理
│   ├── capabilities/    # 权限配置
│   └── icons/           # 应用图标资源
└── screenshots/          # 界面截图
```

## 更新日志

查看 [CHANGELOG.md](CHANGELOG.md) 了解版本更新详情。

## Electron 旧版

[Releases](../../releases) 里保留 v2.0.3 Electron 旧版

如果需要旧版 Electron 代码，可以拉取 electron-legacy 分支

## 贡献

欢迎提交 Issue 和 Pull Request！

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=farion1231/cc-switch&type=Date)](https://www.star-history.com/#farion1231/cc-switch&Date)

## License

MIT © Jason Young
