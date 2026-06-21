# CC Switch 构建打包指南

## 概述

CC Switch 是一个基于 [Tauri v2](https://v2.tauri.app/) 的跨平台桌面应用，使用 React 前端 + Rust 后端架构。本文档说明如何将源代码打包为可执行安装包。

## 环境要求

| 工具 | 版本要求 | 检查命令 | 安装方式 |
|------|---------|---------|---------|
| **Node.js** | >= 18 | `node --version` | [nodejs.org](https://nodejs.org/) |
| **pnpm** | >= 9 | `pnpm --version` | `npm install -g pnpm` |
| **Rust** | >= 1.85.0 | `rustc --version` | [rustup.rs](https://rustup.rs/) |
| **Tauri CLI** | ^2.8.0 | 由 pnpm 管理 | 含在 devDependencies 中 |

### Windows 额外要求

- **WebView2**: Windows 10 (1803+) 已内置；旧系统需手动安装 [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)
- **Visual Studio Build Tools**: 需要安装 "Desktop development with C++" 工作负载
  - 推荐安装 [Visual Studio 2022 Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022)

## 一键打包

在项目根目录执行：

```powershell
.\scripts\build.ps1
```

脚本将自动完成：
1. 检查环境依赖（Node.js、pnpm、Rust、Tauri CLI）
2. 安装 `pnpm install`
3. 执行 `pnpm tauri build`（release 模式）
4. 输出构建产物路径

### 脚本参数

| 参数 | 说明 | 示例 |
|------|------|------|
| `-SkipInstall` | 跳过依赖安装（已安装时用） | `.\scripts\build.ps1 -SkipInstall` |
| `-Debug` | 构建 debug 版本（默认 release） | `.\scripts\build.ps1 -Debug` |
| `-Target <triple>` | 交叉编译目标平台 | `.\scripts\build.ps1 -Target x86_64-pc-windows-msvc` |

## 构建产物

构建成功后，产物位于：

| 平台 | 输出路径 | 格式 |
|------|---------|------|
| **Windows** | `src-tauri/target/release/bundle/msi/` | `.msi` 安装包 |
| **Windows** | `src-tauri/target/release/bundle/wix/` | 若启用 WiX 则会生成 |
| **Windows** | `src-tauri/target/release/cc-switch.exe` | 便携 exe（需配合资源文件） |
| **macOS** | `src-tauri/target/release/bundle/dmg/` | `.dmg` 镜像 |
| **Linux** | `src-tauri/target/release/bundle/appimage/` | `.AppImage` |
| **Linux** | `src-tauri/target/release/bundle/deb/` | `.deb` 包 |

> 默认 `bundle.targets` 为 `"all"`，会生成当前平台支持的所有格式。如需限制，可修改 `tauri.conf.json`。

## 手动构建流程

如果希望逐步执行：

```powershell
# 1. 安装依赖
pnpm install

# 2. 构建前端（会被 tauri build 自动触发，也可单独执行）
pnpm run build:renderer

# 3. 执行 Tauri 构建
pnpm tauri build

# 4. （可选）仅构建 debug 版本
pnpm tauri build --profile debug
```

## 常见问题

### Q: 构建失败提示 "Could not find any targets"

确保安装 Rust 时勾选了目标工具链。Windows 平台：
```powershell
rustup target add x86_64-pc-windows-msvc
```

### Q: 构建时提示 "linker `link.exe` not found"

需要 Visual Studio Build Tools 或 Visual Studio 的 "Desktop development with C++" 工作负载。

### Q: 如何修改版本号？

版本号在 `tauri.conf.json` 的 `version` 字段中定义，与 `package.json` 保持一致即可。

### Q: 构建产物体积偏大

`Cargo.toml` 中已有优化配置（`lto = "thin"`, `opt-level = "s"`, `strip = "symbols"`），能有效减小体积。

### Q: 签名错误: "A public key has been found, but no private key"

此提示表示 `TAURI_SIGNING_PRIVATE_KEY` 环境变量未设置。如需生成正式签名更新包：

1. 按照 [Tauri 更新文档](https://v2.tauri.app/plugin/updater/) 生成签名密钥对
2. 将私钥设置为环境变量 `TAURI_SIGNING_PRIVATE_KEY`
3. 将公钥更新到 `tauri.conf.json` 中 `plugins.updater.pubkey` 字段

如果仅做本地测试构建、不需要更新签名，可将 `TAURI_SIGNING_PRIVATE_KEY` 设为任意占位值，或临时将 `tauri.conf.json` 中 `plugins.updater.createUpdaterArtifacts` 设为 `false`。

### Q: 如何排查构建失败？

查看 `src-tauri/target/release/` 下的 Cargo 构建日志，或添加 `RUST_LOG=cc_switch=debug` 环境变量重试。
