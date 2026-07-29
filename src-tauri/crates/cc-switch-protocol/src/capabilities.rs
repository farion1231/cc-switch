use std::collections::HashMap;

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
    /// 第一阶段只开放供应商纵向闭环。这里使用显式白名单，避免桌面端后续新增命令时
    /// 被 Agent 自动暴露；扩展远程能力前必须先定义安全语义、超时和幂等性。
    pub fn provider_phase() -> Self {
        let mut commands = HashMap::new();
        for capability in [
            CommandCapability {
                name: "provider.list",
                read_only: true,
                idempotent: true,
                timeout_ms: 30_000,
            },
            CommandCapability {
                name: "provider.current",
                read_only: true,
                idempotent: true,
                timeout_ms: 30_000,
            },
            CommandCapability {
                name: "provider.add",
                read_only: false,
                idempotent: false,
                timeout_ms: 30_000,
            },
            CommandCapability {
                name: "provider.update",
                read_only: false,
                idempotent: false,
                timeout_ms: 30_000,
            },
            CommandCapability {
                name: "provider.delete",
                read_only: false,
                idempotent: false,
                timeout_ms: 30_000,
            },
            CommandCapability {
                name: "provider.switch",
                read_only: false,
                idempotent: false,
                timeout_ms: 60_000,
            },
            CommandCapability {
                name: "provider.update_sort_order",
                read_only: false,
                idempotent: false,
                timeout_ms: 30_000,
            },
        ] {
            commands.insert(capability.name, capability);
        }
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

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RemoteCapabilityError {
    #[error("远程命令未开放: {0}")]
    CommandNotExposed(String),
}
