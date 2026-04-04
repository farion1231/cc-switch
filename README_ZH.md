<div align="center">

# CC Switch Legacy

### 一个为 macOS 10.15 Catalina 提供兼容支持的 CC Switch 分支

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#兼容性)
[![macOS](https://img.shields.io/badge/macOS-10.15%2B-blue.svg)](#兼容性)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

[English](README.md) | 中文 | [兼容性适配说明](docs/macos-10.15-compat.md) | [上游项目](https://github.com/farion1231/cc-switch) | [更新日志](CHANGELOG.md)

</div>

## 项目说明

这个仓库是基于 [CC Switch](https://github.com/farion1231/cc-switch) 的兼容性分支。

这个分支的目标很明确：尽量保持原版 CC Switch 的使用体验，同时让桌面应用可以在 **macOS 10.15 Catalina** 上构建和运行。上游项目当前已经不再以 10.15 为兼容目标，所以这里单独维护一个 legacy 分支。

如果你使用的是 macOS 12 及以上，而且并不需要 Catalina 兼容性，那么优先使用上游项目会更合适。

## 这个分支改了什么

相对于上游 CC Switch，这个分支主要针对老版本 macOS 做了以下兼容处理：

- 将 macOS 部署目标从 `12.0` 下调到 `10.15`
- 调整 Tauri 打包配置，使应用允许在 `macOS 10.15+` 安装运行
- 为旧版 WebKit 缺失的协议方法加入 `objc2` 开发模式兼容处理
- 将 `esbuild` 固定到 `0.21.5`，避免依赖更高版本 macOS 才有的系统符号
- 下调前端构建目标，兼容旧版 Safari / WKWebView
- 将 `smol-toml` 替换为 `@iarna/toml`，规避 Safari 13 对 `BigInt` 语法的不兼容
- 为主题监听增加 `MediaQueryList.addListener` 回退逻辑

详细适配过程见 [docs/macos-10.15-compat.md](docs/macos-10.15-compat.md)。

## 功能概览

这个分支保留了 CC Switch 的主要功能：

- 在一个桌面应用里统一管理 **Claude Code**、**Codex**、**Gemini CLI**、**OpenCode**、**OpenClaw**
- 无需手改 JSON、TOML、`.env` 文件即可导入和切换 provider
- 统一管理 **MCP**、**Prompts**、**Skills**
- 支持系统托盘快速切换 provider
- 提供使用量和费用统计视图
- 支持通过自定义配置目录或 WebDAV 做数据同步
- 支持浏览和恢复多种 CLI 工具的会话记录

## 界面预览

| 主界面 | 添加 Provider |
| :---: | :---: |
| ![Main Interface](assets/screenshots/main-en.png) | ![Add Provider](assets/screenshots/add-en.png) |

## 兼容性

### 主要目标平台

- macOS 10.15 Catalina

### 理论上也应继续可用

- macOS 11 及以上
- Windows
- Linux

这个仓库存在的主要原因就是维护 Catalina 兼容性，其他平台原则上尽量保持与上游一致。

## 快速开始

### 基本使用

1. 在主界面中添加一个 provider
2. 启用你想使用的 provider
3. 按对应 CLI 的要求重启终端或工具进程
4. 需要快速切换时可直接使用托盘菜单

### 文档入口

- [macOS 10.15 兼容适配说明](docs/macos-10.15-compat.md)
- [用户手册 English](docs/user-manual/en/README.md)
- [用户手册 中文](docs/user-manual/zh/README.md)
- [ユーザーマニュアル 日本語](docs/user-manual/ja/README.md)

## 从源码构建

### 环境要求

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### 常用命令

```bash
pnpm install
pnpm dev
pnpm typecheck
pnpm test:unit
pnpm build
```

### Rust 后端

```bash
cd src-tauri
cargo fmt
cargo clippy
cargo test
```

## macOS 10.15 相关说明

这个分支已经内置了几项关键的 Catalina 兼容配置：

- `.cargo/config.toml` 中设置了 `MACOSX_DEPLOYMENT_TARGET=10.15`
- `src-tauri/tauri.conf.json` 中将 `minimumSystemVersion` 设置为 `10.15`
- `vite.config.ts` 里下调了 Safari 相关构建目标
- `package.json` 中将 `esbuild` 固定到 Catalina 可用的版本

如果你遇到 Catalina 专属问题，建议先看 [docs/macos-10.15-compat.md](docs/macos-10.15-compat.md)。

## 项目结构

```text
src/              前端（React + TypeScript）
src-tauri/        后端（Tauri + Rust）
assets/           截图与静态资源
docs/             兼容性说明、用户手册、发布说明
tests/            前端测试
```

## 致谢

- 原始项目：[farion1231/cc-switch](https://github.com/farion1231/cc-switch)
- 本仓库是兼容性分支，不是上游官方发布渠道

## 许可证

本项目继续使用 [MIT License](LICENSE)。
