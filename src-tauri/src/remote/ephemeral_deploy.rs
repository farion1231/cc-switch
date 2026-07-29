use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use super::embedded_agent::EphemeralAgentSpec;
use super::models::{RemoteTargetConfig, RemoteTargetValidationError};

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
pub fn build_launch_command(spec: &EphemeralAgentSpec) -> String {
    format!(
        "path='{path}'; \
cleanup() {{ rm -f -- \"$path\"; }}; \
trap cleanup EXIT HUP INT TERM; \
actual_size=$(wc -c < \"$path\" | tr -d '[:space:]'); \
if [ \"$actual_size\" != '{length}' ]; then echo 'AGENT_INTEGRITY_FAILED: size' >&2; exit 70; fi; \
actual_sha=$(sha256sum -- \"$path\" | awk '{{print $1}}'); \
if [ \"$actual_sha\" != '{sha256}' ]; then echo 'AGENT_INTEGRITY_FAILED: sha256' >&2; exit 71; fi; \
chmod 700 -- \"$path\" || exit 72; \
\"$path\" --stdio",
        path = spec.remote_path,
        length = spec.length,
        sha256 = spec.sha256,
    )
}

pub fn build_cleanup_command(remote_path: &str) -> String {
    format!("rm -f -- '{remote_path}'")
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
