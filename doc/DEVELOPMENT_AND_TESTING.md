# CC Switch 开发测试指南

本文档旨在指导开发者如何在不影响本地已安装的 CC Switch 正式版的情况下，进行开发、编译、安装和隔离测试。

## 1. 开发环境配置

### 前提条件
在开始之前，请确保您的系统已安装以下工具：
- **Node.js**: v18+ 
- **pnpm**: v8+ (项目使用 pnpm 管理依赖)
- **Rust**: v1.85+ (及 Cargo)
- **Tauri 依赖**: 请参考 [Tauri 2.0 官方指南](https://v2.tauri.app/start/prerequisites/) 安装各平台（Windows/macOS/Linux）所需的系统库。

### 初始化项目
```bash
# 克隆仓库（如果你还没克隆）
# git clone <repository-url>
# cd cc-switch

# 安装前端依赖
pnpm install
```

---

## 2. 编译与运行

### 开发模式 (热重载)
这是最常用的开发方式，前端支持热重载，后端修改后会自动重新编译运行。
```bash
pnpm dev
```
*注意：在热重载模式下，程序会自动进入 Debug 模式。*

### 生产版本构建
如果您想测试完整的打包流程，可以运行：
```bash
pnpm build
```
构建产物将位于 `src-tauri/target/release/bundle/` 下。

---

## 3. 隔离测试（不影响本地正式版）

CC Switch 默认将配置文件和数据库存储在 `~/.cc-switch` (Linux/macOS) 或 `%USERPROFILE%\.cc-switch` (Windows) 目录下。为了避免开发版本覆盖您正式版的数据，建议使用以下方法进行隔离：

### 方法 A：更改标识符（推荐用于长期开发）
在 `src-tauri/tauri.conf.json` 中，临时修改 `identifier`：
```json
{
  "identifier": "com.ccswitch.dev",  // 原始为 com.ccswitch.desktop
  "productName": "CC Switch Dev"
}
```
这样 Tauri 插件（如 `window-state`）将使用不同的存储空间。

### 方法 B：环境变量隔离（适用于 Linux/macOS）
在启动命令前指定不同的 `HOME` 目录，强制程序在新的位置创建配置：
```bash
# 创建一个专门的测试目录
mkdir -p ./test-env

# 以隔离的路径运行
HOME=$PWD/test-env pnpm dev
```
这样，所有的配置文件和 `.cc-switch` 数据库都会生成在 `./test-env/.cc-switch` 中，完全不会触碰您原本的 `~/.cc-switch`。

---

## 4. 安装测试

### 运行未封包的二进制文件
您可以直接运行编译器生成的 release 二进制文件：
- **Linux**: `./src-tauri/target/release/cc-switch`
- **Windows**: `.\src-tauri\target\release\cc_switch.exe`

### 安装包测试
在 `pnpm build` 完成后：
- **Windows**: 运行 `src-tauri/target/release/bundle/msi/*.msi` 或 `exe`。
- **macOS**: 打开 `src-tauri/target/release/bundle/dmg/*.dmg`。
- **Linux**: 运行 `src-tauri/target/release/bundle/deb/*.deb` 或 `AppImage`。

**提示**：在安装测试版本前，建议先卸载正式版，或者使用上述的“更改标识符”方法使两者共存。

---

## 5. 自动化测试

项目包含了完整的测试套件，建议在提交代码前运行：

### 前端测试 (Vitest)
```bash
pnpm test:unit
```

### 后端测试 (Cargo)
```bash
cd src-tauri
cargo test
```

### 静态检查
```bash
pnpm typecheck  # TypeScript 类型检查
cargo clippy    # Rust 代码检查
```

---

## 6. 常见问题 (FAQ)

- **Q: 为什么我修改了 Rust 代码但没有生效？**
  - A: `pnpm dev` 会监控 Rust 变更，但有时由于文件锁定可能失败。尝试关闭程序，运行 `cargo clean` 后重新 `pnpm dev`。
- **Q: 如何查看后端日志？**
  - A: 在 `pnpm dev` 运行的终端中，你会看到后端的 `println!` 或 `log::info!` 输出。
- **Q: 数据库在哪里？**
  - A: 默认在 `~/.cc-switch/cc-switch.db`，是一个 SQLite 数据库。
