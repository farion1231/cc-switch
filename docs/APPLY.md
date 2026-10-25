# 华为 CodeAgent 补丁包 - 完整使用指南

> 当前仓库的 Windows 构建请使用 [可重复的 Tauri 构建说明](guides/windows-build.md) 和 scripts/build-windows-tauri.cmd。本文是旧补丁包说明，其中的 cargo build 命令不适用于当前 Tauri 应用。

## 📦 包内容

```
cc-switch-huawei-patch/
├── src/                              # 前端代码
│   ├── App.tsx                       # 只加了一行: import "@/huawei"
│   ├── huawei/                       # 华为模块（全部新建，不影响 main 分支）
│   │   ├── constants.ts              # 华为 providerType 常量 + 运行时注入
│   │   ├── huaweiDesktopPresets.ts   # Desktop 预设定义
│   │   ├── index.ts                  # 模块入口：运行时注册所有华为扩展
│   │   ├── huaweiCodeAgent2Api.ts
│   │   ├── huaweiCodeAgent2Presets.ts
│   │   ├── huaweiCodeAgent3Presets.ts
│   │   ├── huaweiCodeAgent3Presets.test.ts
│   │   ├── HuaweiCodeAgentOAuthSection.tsx
│   │   ├── authCenterSection.tsx
│   │   ├── types.ts
│   │   ├── hooks/
│   │   │   └── useHuaweiCodeAgentOauth.ts
│   │   └── locales/
│   │       ├── index.ts
│   │       ├── en.json
│   │       ├── zh.json
│   │       ├── ja.json
│   │       └── zh-TW.json
│   ├── config/
│   │   ├── constants.ts              # 已删除华为常量（改为运行时注入）
│   │   └── claudeDesktopProviderPresets.ts  # 已删除华为预设（改为运行时注入）
│   ├── components/providers/forms/
│   │   ├── ProviderForm.tsx          # 导入路径改为 @/huawei/constants
│   │   ├── CodeAgentDesktopProviderForm.tsx  # 同上
│   │   └── CodeAgentFormFields.tsx   # 华为专用表单字段
│   └── icons/extracted/
│       └── huawei.svg
│
├── src-tauri/                        # Rust 后端
│   ├── build.bat                    # 一键编译脚本
│   ├── src/huawei/                   # 华为后端模块
│   │   ├── auth/
│   │   │   ├── commands.rs
│   │   │   ├── manager.rs
│   │   │   └── mod.rs
│   │   ├── claude_config.rs
│   │   ├── connectivity_proxy.rs
│   │   ├── fingerprint.rs
│   │   ├── mod.rs
│   │   ├── proxy_hooks.rs
│   │   ├── proxy_processor.rs
│   │   └── tls_cert.rs
│   └── tests/
│       ├── huawei_codeagent.rs
│       └── huawei_idea_auth.rs
│
└── APPLY.md                          # 本文件
```

---

## 📋 环境要求

### 1. Rust 工具链
- **版本**: Rust 1.95.0
- **安装**: https://rustup.rs
- **验证**: `rustc --version` 应显示 1.95.0

### 2. Visual Studio Build Tools 2022
- 下载: https://visualstudio.microsoft.com/visual-cpp-build-tools/
- 安装时勾选: **"Desktop development with C++"**
- 必须包含: MSVC 编译器 + Windows 10/11 SDK
- **⚠️ 注意**: 推荐 MSVC 14.32-14.43，避免 14.44+（有 C1056 时间戳 bug）

### 3. Node.js
- **版本**: 18+
- **包管理器**: pnpm (`npm install -g pnpm`)

### 4. 操作系统
- Windows 10/11 x64
- 至少 5GB 可用磁盘空间

---

## 🚀 完整操作流程

### 第 1 步：克隆项目

```bash
git clone https://github.com/farion1231/cc-switch.git
cd cc-switch
git checkout main
git pull origin main
```

### 第 2 步：覆盖华为补丁文件

```bash
# 将补丁包解压到项目根目录（覆盖同名文件）
# 假设补丁包在 D:\cc-switch-huawei-patch\

# Windows cmd:
xcopy /E /Y "D:\cc-switch-huawei-patch\src" "src\"
xcopy /E /Y "D:\cc-switch-huawei-patch\src-tauri" "src-tauri\"

# 或 Git Bash:
cp -r /d/cc-switch-huawei-patch/src/* src/
cp -r /d/cc-switch-huawei-patch/src-tauri/* src-tauri/
```

### 第 3 步：配置 Cargo 镜像（中国大陆需要）

```bash
# 创建或编辑 ~/.cargo/config.toml
cat > ~/.cargo/config.toml << 'EOF'
[source.crates-io]
replace-with = "ustc"

[source.ustc]
registry = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"

[net]
git-fetch-with-cli = true
EOF
```

### 第 4 步：安装前端依赖

```bash
pnpm install --no-frozen-lockfile
```

### 第 5 步：验证前端编译

```bash
# TypeScript 类型检查
pnpm typecheck

# 前端构建
pnpm build:renderer
```

**预期输出**: 无错误，`dist/` 目录生成

### 第 6 步：编译 Rust 后端

**方式 A：使用一键脚本（推荐）**

```cmd
cd src-tauri
build.bat
```

**方式 B：手动编译**

```cmd
# 1. 初始化 MSVC 环境（必须在 cmd 中执行，不是 PowerShell）
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x64

# 2. 验证环境
where cl.exe

# 3. 设置镜像源（如果网络不通）
set NO_PROXY=mirrors.ustc.edu.cn,*.huawei.com
set no_proxy=mirrors.ustc.edu.cn,*.huawei.com

# 4. 编译
cargo build --release
```

### 第 7 步：验证编译结果

```bash
# 检查二进制是否生成
ls -lh src-tauri/target/release/cc-switch.exe

# 预期输出: 约 30-50MB 的 exe 文件
```

### 第 8 步：运行验证

```bash
# 启动应用
./src-tauri/target/release/cc-switch.exe
```

**在应用中验证华为供应商支持：**
1. 点击"添加供应商"
2. 在列表中找到 **Huawei CodeAgent 2.0** 和 **Huawei CodeAgent 3.0**
3. 选择其中一个，点击授权
4. 完成 OAuth 授权流程
5. 确认供应商出现在列表中

---

## 🔧 一键编译脚本说明

补丁包中包含 `src-tauri/build.bat`，可直接运行：

```cmd
cd src-tauri
build.bat
```

脚本会自动完成以下步骤：
1. 初始化 MSVC x64 编译环境（调用 vcvarsall.bat）
2. 验证 cl.exe 和 link.exe 可用
3. 设置 Cargo 镜像源环境变量
4. 检查 Rust 工具链版本
5. 执行 `cargo build --release`
6. 验证二进制文件生成

---

## ⚠️ 常见问题

### Q1: C1056 编译错误

**现象**: `fatal error C1056: 无法更新"...o"中的时间日期戳字段`

**原因**: 杀毒软件（如 Windows Defender、奇安信天擎）扫描生成的 `.o` 文件时锁定

**解决**:
1. 关闭 Windows Defender 实时保护
2. 退出奇安信天擎
3. 或将 `target/` 目录加入白名单
4. 或使用 MSVC 14.32-14.43（避免 14.44+ 的时间戳 bug）

### Q2: crates.io 网络超时

**现象**: `failed to download from index.crates.io`

**解决**: 使用中科大镜像源（见第 3 步配置）

### Q3: link.exe 找不到

**现象**: `linker link.exe not found`

**解决**: 必须先执行 `vcvarsall.bat x64` 初始化 MSVC 环境

### Q4: kernel32.lib 找不到

**现象**: `LNK1181: cannot open input file "kernel32.lib"`

**解决**: Visual Studio Build Tools 安装时必须勾选 Windows SDK

### Q5: Rust 版本不匹配

**现象**: `error: could not download nonexistent rust version 1.95`

**解决**:
```bash
# 方法1: 安装指定版本
rustup toolchain install 1.95.0-x86_64-pc-windows-msvc --profile minimal

# 方法2: 修改 rust-toolchain.toml 使用本地已安装版本
# 将 channel = "1.95.0" 改为本地版本如 channel = "1.92.0"
```

### Q6: pnpm-lock.yaml 冲突

**现象**: `ERR_PNPM_OUTDATED_LOCKFILE`

**解决**: 使用 `pnpm install --no-frozen-lockfile` 允许更新锁文件

---

## 🔑 重构说明

本次补丁已做**解耦重构**，华为代码不再内嵌在 main 分支的共享文件中：

| 改造前 | 改造后 | 效果 |
|--------|--------|------|
| `constants.ts` 内嵌华为常量 | 移到 `huawei/constants.ts`，运行时注入 | main 合并零冲突 |
| `claudeDesktopProviderPresets.ts` 内嵌华为预设 | 移到 `huawei/huaweiDesktopPresets.ts`，运行时 push | main 合并零冲突 |
| `ProviderForm.tsx` 引用 `@/config/constants` | 改为引用 `@/huawei/constants` | main 合并零冲突 |
| i18n 文件内嵌华为翻译 | 已在 `huawei/locales/` 独立注入 | main 合并零冲突 |

**以后合并 main 分支时**，这些共享文件不再有华为代码，理论上零冲突。

---

## 📐 预计工时

| 步骤 | 时间 |
|------|------|
| 环境准备 | 20-30 分钟 |
| 依赖安装 | 5-10 分钟 |
| 代码覆盖 | 2 分钟 |
| 前端编译 | 2-5 分钟 |
| Rust 编译 | 10-30 分钟 |
| 功能验证 | 10 分钟 |
| **总计** | **~1-1.5 小时** |
