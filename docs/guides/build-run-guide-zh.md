# CC Switch 编译与运行指南（macOS）

本文档记录如何在本地从源码编译、运行和打包 `CC Switch`。内容以当前仓库实际验证过的流程为准，适合作为后续重复构建时的操作手册。

## 1. 适用范围

- 适用系统：macOS
- 前端：`Node.js + pnpm + Vite`
- 桌面端：`Tauri 2`
- 后端：`Rust`

当前仓库里声明的关键版本：

- Node.js：`22.12.0`
- Rust toolchain：`1.95`

对应文件：

- [`.node-version`](/Users/zhb/Desktop/cc-switch/.node-version)
- [`rust-toolchain.toml`](/Users/zhb/Desktop/cc-switch/rust-toolchain.toml)
- [`package.json`](/Users/zhb/Desktop/cc-switch/package.json)

## 2. 环境准备

### 2.1 安装 Xcode Command Line Tools

```bash
xcode-select --install
```

安装完成后确认：

```bash
xcode-select -p
```

正常会输出类似：

```bash
/Library/Developer/CommandLineTools
```

### 2.2 安装 Node.js 和 pnpm

建议使用 Node `22.12.0`。

确认版本：

```bash
node --version
pnpm --version
```

如果没有 `pnpm`，可以先启用：

```bash
corepack enable
corepack prepare pnpm@latest --activate
```

### 2.3 安装 Rust 和 Cargo

建议用 `rustup` 安装，并确认默认工具链是 `1.95` 或兼容版本。

确认版本：

```bash
rustup show
cargo --version
rustc --version
```

如果你在中国大陆网络环境，推荐先设置镜像：

```bash
export RUSTUP_DIST_SERVER=https://rsproxy.cn
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
```

如果你同时有本地 HTTP 代理，也可以在当前终端临时加上：

```bash
export HTTP_PROXY=http://127.0.0.1:7897
export HTTPS_PROXY=http://127.0.0.1:7897
export ALL_PROXY=http://127.0.0.1:7897
```

## 3. 获取源码

如果本地还没有仓库：

```bash
git clone https://github.com/farion1231/cc-switch.git
cd cc-switch
```

如果你已经有仓库，只需要进入目录：

```bash
cd /你的路径/cc-switch
```

## 4. 安装前端依赖

在项目根目录执行：

```bash
pnpm install --frozen-lockfile
```

第一次安装会比较久，属于正常现象。

## 5. 编译前检查

### 5.1 前端类型检查

```bash
pnpm exec tsc --noEmit
```

### 5.2 Rust 后端检查

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

如果这是第一次运行，Rust 会下载 toolchain 组件和 crates 依赖，时间会明显更长。

## 6. 开发模式运行

在项目根目录执行：

```bash
pnpm tauri dev
```

这个命令会自动做两件事：

1. 启动前端开发服务器
2. 编译并启动 Tauri 桌面应用

### 6.1 正常启动时你会看到

前端服务类似：

```bash
VITE v7.x ready
Local: http://localhost:3000/
```

后端会继续编译，第一次通常需要 1 到数分钟。完成后会看到类似：

```bash
Finished `dev` profile [unoptimized + debuginfo] target(s) in ...
Running `target/debug/cc-switch`
```

应用窗口弹出后，就说明开发运行成功。

### 6.2 如何停止

在当前终端按：

```bash
Ctrl + C
```

## 7. 正式构建

如果你想生成可分发的应用包，在项目根目录执行：

```bash
pnpm build
```

这个命令会先构建前端，再执行 Tauri 正式打包。

产物通常位于：

```bash
src-tauri/target/release/bundle/
```

在 macOS 下，常见会生成：

- `.app`
- `.dmg`

具体以实际输出为准。

## 8. 常用命令速查

### 安装依赖

```bash
pnpm install --frozen-lockfile
```

### 前端类型检查

```bash
pnpm exec tsc --noEmit
```

### Rust 后端检查

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

### 启动开发模式

```bash
pnpm tauri dev
```

### 正式打包

```bash
pnpm build
```

## 9. 中国大陆网络建议

如果你在国内，建议把下面这些环境变量放到当前终端中再执行安装或编译：

```bash
export HTTP_PROXY=http://127.0.0.1:7897
export HTTPS_PROXY=http://127.0.0.1:7897
export ALL_PROXY=http://127.0.0.1:7897

export RUSTUP_DIST_SERVER=https://rsproxy.cn
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
```

只想临时对单条命令生效，也可以这样写：

```bash
HTTP_PROXY=http://127.0.0.1:7897 \
HTTPS_PROXY=http://127.0.0.1:7897 \
ALL_PROXY=http://127.0.0.1:7897 \
pnpm tauri dev
```

或者：

```bash
HTTP_PROXY=http://127.0.0.1:7897 \
HTTPS_PROXY=http://127.0.0.1:7897 \
ALL_PROXY=http://127.0.0.1:7897 \
cargo check --manifest-path src-tauri/Cargo.toml
```

## 10. 常见问题

### 10.1 `cargo` 或 `rustc` 不可用

先确认：

```bash
which cargo
which rustc
rustup show
```

如果 `rustup` 已安装但工具链异常，可以尝试：

```bash
rustup self update
rustup set profile default
rustup toolchain install 1.95
rustup default 1.95
```

### 10.2 看到 `cargo binary ... is not applicable to the toolchain`

这通常是当前 Rust toolchain 状态不完整。可尝试：

```bash
rustup toolchain uninstall stable-aarch64-apple-darwin
rustup toolchain install stable-aarch64-apple-darwin
rustup default stable-aarch64-apple-darwin
```

如果在国内，建议同时设置 `rsproxy.cn` 镜像。

### 10.3 `pnpm tauri dev` 卡很久

第一次运行慢通常是正常的，因为它会：

- 安装前端依赖
- 下载 Rust crates
- 编译 Tauri 依赖
- 编译项目本身

建议先单独执行：

```bash
pnpm exec tsc --noEmit
cargo check --manifest-path src-tauri/Cargo.toml
```

确认两边都通过，再跑：

```bash
pnpm tauri dev
```

### 10.4 `localhost:3000` 被占用

开发模式默认使用 `http://localhost:3000/`。如果端口冲突，可以先找占用进程：

```bash
lsof -nP -iTCP:3000 -sTCP:LISTEN
```

结束占用进程后再重试。

### 10.5 `Browserslist` 或前端依赖数据过旧

这类一般只是提示，不影响运行。需要时可以自行更新：

```bash
pnpm up
```

是否升级依赖，建议结合项目实际情况判断，不要在不需要时顺手升级整套依赖。

## 11. 推荐的最短流程

后续你自己编译运行时，最短可以按这个顺序执行：

```bash
cd /你的路径/cc-switch
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit
cargo check --manifest-path src-tauri/Cargo.toml
pnpm tauri dev
```

如果是打正式包：

```bash
cd /你的路径/cc-switch
pnpm install --frozen-lockfile
pnpm build
```

## 12. 本次已验证通过的事实

这次在当前仓库里，我已经实际验证过：

- `pnpm exec tsc --noEmit` 可通过
- `cargo check --manifest-path src-tauri/Cargo.toml` 可通过
- `pnpm tauri dev` 可启动

所以这份文档不是泛泛而谈，而是基于当前项目实际跑通过的链路整理出来的。
