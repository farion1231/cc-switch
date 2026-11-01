//! 在飞请求注册表
//!
//! 记录「此刻正在上游跑」的请求明细，供用量页把在飞行置顶展示。
//!
//! 生命周期与 [`crate::proxy::forwarder::ActiveConnectionGuard`] 完全一致：
//! guard 在选到 provider 时登记条目，在响应（含流式 body）结束时随 Drop 注销。
//! 用 guard 而不是另立一套簿记，是刻意的——故障转移、整流重试、流式中断这些
//! 边界情况已经在 guard 的生命周期里处理对了，再来一套迟早会漂移。

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 一条在飞请求的快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InFlightRequest {
    /// 注册表内部 id，前端用它做 key，也用来识别「这条飞完了」
    pub id: u64,
    pub app_type: String,
    pub provider_id: String,
    pub provider_name: String,
    /// 实际发往上游的模型名；还没解析出来时为空
    pub model: Option<String>,
    /// 请求开始时刻（RFC 3339）。耗时由前端按本地时钟算，后端不需要每秒推送
    pub started_at: String,
    pub streaming: bool,
}

/// 在飞请求注册表。放在 `ProxyStatus` 里随 `Arc` 流转，代理重启即清空。
///
/// 必须实现 `Debug`：`ProxyStatus` derive 了 `Debug`，缺了会把整个枚举的
/// derive 链拽断（CI 第一次编译就是这么红的）。
#[derive(Debug, Default)]
pub struct InFlightRegistry {
    /// 顺序即开始顺序；规模有上限（见 [`Self::MAX`]），正常情况下并发在飞
    /// 远到不了这个数。
    entries: RwLock<Vec<InFlightRequest>>,
    next_id: AtomicU64,
}

impl InFlightRegistry {
    /// 上限兜底：注册表每条请求登记/注销各一次写锁，超限说明有泄漏而不是负载，
    /// 拒绝新条目并让旧条目自然过期，比无界增长安全。
    const MAX: usize = 200;

    /// 登记（或改写）一条在飞请求，返回它的 id。
    ///
    /// `existing_id` 传 `Some` 表示故障转移把这条请求挪到了新 provider 名下：
    /// 同一条请求换行显示，而不是旧行还在旧家、新行又冒出来。
    pub async fn upsert(
        &self,
        existing_id: Option<u64>,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
        model: Option<String>,
        streaming: bool,
    ) -> Option<u64> {
        let mut entries = self.entries.write().await;

        if let Some(id) = existing_id {
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                entry.provider_id = provider_id.to_string();
                entry.provider_name = provider_name.to_string();
                if model.is_some() {
                    entry.model = model;
                }
                entry.streaming = streaming;
                return Some(id);
            }
            // 找不到说明条目已被注销，别重新登记一条僵尸
            return None;
        }

        if entries.len() >= Self::MAX {
            return None;
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        entries.push(InFlightRequest {
            id,
            app_type: app_type.to_string(),
            provider_id: provider_id.to_string(),
            provider_name: provider_name.to_string(),
            model,
            started_at: chrono::Utc::now().to_rfc3339(),
            streaming,
        });
        Some(id)
    }

    /// 注销一条在飞请求（guard Drop 时调用；id 不存在时静默返回）。
    pub async fn remove(&self, id: u64) {
        self.entries.write().await.retain(|e| e.id != id);
    }

    /// 当前快照，按开始时间从早到晚。
    pub async fn snapshot(&self) -> Vec<InFlightRequest> {
        let mut entries = self.entries.read().await.clone();
        entries.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        entries
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_creates_then_updates_the_same_entry() {
        let registry = InFlightRegistry::default();

        let id = registry
            .upsert(
                None,
                "claude",
                "p1",
                "Provider One",
                Some("m1".into()),
                true,
            )
            .await
            .expect("registered");

        // 故障转移：同一条请求挪到新 provider 名下，不能变成两条
        registry
            .upsert(Some(id), "claude", "p2", "Provider Two", None, true)
            .await
            .expect("moved");

        let snapshot = registry.snapshot().await;
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].provider_id, "p2");
        // 没给新模型时保留旧值——换 provider 不代表换模型
        assert_eq!(snapshot[0].model.as_deref(), Some("m1"));
    }

    #[tokio::test]
    async fn remove_drops_the_entry_and_is_idempotent() {
        let registry = InFlightRegistry::default();
        let id = registry
            .upsert(None, "claude", "p1", "P", None, false)
            .await
            .unwrap();

        registry.remove(id).await;
        registry.remove(id).await;

        assert!(registry.snapshot().await.is_empty());
    }

    #[tokio::test]
    async fn moving_a_removed_entry_does_not_resurrect_it() {
        let registry = InFlightRegistry::default();
        let id = registry
            .upsert(None, "claude", "p1", "P", None, false)
            .await
            .unwrap();
        registry.remove(id).await;

        // guard 先 drop、故障转移的 upsert 后到（spawn 出去的减量竞态）：
        // 不能把一条已经结束的请求重新登记成在飞
        let again = registry
            .upsert(Some(id), "claude", "p2", "P2", None, false)
            .await;

        assert_eq!(again, None);
        assert!(registry.snapshot().await.is_empty());
    }

    #[tokio::test]
    async fn snapshot_is_sorted_by_start_time_oldest_first() {
        let registry = InFlightRegistry::default();
        let a = registry
            .upsert(None, "claude", "p1", "A", None, false)
            .await
            .unwrap();
        let b = registry
            .upsert(None, "claude", "p2", "B", None, false)
            .await
            .unwrap();
        let c = registry
            .upsert(None, "claude", "p3", "C", None, false)
            .await
            .unwrap();

        let snapshot = registry.snapshot().await;
        let ids: Vec<u64> = snapshot.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![a, b, c]);
        assert_eq!(registry.len().await, 3);
        assert!(!registry.is_empty().await);
    }

    #[tokio::test]
    async fn registration_is_capped() {
        let registry = InFlightRegistry::default();
        for _ in 0..InFlightRegistry::MAX {
            registry
                .upsert(None, "claude", "p", "P", None, false)
                .await
                .unwrap();
        }

        // 满了拒绝新条目，而不是无界增长
        assert_eq!(
            registry.upsert(None, "claude", "p", "P", None, false).await,
            None
        );
        assert_eq!(registry.len().await, InFlightRegistry::MAX);
    }
}
