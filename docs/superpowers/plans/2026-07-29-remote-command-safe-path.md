# Remote Command Safe PATH Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 SSH 非交互式 shell 在用户 PATH 缺失时仍能完成 Linux Agent 预检、完整性校验、启动和清理。

**Architecture:** 在 `remote/ephemeral_deploy.rs` 集中定义安全 PATH 前缀，并让预检、启动和清理三个命令生成器复用。`remote/ssh.rs` 继续只负责进程与错误边界，不改变 SSH 参数隔离、Agent 协议或错误码。

**Tech Stack:** Rust 2021、OpenSSH、Cargo integration tests、Tauri 2、PowerShell

---

## 文件结构

- Modify: `src-tauri/src/remote/ephemeral_deploy.rs` - 维护安全 PATH 前缀，生成预检、启动和清理命令。
- Modify: `src-tauri/src/remote/ssh.rs` - 预检调用受控命令生成器。
- Modify: `src-tauri/tests/remote_ephemeral_deploy.rs` - 覆盖 PATH 初始化、alias 绕过和既有命令约束。

### Task 1: 启动与清理命令建立安全 PATH

**Files:**
- Modify: `src-tauri/tests/remote_ephemeral_deploy.rs:34-76`
- Modify: `src-tauri/src/remote/ephemeral_deploy.rs:35-54`

- [ ] **Step 1: 写入会失败的启动与清理命令测试**

在测试文件顶部测试辅助区域加入：

```rust
const SAFE_REMOTE_COMMAND_PREFIX: &str =
    r#"PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}"; export PATH; "#;
```

在 `scp_and_remote_commands_keep_transport_arguments_isolated` 中加入启动命令断言，并用以下 `cleanup` 断言替换原有的 `assert_eq!`：

```rust
    // 非交互式 shell 可能不继承标准 PATH；启动与清理必须共享同一前缀，避免只修一条退出路径。
    assert!(launch.starts_with(SAFE_REMOTE_COMMAND_PREFIX));
    for executable in ["wc", "tr", "sha256sum", "awk", "chmod", "rm"] {
        assert!(
            launch.contains(&format!("command {executable}")),
            "启动命令必须绕过 {executable} 的 alias 或同名函数"
        );
    }

    let cleanup = build_cleanup_command(&spec.remote_path);
    assert!(cleanup.starts_with(SAFE_REMOTE_COMMAND_PREFIX));
    assert!(cleanup.contains(&format!(
        "command rm -f -- '{}'",
        spec.remote_path
    )));
```

- [ ] **Step 2: 运行测试并确认按预期失败**

先停止当前仓库 Tauri dev，避免 Windows 锁定 `target/debug/cc-switch.exe`。从 dev 根进程递归收集子进程并停止，不结束其他 Node/Codex 进程。

Run:

```powershell
$env:CC_SWITCH_AGENT_X86_64_PATH='D:\code_program\person\cc-switch\src-tauri\target\downloaded-agent-artifacts\linux-x86_64\cc-switch-agent'
$env:CC_SWITCH_AGENT_AARCH64_PATH='D:\code_program\person\cc-switch\src-tauri\target\downloaded-agent-artifacts\linux-aarch64\cc-switch-agent'
cargo test -j 1 --test remote_ephemeral_deploy scp_and_remote_commands_keep_transport_arguments_isolated -- --exact --nocapture
```

Expected: FAIL，首个失败断言指出启动命令缺少 `SAFE_REMOTE_COMMAND_PREFIX`。

- [ ] **Step 3: 实现最小安全 PATH 包装**

在 `ephemeral_deploy.rs` 的 import 后加入：

```rust
const REMOTE_PATH_SETUP: &str =
    r#"PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}"; export PATH"#;

/// 为桌面端生成的远端命令建立确定性工具搜索路径，同时保留管理员提供的附加目录。
/// 标准目录必须位于原 PATH 之前，避免用户环境中的同名程序接管完整性校验和临时文件清理。
fn with_remote_command_environment(command: String) -> String {
    format!("{REMOTE_PATH_SETUP}; {command}")
}
```

把 `build_launch_command` 改为先生成原命令体，再包装；外部工具都通过 `command` 调用：

```rust
pub fn build_launch_command(spec: &EphemeralAgentSpec) -> String {
    let command = format!(
        "path='{path}'; \
cleanup() {{ command rm -f -- \"$path\"; }}; \
trap cleanup EXIT HUP INT TERM; \
actual_size=$(command wc -c < \"$path\" | command tr -d '[:space:]'); \
if [ \"$actual_size\" != '{length}' ]; then echo 'AGENT_INTEGRITY_FAILED: size' >&2; exit 70; fi; \
actual_sha=$(command sha256sum -- \"$path\" | command awk '{{print $1}}'); \
if [ \"$actual_sha\" != '{sha256}' ]; then echo 'AGENT_INTEGRITY_FAILED: sha256' >&2; exit 71; fi; \
command chmod 700 -- \"$path\" || exit 72; \
\"$path\" --stdio",
        path = spec.remote_path,
        length = spec.length,
        sha256 = spec.sha256,
    );
    with_remote_command_environment(command)
}
```

把清理命令改为：

```rust
pub fn build_cleanup_command(remote_path: &str) -> String {
    with_remote_command_environment(format!("command rm -f -- '{remote_path}'"))
}
```

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
cargo test -j 1 --test remote_ephemeral_deploy scp_and_remote_commands_keep_transport_arguments_isolated -- --exact --nocapture
```

Expected: PASS，`1 passed; 0 failed`。

- [ ] **Step 5: 提交第一轮修复**

```powershell
git add src-tauri/src/remote/ephemeral_deploy.rs src-tauri/tests/remote_ephemeral_deploy.rs
git commit -m "fix(remote): bootstrap Agent command PATH"
```

### Task 2: 预检复用安全远端环境

**Files:**
- Modify: `src-tauri/tests/remote_ephemeral_deploy.rs:5-9`
- Modify: `src-tauri/tests/remote_ephemeral_deploy.rs:76`
- Modify: `src-tauri/src/remote/ephemeral_deploy.rs:35-45`
- Modify: `src-tauri/src/remote/ssh.rs:10-16,138-141`

- [ ] **Step 1: 写入会失败的预检命令测试**

把测试 import 改为：

```rust
use cc_switch_lib::remote::ephemeral_deploy::{
    build_cleanup_command, build_launch_command, build_preflight_command, build_scp_args,
    CleanupScheduler, EphemeralCleanupGuard,
};
```

新增测试：

```rust
#[test]
fn preflight_uses_the_same_safe_remote_environment() {
    let command = build_preflight_command();

    // 预检和 Agent 生命周期命令必须保持同一环境契约，否则 PATH 异常会在不同阶段产生不一致错误。
    assert!(command.starts_with(SAFE_REMOTE_COMMAND_PREFIX));
    assert!(command.contains("command uname -s; command uname -m"));
}
```

- [ ] **Step 2: 运行测试并确认按预期失败**

Run:

```powershell
cargo test -j 1 --test remote_ephemeral_deploy preflight_uses_the_same_safe_remote_environment -- --exact --nocapture
```

Expected: FAIL to compile with unresolved import `build_preflight_command`。

- [ ] **Step 3: 实现预检命令生成器并接入 SSH**

在 `ephemeral_deploy.rs` 中加入：

```rust
pub fn build_preflight_command() -> String {
    with_remote_command_environment("command uname -s; command uname -m".to_string())
}
```

在 `ssh.rs` 的 import 中加入 `build_preflight_command`，并把预检命令构造改为：

```rust
pub fn preflight(target: &RemoteTargetConfig) -> Result<RemotePlatform, RemoteSshError> {
    let args = build_ssh_args(target, &[build_preflight_command()])?;
```

- [ ] **Step 4: 运行预检测试和完整临时部署测试**

Run:

```powershell
cargo test -j 1 --test remote_ephemeral_deploy preflight_uses_the_same_safe_remote_environment -- --exact --nocapture
cargo test -j 1 --test remote_ephemeral_deploy -- --nocapture
```

Expected: 两条命令均 PASS；完整套件为 `7 passed; 0 failed`。

- [ ] **Step 5: 提交预检接入**

```powershell
git add src-tauri/src/remote/ephemeral_deploy.rs src-tauri/src/remote/ssh.rs src-tauri/tests/remote_ephemeral_deploy.rs
git commit -m "fix(remote): harden SSH preflight PATH"
```

### Task 3: 回归、重建与真实远端验证

**Files:**
- Verify: `src-tauri/src/remote/ephemeral_deploy.rs`
- Verify: `src-tauri/src/remote/ssh.rs`
- Verify: `src-tauri/tests/remote_ephemeral_deploy.rs`

- [ ] **Step 1: 格式化并运行静态检查**

Run:

```powershell
cargo fmt --all -- --check
cargo clippy -j 1 -p cc-switch --lib -- -D warnings
```

Expected: 两条命令 exit code 0，无格式或 Clippy 错误。

- [ ] **Step 2: 运行远端功能回归测试**

Run:

```powershell
$env:CC_SWITCH_AGENT_X86_64_PATH='D:\code_program\person\cc-switch\src-tauri\target\downloaded-agent-artifacts\linux-x86_64\cc-switch-agent'
$env:CC_SWITCH_AGENT_AARCH64_PATH='D:\code_program\person\cc-switch\src-tauri\target\downloaded-agent-artifacts\linux-aarch64\cc-switch-agent'
cargo test -j 1 --test remote_ephemeral_deploy -- --nocapture
cargo test -j 1 --test remote_agent_minimal -- --nocapture
cargo test -j 1 --test remote_ssh_config -- --nocapture
```

Expected: 全部 PASS，无 `memory allocation failed` 或文件锁错误。

- [ ] **Step 3: 检查提交范围和工作区**

Run:

```powershell
git diff --check
git status --short --branch
git log -3 --oneline --decorate
```

Expected: 无未提交代码改动；本地 `main` 仅领先远端本次设计和修复提交。

- [ ] **Step 4: 使用真实 Agent 产物恢复 Tauri dev**

Run:

```powershell
$env:CC_SWITCH_AGENT_X86_64_PATH='D:\code_program\person\cc-switch\src-tauri\target\downloaded-agent-artifacts\linux-x86_64\cc-switch-agent'
$env:CC_SWITCH_AGENT_AARCH64_PATH='D:\code_program\person\cc-switch\src-tauri\target\downloaded-agent-artifacts\linux-aarch64\cc-switch-agent'
$env:CARGO_BUILD_JOBS='1'
Start-Process -FilePath 'D:\app\node\pnpm.cmd' -ArgumentList 'run','dev' -WorkingDirectory 'D:\code_program\person\cc-switch' -WindowStyle Hidden -RedirectStandardOutput 'D:\code_program\person\cc-switch\src-tauri\target\tauri-dev.stdout.log' -RedirectStandardError 'D:\code_program\person\cc-switch\src-tauri\target\tauri-dev.stderr.log'
```

Expected: `src-tauri/target/debug/cc-switch.exe` 持续运行，`http://localhost:3000/` 返回 HTTP 200，stderr 不含构建失败。

- [ ] **Step 5: 在 `dubhe-dev-xh` 真实复验**

在开发版中连接既有远端目标。Expected:

- 不出现 `command not found: wc/tr/rm`。
- 不出现 `AGENT_INTEGRITY_FAILED: size`。
- Agent 握手成功，远端功能可调用。
- 断开后执行 `find /tmp -maxdepth 1 -name 'cc-switch-agent-*' -print` 无本次会话残留。

- [ ] **Step 6: 若真实复验通过，推送本地 main**

```powershell
git push origin main
```

Expected: 远端 `main` 更新到本次修复提交，GitHub CI 开始运行。
