//! 本机大文件基准（设计 §7.2），默认不跑：
//!
//! ```text
//! CC_SWITCH_BENCH_PROVIDER=claude CC_SWITCH_BENCH_FILE=<jsonl> \
//!   cargo test --release --lib bench_parse_env_file -- --ignored --nocapture
//! ```
//!
//! 输出：冷解析耗时（3 次中位数）、全量 payload 及按块类型 / 字段的构成、
//! 首包（Header + 首个 Messages）的大小与产出时间、缓存命中后再次读取的耗时。
//! 设置 `CC_SWITCH_BENCH_DUMP=<path>` 时把 `get_session_messages` 的返回写到该文件，
//! 供前端冒烟 / 基准脚本使用。

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::Value;

use super::cache::{self, chunk_ranges, Transcript};
use super::model::TranscriptChunk;
use super::{load_messages, load_transcript};

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// 按「块类型.字段」累计序列化字节，找出 payload 的主要构成
fn field_breakdown(messages: &Value) -> BTreeMap<String, (usize, usize)> {
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut add = |key: String, value: &Value| {
        let entry = out.entry(key).or_default();
        entry.0 += 1;
        entry.1 += serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0);
    };
    for message in messages.as_array().into_iter().flatten() {
        let Some(object) = message.as_object() else {
            continue;
        };
        for (key, value) in object {
            if key == "blocks" {
                for block in value.as_array().into_iter().flatten() {
                    let ty = block["type"].as_str().unwrap_or("?");
                    add(format!("block.{ty}"), block);
                    for (field, v) in block.as_object().into_iter().flatten() {
                        add(format!("block.{ty}.{field}"), v);
                    }
                }
            } else {
                add(format!("message.{key}"), value);
            }
        }
    }
    out
}

#[test]
#[ignore]
fn bench_parse_env_file() {
    let (Ok(provider), Ok(file)) = (
        std::env::var("CC_SWITCH_BENCH_PROVIDER"),
        std::env::var("CC_SWITCH_BENCH_FILE"),
    ) else {
        eprintln!("CC_SWITCH_BENCH_PROVIDER / CC_SWITCH_BENCH_FILE 未设置，跳过");
        return;
    };
    let file_len = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);

    // 1. 冷解析（不经缓存）：解析 + 建索引 / 序列化估算
    let mut parse_runs = Vec::new();
    let mut build_runs = Vec::new();
    let mut transcript = Transcript::default();
    for _ in 0..3 {
        let start = Instant::now();
        let messages = load_messages(&provider, &file).expect("load_messages");
        parse_runs.push(ms(start));
        let start = Instant::now();
        transcript = Transcript::new(messages);
        build_runs.push(ms(start));
    }

    // 2. 走命令同款路径：校验 → 指纹 → 缓存 → 首包
    cache::global().remove(&provider, &file);
    let start = Instant::now();
    let loaded = load_transcript(&provider, &file).expect("load_transcript");
    assert!(!loaded.cached);
    let header = TranscriptChunk::Header {
        total: loaded.transcript.messages.len(),
        turns: loaded.transcript.turns.clone(),
        cached: loaded.cached,
        parse_ms: loaded.parse_ms,
    };
    let header_bytes = serde_json::to_vec(&header).unwrap().len();
    let ranges = chunk_ranges(&loaded.transcript.message_bytes);
    let first_chunk_bytes = ranges
        .first()
        .map(|&(s, e)| {
            serde_json::to_vec(&TranscriptChunk::Messages {
                start: s,
                messages: loaded.transcript.messages[s..e].to_vec(),
            })
            .unwrap()
            .len()
        })
        .unwrap_or(0);
    let cold_first_chunk_ms = ms(start);

    // 超过缓存字节上限（96MB）的会话不进缓存，再次读取仍是冷解析
    let mut hit_runs = Vec::new();
    let mut hit_cached = true;
    for _ in 0..3 {
        let start = Instant::now();
        let hit = load_transcript(&provider, &file).expect("second load");
        hit_cached &= hit.cached;
        let _ = serde_json::to_vec(&TranscriptChunk::Header {
            total: hit.transcript.messages.len(),
            turns: hit.transcript.turns.clone(),
            cached: true,
            parse_ms: hit.parse_ms,
        })
        .unwrap();
        if let Some(&(s, e)) = ranges.first() {
            let _ = serde_json::to_vec(&hit.transcript.messages[s..e]).unwrap();
        }
        hit_runs.push(ms(start));
    }

    // 3. payload 构成
    let payload = serde_json::to_vec(&transcript.messages).unwrap();
    if let Ok(dump) = std::env::var("CC_SWITCH_BENCH_DUMP") {
        std::fs::write(dump, &payload).unwrap();
    }
    let max_message = transcript.message_bytes.iter().copied().max().unwrap_or(0);
    let value: Value = serde_json::from_slice(&payload).unwrap();

    eprintln!("== {provider}: {file} ({:.1} MB)", file_len as f64 / 1e6);
    eprintln!(
        "parse ms {:?} median {:.1}; Transcript::new median {:.1}",
        parse_runs
            .iter()
            .map(|v| (v * 10.0).round() / 10.0)
            .collect::<Vec<_>>(),
        median(parse_runs.clone()),
        median(build_runs)
    );
    eprintln!(
        "messages {}, turns {}, chunks {}",
        transcript.messages.len(),
        transcript.turns.len(),
        ranges.len()
    );
    eprintln!(
        "payload {:.3} MB, max message {:.1} KB",
        payload.len() as f64 / 1e6,
        max_message as f64 / 1e3
    );
    eprintln!(
        "cold → first chunk {:.1} ms (header {:.1} KB + first chunk {:.1} KB); {} → first chunk {:?} ms",
        cold_first_chunk_ms,
        header_bytes as f64 / 1e3,
        first_chunk_bytes as f64 / 1e3,
        if hit_cached {
            "cache hit"
        } else {
            "NOT cached (over cache cap)"
        },
        hit_runs
            .iter()
            .map(|v| (v * 100.0).round() / 100.0)
            .collect::<Vec<_>>()
    );
    let mut rows: Vec<_> = field_breakdown(&value).into_iter().collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.1 .1));
    for (key, (count, bytes)) in rows.into_iter().filter(|r| r.1 .1 >= 5_000) {
        eprintln!("  {key:<32} {count:>6} × {:>9.1} KB", bytes as f64 / 1e3);
    }
}
