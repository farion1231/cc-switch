use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use super::embedded_agent::EphemeralAgentSpec;
use super::models::{RemoteTargetConfig, RemoteTargetValidationError};

const REMOTE_PATH_SETUP: &str =
    "PATH='/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'${PATH:+:$PATH}; export PATH";

/// 为桌面端生成的远端命令建立确定性工具搜索路径，同时保留管理员提供的附加目录。
/// 标准目录必须位于原 PATH 之前，避免用户环境中的同名程序接管完整性校验和临时文件清理。
/// 脚本避免双引号，因为 Windows OpenSSH 会把 Rust 单参数中的双引号作为远端字面字符传递。
fn with_remote_command_environment(command: String) -> String {
    format!("{REMOTE_PATH_SETUP}; {command}")
}

/// 平台预检与 Agent 生命周期命令共享同一远端环境，避免非交互式 shell 的 PATH 差异改变错误阶段。
pub fn build_preflight_command() -> String {
    with_remote_command_environment("command uname -s; command uname -m".to_string())
}

/// 构造 scp 参数数组，不经过本地 shell。路径中的空格保持为单个 OsString，避免命令拼接
/// 引入参数注入；远端路径只能来自 EphemeralAgentSpec 生成的十六进制 token。
pub fn build_scp_args(
    target: &RemoteTargetConfig,
    local_path: &Path,
    remote_path: &str,
) -> Result<Vec<OsString>, RemoteTargetValidationError> {
    let target = target.clone().normalize()?;
    let mut args = vec![
        OsString::from("-o"),
        OsString::from("BatchMode=yes"),
        OsString::from("-o"),
        OsString::from("StrictHostKeyChecking=yes"),
    ];
    if let Some(port) = target.port {
        args.push(OsString::from("-P"));
        args.push(OsString::from(port.to_string()));
    }
    if let Some(identity_file) = target.identity_file {
        args.push(OsString::from("-i"));
        args.push(OsString::from(identity_file));
    }
    args.push(local_path.as_os_str().to_os_string());
    let host = match target.username {
        Some(username) => format!("{username}@{}", target.host_alias),
        None => target.host_alias,
    };
    args.push(OsString::from(format!("{host}:{remote_path}")));
    Ok(args)
}

/// 远端 shell 仅插入本地生成的十六进制路径、十进制长度与 SHA-256，不包含任何用户输入。
/// trap 覆盖正常退出和常见终止信号；桌面端清理守卫还会通过独立 SSH 做一次兜底删除。
/// 变量名不能使用 zsh 与 PATH 绑定的特殊参数 path，否则赋值后所有外部校验命令都会失效。
pub fn build_launch_command(spec: &EphemeralAgentSpec) -> String {
    let command = format!(
        "cc_switch_agent_path='{path}'; \
cleanup() {{ command rm -f -- $cc_switch_agent_path; }}; \
trap cleanup EXIT HUP INT TERM; \
actual_size=$(command wc -c < $cc_switch_agent_path | command tr -d '[:space:]'); \
if [ x$actual_size != x'{length}' ]; then echo 'AGENT_INTEGRITY_FAILED: size' >&2; exit 70; fi; \
actual_sha=$(command sha256sum -- $cc_switch_agent_path | command awk '{{print $1}}'); \
if [ x$actual_sha != x'{sha256}' ]; then echo 'AGENT_INTEGRITY_FAILED: sha256' >&2; exit 71; fi; \
command chmod 700 -- $cc_switch_agent_path || exit 72; \
$cc_switch_agent_path --stdio",
        path = spec.remote_path,
        length = spec.length,
        sha256 = spec.sha256,
    );
    with_remote_command_environment(command)
}

pub fn build_cleanup_command(remote_path: &str) -> String {
    with_remote_command_environment(format!("command rm -f -- '{remote_path}'"))
}

/// 清理执行器由 SSH 层实现，守卫只负责“至多调度一次”的生命周期语义。接口允许测试记录
/// 调度而不启动真实进程，也让上传失败和握手失败复用同一个兜底路径。
pub trait CleanupScheduler: Send + Sync {
    fn schedule(&self, target: &RemoteTargetConfig, remote_path: &str);
}

pub struct EphemeralCleanupGuard {
    target: RemoteTargetConfig,
    remote_path: String,
    scheduler: Arc<dyn CleanupScheduler>,
    scheduled: bool,
}

impl EphemeralCleanupGuard {
    pub fn new(
        target: RemoteTargetConfig,
        remote_path: String,
        scheduler: Arc<dyn CleanupScheduler>,
    ) -> Self {
        Self {
            target,
            remote_path,
            scheduler,
            scheduled: false,
        }
    }

    pub fn schedule_cleanup(&mut self) {
        if self.scheduled {
            return;
        }
        self.scheduled = true;
        self.scheduler.schedule(&self.target, &self.remote_path);
    }
}

impl Drop for EphemeralCleanupGuard {
    fn drop(&mut self) {
        self.schedule_cleanup();
    }
}
