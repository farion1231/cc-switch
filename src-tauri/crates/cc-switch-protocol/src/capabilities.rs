use std::collections::HashMap;

/// 远程命令的安全与调度元数据；桌面 runtime 和 Agent 握手必须共享同一份定义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCapability {
    pub name: &'static str,
    pub read_only: bool,
    pub idempotent: bool,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct CommandCapabilityRegistry {
    commands: HashMap<&'static str, CommandCapability>,
}

impl CommandCapabilityRegistry {
    /// 返回当前 Agent 明确开放的完整领域白名单；新增命令必须先定义幂等性和超时。
    pub fn remote_supported() -> Self {
        Self::from_capabilities(
            provider_capabilities()
                .into_iter()
                .chain(usage_capabilities()),
        )
    }

    fn from_capabilities(capabilities: impl IntoIterator<Item = CommandCapability>) -> Self {
        let commands = capabilities
            .into_iter()
            .map(|capability| (capability.name, capability))
            .collect();
        Self { commands }
    }

    pub fn require(&self, command: &str) -> Result<&CommandCapability, RemoteCapabilityError> {
        self.commands
            .get(command)
            .ok_or_else(|| RemoteCapabilityError::CommandNotExposed(command.to_string()))
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.commands.keys().copied()
    }
}

fn provider_capabilities() -> [CommandCapability; 7] {
    [
        capability("provider.list", true, true, 30_000),
        capability("provider.current", true, true, 30_000),
        capability("provider.add", false, false, 30_000),
        capability("provider.update", false, false, 30_000),
        capability("provider.delete", false, false, 30_000),
        capability("provider.switch", false, false, 60_000),
        capability("provider.update_sort_order", false, false, 30_000),
    ]
}

fn usage_capabilities() -> [CommandCapability; 16] {
    [
        capability("usage.summary", true, true, 30_000),
        capability("usage.summary_by_app", true, true, 30_000),
        capability("usage.trends", true, true, 30_000),
        capability("usage.provider_stats", true, true, 30_000),
        capability("usage.model_stats", true, true, 30_000),
        capability("usage.logs", true, true, 30_000),
        capability("usage.detail", true, true, 30_000),
        capability("usage.data_sources", true, true, 30_000),
        capability("usage.pricing.list", true, true, 30_000),
        capability("usage.limits", true, true, 30_000),
        capability("usage.provider_query", true, true, 30_000),
        capability("usage.pricing.update", false, false, 30_000),
        capability("usage.pricing.delete", false, false, 30_000),
        capability("usage.provider_test", false, false, 30_000),
        capability("usage.session_sync", false, false, 300_000),
        capability("usage.codex_rebuild", false, false, 300_000),
    ]
}

const fn capability(
    name: &'static str,
    read_only: bool,
    idempotent: bool,
    timeout_ms: u64,
) -> CommandCapability {
    CommandCapability {
        name,
        read_only,
        idempotent,
        timeout_ms,
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RemoteCapabilityError {
    #[error("远程命令未开放: {0}")]
    CommandNotExposed(String),
}
