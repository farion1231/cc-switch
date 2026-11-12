//! 检测结果缓存
//!
//! 多数客户端每轮都会把全部历史重发一遍（Responses 用 previous_response_id 的增量请求除外），
//! 绝大部分文本字段和上一轮逐字节相同。按整个字段的原文缓存合并后的命中区间，下一轮就只需要
//! 对新内容跑正则；哈希仍要扫一遍全文，端到端开销还是随总长度线性增长。
//!
//! - 键是 `SHA-256(长度 ‖ 原文)`，值只有区间，不保存原文。
//! - 粒度是整个文本字段，不切块：gitleaks 关键词可能离匹配点很远，allowlist 的 `line` 目标和
//!   上下文回溯也会因切块改变结果。
//! - 区间与占位符语言无关，换语言不影响命中。
//! - 有界：条目数和区间总数各有上限，按最近使用淘汰；单条区间过多的结果不缓存。

use super::Span;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

/// 短于这个长度的文本直接检测，不占缓存条目。经验值，尚无基准测量支撑
const MIN_CACHED_LEN: usize = 256;
const MAX_ENTRIES: usize = 4096;
/// 单条结果的区间数上限，超过的不缓存
const MAX_SPANS_PER_ENTRY: usize = 4096;
const MAX_TOTAL_SPANS: usize = 256 * 1024;

type Key = [u8; 32];

pub(super) struct DetectionCache {
    inner: Mutex<Inner>,
}

struct Entry {
    spans: Arc<[Span]>,
    /// 最近一次使用时的计数，用来识别 `order` 里过期的记录
    stamp: u64,
}

#[derive(Default)]
struct Inner {
    map: HashMap<Key, Entry>,
    /// 按使用先后排列的 (键, stamp)。命中时追加新记录而不移动旧记录，旧记录在淘汰时跳过
    order: VecDeque<(Key, u64)>,
    tick: u64,
    total_spans: usize,
}

impl DetectionCache {
    pub(super) fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    /// 命中缓存就直接返回，否则调用 `detect` 并缓存结果。检测时不持锁。
    pub(super) fn get_or_detect(
        &self,
        text: &str,
        detect: impl FnOnce() -> Vec<Span>,
    ) -> Arc<[Span]> {
        if text.len() < MIN_CACHED_LEN {
            return detect().into();
        }
        let key: Key = Sha256::new()
            .chain_update((text.len() as u64).to_le_bytes())
            .chain_update(text.as_bytes())
            .finalize()
            .into();
        if let Some(spans) = self.lock().get(&key) {
            return spans;
        }
        let spans: Arc<[Span]> = detect().into();
        if spans.len() <= MAX_SPANS_PER_ENTRY {
            self.lock().insert(key, spans.clone());
        }
        spans
    }

    /// 缓存里只有区间，锁中毒时数据仍然完整，继续使用
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.lock().map.len()
    }
}

impl Inner {
    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    fn get(&mut self, key: &Key) -> Option<Arc<[Span]>> {
        let tick = self.next_tick();
        let entry = self.map.get_mut(key)?;
        entry.stamp = tick;
        let spans = entry.spans.clone();
        self.order.push_back((*key, tick));
        self.compact_order();
        Some(spans)
    }

    fn insert(&mut self, key: Key, spans: Arc<[Span]>) {
        // 并发时别的请求可能已经存进来了，结果相同
        if self.map.contains_key(&key) {
            return;
        }
        let tick = self.next_tick();
        self.total_spans += spans.len();
        self.map.insert(key, Entry { spans, stamp: tick });
        self.order.push_back((key, tick));
        while self.map.len() > MAX_ENTRIES || self.total_spans > MAX_TOTAL_SPANS {
            if !self.evict_oldest() {
                break;
            }
        }
        self.compact_order();
    }

    fn evict_oldest(&mut self) -> bool {
        while let Some((key, stamp)) = self.order.pop_front() {
            if self.map.get(&key).is_some_and(|e| e.stamp == stamp) {
                if let Some(entry) = self.map.remove(&key) {
                    self.total_spans -= entry.spans.len();
                }
                return true;
            }
        }
        false
    }

    /// 命中会不断追加记录，过期记录太多时清理一遍，保证 `order` 的长度有界
    fn compact_order(&mut self) {
        if self.order.len() > 2 * self.map.len() + 64 {
            let map = &self.map;
            self.order
                .retain(|(key, stamp)| map.get(key).is_some_and(|e| e.stamp == *stamp));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Kind;
    use super::*;

    fn span(start: usize) -> Vec<Span> {
        vec![Span {
            start,
            end: start + 1,
            kind: Kind::Email,
        }]
    }

    fn text(i: usize) -> String {
        format!("{i:0>width$}", width = MIN_CACHED_LEN)
    }

    #[test]
    fn short_text_is_not_cached() {
        let cache = DetectionCache::new();
        cache.get_or_detect("short", || span(0));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn hit_skips_detection() {
        let cache = DetectionCache::new();
        let t = text(1);
        assert_eq!(cache.get_or_detect(&t, || span(3))[0].start, 3);
        let again = cache.get_or_detect(&t, || panic!("should hit the cache"));
        assert_eq!(again[0].start, 3);
    }

    #[test]
    fn evicts_least_recently_used_and_stays_bounded() {
        let cache = DetectionCache::new();
        let keep = text(0);
        cache.get_or_detect(&keep, || span(0));
        for i in 1..MAX_ENTRIES * 2 {
            cache.get_or_detect(&text(i), || span(i));
            // 一直在用的条目不应被淘汰
            cache.get_or_detect(&keep, || panic!("recently used entry was evicted"));
        }
        assert!(cache.len() <= MAX_ENTRIES);
        let inner = cache.lock();
        assert!(inner.order.len() <= 2 * inner.map.len() + 64);
        assert_eq!(
            inner.total_spans,
            inner.map.values().map(|e| e.spans.len()).sum::<usize>()
        );
    }

    #[test]
    fn oversized_results_are_not_cached() {
        let cache = DetectionCache::new();
        let many: Vec<Span> = (0..=MAX_SPANS_PER_ENTRY).flat_map(span).collect();
        cache.get_or_detect(&text(7), || many);
        assert_eq!(cache.len(), 0);
    }
}
