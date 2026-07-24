# WebDAV 分模块同步隔离编译与测试 README

这份文档专门说明这次 `WebDAV 分模块同步` 改动如何在**不污染你本地现有 CC Switch 配置**的前提下完成：

- 编译
- 单元测试
- 手工功能测试
- 真实坚果云 WebDAV smoke test

本文默认你在本仓库根目录执行命令。

---

## 1. 原则

这次测试必须遵守下面两条：

1. **不要复用你现在的本地 CC Switch 配置目录**
   - 不要直接读写你当前的 `~/.cc-switch`
   - 不要直接复用你当前的 `cc-switch-sync/default`

2. **开发版必须在隔离环境里运行**
   - 本地配置隔离：通过 `CC_SWITCH_TEST_HOME` 和 `HOME`
   - 云端目录隔离：通过新的 `remoteRoot` 和/或新的 `profile`

---

## 2. 一次性准备

### 2.1 安装依赖

```bash
pnpm install
```

如果你的 `pnpm` 是 11.x，且提示 `Ignored build scripts`，执行：

```bash
pnpm approve-builds --all
pnpm install
```

当前分支里也已经在 [pnpm-workspace.yaml](/home/fan/.config/superpowers/worktrees/cc-switch/feat-webdav-module-sync/pnpm-workspace.yaml) 增加了：

```yaml
allowBuilds:
  esbuild: true
  msw: true
```

这是为了让 `typecheck` / `vitest` 可以在当前环境下正常执行。它是**前端工具链验证配置**，不是 WebDAV 业务逻辑的一部分。

### 2.2 Rust 依赖

Rust 依赖会在第一次执行 `cargo test` / `cargo clippy` 时自动拉取。

---

## 3. 创建隔离运行环境

下面这组命令是最重要的。它会把开发版的 HOME、CC Switch 配置目录和 XDG 目录都隔离出去。

```bash
export DEV_ROOT="$(mktemp -d /tmp/cc-switch-webdav-dev.XXXXXX)"
mkdir -p "$DEV_ROOT"/home

export CC_SWITCH_TEST_HOME="$DEV_ROOT/home"

# 可选：把 Cargo 构建产物也隔离
export CARGO_TARGET_DIR="$PWD/.isolated-target"

echo "DEV_ROOT=$DEV_ROOT"
echo "CC_SWITCH_TEST_HOME=$CC_SWITCH_TEST_HOME"
```

### 3.1 为什么要设置这个变量

- `CC_SWITCH_TEST_HOME`
  - CC Switch 后端优先使用它来决定“用户主目录”。
  - 这能把 `~/.cc-switch`、skills 等全部导向这个临时目录，实现完全的数据隔离。
  - 我们**不再**重写原生的 `HOME` 或 `XDG_*`，这样就能保证你的 `cargo` 缓存、`mise` 配置、系统工具正常工作，做到秒级增量编译，而不是每次都在新 `HOME` 里重新下载几百个包并编译！

### 3.2 验证隔离是否生效

运行下面命令：

```bash
echo "$CC_SWITCH_TEST_HOME"
ls -la "$CC_SWITCH_TEST_HOME"
```

然后启动开发版后，新的 CC Switch 配置会写到：

```bash
$CC_SWITCH_TEST_HOME/.cc-switch
```

而不是你真实的：

```bash
~/.cc-switch
```

---

## 4. 如何编译和跑静态检查

### 4.1 前端检查

```bash
pnpm format:check
pnpm typecheck
pnpm test:unit
```

### 4.2 后端检查

```bash
cd src-tauri
cargo fmt --check
cargo clippy --no-deps
cargo test --no-fail-fast
cd ..
```

### 4.3 一次跑完整验证矩阵

```bash
pnpm format:check
pnpm typecheck
pnpm test:unit

cd src-tauri
cargo fmt --check
cargo clippy --no-deps
cargo test --no-fail-fast
cd ..
```

---

## 5. 如何编译开发版 / 构建包

### 5.1 启动开发版

```bash
pnpm tauri dev
```

或：

```bash
pnpm dev
```

推荐使用：

```bash
pnpm tauri dev
```

因为它会同时启动前端和 Tauri 后端。

### 5.2 构建发行包

```bash
pnpm build
```

构建产物通常在：

```bash
src-tauri/target/release/bundle/
```

如果你设置了：

```bash
export CARGO_TARGET_DIR="$PWD/.isolated-target"
```

那么 Rust 构建产物会走：

```bash
.isolated-target/
```

---

## 6. 手工功能测试：推荐用两个隔离环境

为了验证“上传只改选中模块、下载只覆盖选中模块”，最稳妥的方法是开两个完全隔离的开发环境：

- 环境 A：模拟“上传端”
- 环境 B：模拟“下载端”

### 6.1 环境 A

终端 A：

```bash
export DEV_ROOT_A="$(mktemp -d /tmp/cc-switch-webdav-A.XXXXXX)"
mkdir -p "$DEV_ROOT_A"/home
export CC_SWITCH_TEST_HOME="$DEV_ROOT_A/home"

pnpm tauri dev
```

### 6.2 环境 B

终端 B：

```bash
export DEV_ROOT_B="$(mktemp -d /tmp/cc-switch-webdav-B.XXXXXX)"
mkdir -p "$DEV_ROOT_B"/home
export CC_SWITCH_TEST_HOME="$DEV_ROOT_B/home"
export CC_SWITCH_DISABLE_SINGLE_INSTANCE=1

pnpm tauri dev
```

> **注意：** 环境变量 `CC_SWITCH_DISABLE_SINGLE_INSTANCE=1` 非常重要，它能绕过 Tauri 的单实例检测，允许双开。同时，得益于 Vite 的 `strictPort: false`，终端 B 的前端会自动占用 3001 等可用端口，后端也会极速复用编译缓存（0秒启动）。

---

## 7. WebDAV 测试配置建议

### 7.1 坚果云配置

在开发版设置页填：

- `WebDAV Server URL`
  - `https://dav.jianguoyun.com/dav/`
- `Username`
  - 你的坚果云邮箱账号
- `Password`
  - 坚果云第三方应用密码

### 7.2 remoteRoot 一定要用短名字

坚果云对根目录新建目录名有长度限制。

不要用太长的目录名，比如：

```text
cc-switch-sync-live-1779087934214
```

它会返回：

```text
sandbox name is too long
```

推荐使用短名字，例如：

```bash
csl-$(date +%s)
```

例如：

```text
csl-1779088000
```

### 7.3 profile 也建议用独立值

例如：

```text
module-sync
```

或：

```text
manual-a
```

### 7.4 不要使用你的生产路径

不要使用：

- `remoteRoot = cc-switch-sync`
- `profile = default`

用于这次开发测试。

---

## 8. 手工功能测试步骤

下面给出一套最完整、最稳妥的验证流程。

---

### 8.1 测试 1：全模块首次上传

在环境 A 中：

1. 打开设置页，配置 WebDAV
2. `remoteRoot` 填一个新的短目录，例如：
   - `csl-1779088000`
3. `profile` 填：
   - `module-sync`
4. 上传默认模块保持全选：
   - API
   - MCP
   - Prompts
   - Skills
5. 下载默认模块也保持全选
6. 点击“保存配置”
7. 点击“测试连接”
8. 在开发版里创建一些隔离测试数据：
   - API provider：`provider-a`
   - MCP：`mcp-a`
   - Prompt：`prompt-a`
   - Skill：`skill-a`
9. 点击“Upload to Cloud”
10. 确认上传

预期：

- 上传成功
- 远端创建新的 `v3/db-v6/<profile>` 结构
- 不影响你真实 `~/.cc-switch`
- 不影响你现在坚果云里的生产目录

---

### 8.2 测试 2：仅上传 MCP，确认远端其他模块保留

继续在环境 A 中：

1. 只保留“上传默认模块”里的 `MCP`
2. API / Prompts / Skills 全部取消
3. 保存配置
4. 修改 MCP 数据：
   - 删除 `mcp-a`
   - 新建 `mcp-b`
5. 不改 API / Prompt / Skill
6. 再次点击“Upload to Cloud”

预期：

- 上传成功
- 远端 MCP 变成新内容
- 远端 API / Prompts / Skills 仍保留第一次上传的内容

---

### 8.3 测试 3：仅下载 Prompts，确认本地未选模块保持不变

切换到环境 B：

1. 配置同一个坚果云地址
2. `remoteRoot` 填和环境 A 相同的值
3. `profile` 填和环境 A 相同的值
4. 在环境 B 里先创建本地专属数据：
   - API provider：`provider-local`
   - MCP：`mcp-local`
   - Skill：`skill-local`
   - Prompt：`prompt-local`
5. 将“下载默认模块”改成只选：
   - Prompts
6. 保存配置
7. 点击“Download from Cloud”
8. 确认下载

预期：

- `prompt-a` 会从远端同步到环境 B
- `provider-local` 保持不变
- `mcp-local` 保持不变
- `skill-local` 保持不变
- 未勾选模块不会被清空

---

### 8.4 测试 4：自动同步只跟随上传模块

继续在环境 B：

1. 将“上传默认模块”改成只选：
   - Skills
2. 开启 `Auto Sync`
3. 保存配置
4. 修改一个 API provider
5. 观察：
   - 不应该触发 WebDAV 上传
6. 修改一个 skill
7. 再观察：
   - 应该触发 WebDAV 上传

建议观察方式：

- 看应用里的 `Last sync`
- 看运行 `pnpm tauri dev` 的终端日志
- 搜索类似：

```text
[WebDAV][AutoSync]
```

预期：

- provider 修改不触发 auto sync
- skill 修改才触发 auto sync

---

## 9. 自动化真实 WebDAV smoke test

这次改动里已经加了一个 `ignored` 的 live test：

- 位置：
  - [src-tauri/src/services/webdav_sync.rs](/home/fan/.config/superpowers/worktrees/cc-switch/feat-webdav-module-sync/src-tauri/src/services/webdav_sync.rs)
- 测试名：
  - `live_webdav_module_sync_roundtrip_preserves_unselected_modules`

### 9.1 运行方式

```bash
cd src-tauri

CC_SWITCH_LIVE_WEBDAV_URL='https://dav.jianguoyun.com/dav/' \
CC_SWITCH_LIVE_WEBDAV_USERNAME='你的坚果云账号' \
CC_SWITCH_LIVE_WEBDAV_PASSWORD='你的坚果云第三方应用密码' \
cargo test live_webdav_module_sync_roundtrip_preserves_unselected_modules -- --ignored --nocapture
```

### 9.2 它会做什么

这个测试会：

1. 自动创建临时 `CC_SWITCH_TEST_HOME`
2. 自动生成新的短 `remoteRoot`
3. 用全模块上传一轮
4. 再做一轮“只上传 MCP”
5. 再在另一套隔离本地状态上做“只下载 Prompts”
6. 断言未勾选模块在本地保持不变

### 9.3 它不会做什么

它不会：

- 读写你的真实 `~/.cc-switch`
- 使用你当前生产的 `cc-switch-sync/default`

### 9.4 注意

这个 live test 会在坚果云里留下一个新的测试目录。

测试完成后，如果你想清理，可以到坚果云网页端或 WebDAV 目录里手动删除。

---

## 10. 快速命令清单

### 10.1 隔离启动开发版

```bash
export DEV_ROOT="$(mktemp -d /tmp/cc-switch-webdav-dev.XXXXXX)"
mkdir -p "$DEV_ROOT"/home
export CC_SWITCH_TEST_HOME="$DEV_ROOT/home"
pnpm tauri dev
```

### 10.2 跑前端验证

```bash
pnpm format:check
pnpm typecheck
pnpm test:unit
```

### 10.3 跑后端验证

```bash
cd src-tauri
cargo fmt --check
cargo clippy --no-deps
cargo test --no-fail-fast
cd ..
```

### 10.4 跑真实 WebDAV smoke test

```bash
cd src-tauri

CC_SWITCH_LIVE_WEBDAV_URL='https://dav.jianguoyun.com/dav/' \
CC_SWITCH_LIVE_WEBDAV_USERNAME='你的坚果云账号' \
CC_SWITCH_LIVE_WEBDAV_PASSWORD='你的坚果云第三方应用密码' \
cargo test live_webdav_module_sync_roundtrip_preserves_unselected_modules -- --ignored --nocapture
```

---

## 11. 测试完成后如何清理

### 11.1 删除本地隔离目录

如果你用了 `mktemp -d`，记住输出的 `DEV_ROOT`，测试完直接删：

```bash
rm -rf "$DEV_ROOT"
```

如果你开了 A/B 两个环境：

```bash
rm -rf "$DEV_ROOT_A" "$DEV_ROOT_B"
```

### 11.2 删除隔离构建目录（如果设置过）

```bash
rm -rf .isolated-target
```

### 11.3 删除坚果云测试目录

删除你这次测试使用的：

- `remoteRoot`
- 对应 `profile`

例如：

```text
csl-1779088000
```

---

## 12. 结论

如果你只是想“安全地编译和跑测试”，最短路径就是：

1. 先导出隔离环境变量
2. 跑：

```bash
pnpm typecheck
pnpm test:unit
cd src-tauri && cargo test --no-fail-fast && cargo clippy --no-deps && cd ..
```

如果你想“真的测这个功能”，推荐再做两件事：

1. 用 `pnpm tauri dev` 起两个隔离开发版，做 A/B 手工验证
2. 跑一次 `live_webdav_module_sync_roundtrip_preserves_unselected_modules`

这样你既能验证代码层，又能验证真实坚果云链路，而且不会污染你当前本地 CC Switch 配置。
