//! Remote 事件出口抽象
//!
//! 鉴权/配对路径不需要 GUI 运行时。把事件出口抽成可替换的 [`EventSink`]，
//! 使 `RemoteState` 不必为了发事件而硬持 `tauri::AppHandle`，从而可以在
//! router 级测试里构造（`tauri::test::mock_app()` 给的是 `AppHandle<MockRuntime>`，
//! 与默认 Wry 不是同一个类型）。

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// 桌面端监听的审批事件名
pub const APPROVAL_EVENT: &str = "remote-approval-required";

/// 证据链来源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// 来自 tailnet（对端节点身份可验证）
    Tailnet,
    /// 来自本机回环地址
    Loopback,
    /// 对端不在本 tailnet 内：tailscale 明确回了 `peer not found`
    ///
    /// 与 `Unavailable` 分开，是因为这条判定是**确定性的否定**——CLI 认识这个
    /// 地址并说它不在网内；而 `Unavailable` 是「没查出来」，两者不该被审批
    /// 逻辑当成同一件事。
    NotInTailnet,
    /// 来源无法判定
    Unavailable,
}

/// 审批请求的来源证据
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalEvidence {
    pub source_kind: SourceKind,
    pub node_name: Option<String>,
    pub node_stable_id: Option<String>,
    pub login_name: Option<String>,
    pub is_foreign_account: bool,
    pub peer_addr: String,
}

impl ApprovalEvidence {
    /// 构造一条回环来源的证据（无 tailnet 身份信息）
    pub fn loopback(peer_addr: impl Into<String>) -> Self {
        Self {
            source_kind: SourceKind::Loopback,
            node_name: None,
            node_stable_id: None,
            login_name: None,
            is_foreign_account: false,
            peer_addr: peer_addr.into(),
        }
    }
}

/// 审批请求事件
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalEvent {
    pub request_id: String,
    pub client_name: String,
    pub user_code: String,
    pub evidence: ApprovalEvidence,
}

/// 事件出口。实现必须是廉价且不阻塞的：调用方在请求处理路径上同步调用。
pub trait EventSink: Send + Sync {
    /// 发出一条审批事件。无订阅者时静默丢弃，不得 panic。
    fn emit(&self, ev: &ApprovalEvent);

    /// 通知桌面端前端一个普通事件（如 `provider-switched`）。
    /// 默认实现为空 —— 没有前端订阅者时静默丢弃。
    fn notify_frontend(&self, _event: &str, _payload: serde_json::Value) {}
}

/// 生产实现：真的包装 `tauri::AppHandle::emit`
pub struct AppHandleSink {
    app: tauri::AppHandle,
}

impl AppHandleSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }

    fn emit_json(&self, event: &str, payload: impl serde::Serialize + Clone) -> Result<(), String> {
        use tauri::Emitter;
        self.app.emit(event, payload).map_err(|e| e.to_string())
    }
}

impl EventSink for AppHandleSink {
    fn emit(&self, ev: &ApprovalEvent) {
        // 无监听者时 emit 返回 Err，忽略即可（静默丢弃）
        let _ = self.emit_json(APPROVAL_EVENT, ev);
    }

    fn notify_frontend(&self, event: &str, payload: serde_json::Value) {
        let _ = self.emit_json(event, payload);
    }
}

/// 测试实现：把事件记在内存里
#[allow(dead_code)] // 生产构建里没有构造点；用它的只有 test 配置与 `#[cfg(test)] mod e2e`
#[derive(Debug, Default, Clone)]
pub struct RecordingSink {
    events: std::sync::Arc<Mutex<Vec<ApprovalEvent>>>,
}

impl RecordingSink {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// 返回已记录的事件快照
    #[allow(dead_code)]
    pub fn recorded(&self) -> Vec<ApprovalEvent> {
        self.events
            .lock()
            .expect("RecordingSink mutex poisoned")
            .clone()
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, ev: &ApprovalEvent) {
        self.events
            .lock()
            .expect("RecordingSink mutex poisoned")
            .push(ev.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::{ApprovalEvent, ApprovalEvidence, EventSink, RecordingSink};

    /// 空实现：任何事件都丢弃，用于验证「无订阅者不 panic」。
    /// 只被本模块的用例取用，故随用例一起挂在 test 配置下。
    #[derive(Debug, Default, Clone, Copy)]
    struct NullSink;

    impl EventSink for NullSink {
        fn emit(&self, _ev: &ApprovalEvent) {}
    }

    /// emit 一次 → recorded 长度 1，且证据里的 peer_addr 原样保留。
    #[test]
    fn recording_sink_captures_approval_event() {
        let recording = RecordingSink::new();

        recording.emit(&ApprovalEvent {
            request_id: "r1".into(),
            client_name: "cli".into(),
            user_code: "AB12".into(),
            evidence: ApprovalEvidence::loopback("127.0.0.1:5000"),
        });

        assert_eq!(recording.recorded().len(), 1);
        assert_eq!(recording.recorded()[0].evidence.peer_addr, "127.0.0.1:5000");
    }

    /// 生产路径不许出现裸 `app_handle.emit(` —— 事件必须走 sink。
    #[test]
    fn app_handle_sink_is_used_by_production_path() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("remote")
            .join("handlers.rs");
        let content = std::fs::read_to_string(&src)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", src.display()));

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            assert!(
                !trimmed.contains("app_handle.emit("),
                "handlers.rs:{} still emits directly instead of going through the sink: {}",
                i + 1,
                trimmed
            );
        }
    }

    /// 边界条件：无订阅者 → 静默丢弃，不 panic。
    #[test]
    fn emit_without_subscribers_is_silently_dropped() {
        let sink = NullSink;
        sink.emit(&ApprovalEvent {
            request_id: "r1".into(),
            client_name: "cli".into(),
            user_code: "AB12".into(),
            evidence: ApprovalEvidence::loopback("127.0.0.1:5000"),
        });
    }

    /// 边界条件：并发 emit 不丢事件（`Arc<Mutex<Vec<_>>>` 串行化）。
    #[test]
    fn concurrent_emits_are_all_recorded() {
        use std::sync::Arc;

        let recording = RecordingSink::new();
        let mut handles = Vec::new();

        for t in 0..64 {
            let sink = recording.clone();
            handles.push(std::thread::spawn(move || {
                sink.emit(&ApprovalEvent {
                    request_id: format!("r{t}"),
                    client_name: "cli".into(),
                    user_code: "AB12".into(),
                    evidence: ApprovalEvidence::loopback("127.0.0.1:5000"),
                });
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let recorded = recording.recorded();
        assert_eq!(recorded.len(), 64);
        let mut ids: Vec<String> = recorded.into_iter().map(|e| e.request_id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 64, "concurrent emits lost or duplicated events");

        // 确认 clone 共享同一份记录
        let shared: Arc<RecordingSink> = Arc::new(recording.clone());
        assert_eq!(shared.recorded().len(), 64);
    }
}
