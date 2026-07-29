use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentArchitecture {
    X86_64,
    Aarch64,
}

impl AgentArchitecture {
    pub fn parse(value: &str) -> Result<Self, EmbeddedAgentError> {
        match value {
            "x86_64" | "amd64" => Ok(Self::X86_64),
            "aarch64" | "arm64" => Ok(Self::Aarch64),
            other => Err(EmbeddedAgentError::UnsupportedArchitecture(
                other.to_string(),
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

/// 单次 SSH 会话使用的 Agent 元数据。
///
/// 该结构不保存本地文件路径，防止生产连接退回外部 artifact；真正的嵌入字节由 Task 6
/// 的构建边界提供，此处只计算投递和远端校验需要的不可变信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EphemeralAgentSpec {
    pub architecture: AgentArchitecture,
    pub remote_path: String,
    pub length: usize,
    pub sha256: String,
}

impl EphemeralAgentSpec {
    pub fn for_architecture(architecture: &str, bytes: &[u8]) -> Result<Self, EmbeddedAgentError> {
        let architecture = AgentArchitecture::parse(architecture)?;
        let token = Uuid::new_v4().simple().to_string();
        Ok(Self {
            architecture,
            remote_path: format!("/tmp/cc-switch-agent-{token}"),
            length: bytes.len(),
            sha256: sha256_hex(bytes),
        })
    }
}

/// build.rs 将发布 workflow 提供的 musl 产物复制到 OUT_DIR；include_bytes! 使 Windows、
/// macOS 和 Linux 桌面包都持有相同的两份字节，不依赖应用运行机器安装 Linux 工具链。
const LINUX_X86_64_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/cc-switch-agent-linux-x86_64"));
const LINUX_AARCH64_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/cc-switch-agent-linux-aarch64"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedAgentEntry {
    pub architecture: AgentArchitecture,
    pub bytes: &'static [u8],
    pub length: usize,
    pub sha256: String,
}

pub fn embedded_agent_catalog() -> [EmbeddedAgentEntry; 2] {
    [
        catalog_entry(AgentArchitecture::X86_64, LINUX_X86_64_BYTES),
        catalog_entry(AgentArchitecture::Aarch64, LINUX_AARCH64_BYTES),
    ]
}

fn catalog_entry(architecture: AgentArchitecture, bytes: &'static [u8]) -> EmbeddedAgentEntry {
    EmbeddedAgentEntry {
        architecture,
        bytes,
        length: bytes.len(),
        sha256: sha256_hex(bytes),
    }
}

pub fn embedded_agent_bytes(architecture: &str) -> Result<&'static [u8], EmbeddedAgentError> {
    let architecture = AgentArchitecture::parse(architecture)?;
    let bytes = match architecture {
        AgentArchitecture::X86_64 => LINUX_X86_64_BYTES,
        AgentArchitecture::Aarch64 => LINUX_AARCH64_BYTES,
    };
    if bytes.is_empty() {
        return Err(EmbeddedAgentError::ArtifactMissing(
            architecture.as_str().to_string(),
        ));
    }
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // 写入 String 不会产生格式错误；忽略 fmt::Result 可避免为纯内存编码扩大错误面。
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EmbeddedAgentError {
    #[error("远端架构不受支持: {0}")]
    UnsupportedArchitecture(String),
    #[error("桌面构建缺少内嵌 Agent: {0}")]
    ArtifactMissing(String),
}
