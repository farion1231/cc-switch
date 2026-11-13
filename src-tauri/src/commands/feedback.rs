//! 设置 → 关于 → 问题反馈：从应用日志中截取「反馈当日」的内容，连同版本与系统信息
//! 直接预填进 GitHub Issue 页面。只打开页面，提交由用户自己确认完成。
//!
//! 预填走 URL 参数，而浏览器 / GitHub 对 URL 长度有约 8 KB 的上限，整天的日志通常放不下，
//! 所以从当日日志末尾往前取，在长度预算内尽量多放最近的行。
//!
//! Issue 是公开的，日志在放入链接前一律经过隐私过滤器（`proxy::privacy_filter`）脱敏。

use crate::proxy::privacy_filter::{self, Placeholders};
use chrono::{Local, TimeZone};
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const CURRENT_LOG_FILE: &str = "cc-switch.log";
/// tauri-plugin-log 按大小轮转后的归档名为 `cc-switch_<时间戳>.log`
const ARCHIVE_LOG_PREFIX: &str = "cc-switch_";
/// 读取时只在内存里保留当日末尾这么多字节；URL 预算远小于此，足够挑选
const MAX_TAIL_BYTES: usize = 64 * 1024;
/// 单行过长（如整段请求体）时截断，避免一行就占满预算
const MAX_LOG_LINE_CHARS: usize = 800;
/// 预填 Issue 链接的长度上限（留出余量，低于常见的 8 KB 限制）
const MAX_ISSUE_URL_LEN: usize = 7000;
const DEFAULT_ISSUE_REPO: &str = "farion1231/cc-switch";
/// 仅开发构建读取：把 Issue 指向自己的 fork（`owner/repo`），用来验证模板预填。
/// GitHub 只从默认分支读取 Issue 模板，fork 需开启 Issues 且默认分支带有字段 id。
#[cfg(debug_assertions)]
const ISSUE_REPO_OVERRIDE_ENV: &str = "CC_SWITCH_FEEDBACK_REPO";
const ISSUE_TEMPLATE: &str = "bug_report.yml";
/// 脱敏占位符语言：Issue 内容面向维护者，统一用英文（[EMAIL]、[SECRET] 等）
const ISSUE_LANGUAGE: &str = "en";
/// bug_report.yml 中「操作系统」下拉框的选项，需与 `os_display_name` 的输出一致
const TEMPLATE_OS_OPTIONS: [&str; 3] = ["Windows", "macOS", "Linux"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackReport {
    /// 截取的日期（本地时区，YYYY-MM-DD）
    pub date: String,
    /// 当日日志总行数
    pub total_lines: usize,
    /// 实际写入 Issue 的行数（当日末尾部分）
    pub included_lines: usize,
    pub issue_url: String,
}

/// 准备问题反馈：截取当日日志并返回预填好的 Issue 链接（不会自动提交）
#[tauri::command]
pub async fn prepare_feedback_report() -> Result<FeedbackReport, String> {
    // 确保缓冲中的日志已落盘，再读取文件
    log::logger().flush();

    let report = tokio::task::spawn_blocking(|| {
        let log_dir = crate::panic_hook::get_log_dir();
        build_feedback_report(&log_dir, &FeedbackEnv::current())
    })
    .await
    .map_err(|e| format!("读取反馈日志失败: {e}"))?;

    log::info!(
        "[feedback] 已生成 {} 的问题反馈链接：附带 {}/{} 行日志",
        report.date,
        report.included_lines,
        report.total_lines
    );
    Ok(report)
}

/// 反馈所需的运行环境信息；单独抽出便于测试时注入固定值
struct FeedbackEnv {
    version: String,
    os: &'static str,
    arch: &'static str,
    now: chrono::DateTime<Local>,
    /// Issue 所在仓库（`owner/repo`）
    issue_repo: String,
}

impl FeedbackEnv {
    fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: os_display_name(std::env::consts::OS),
            arch: std::env::consts::ARCH,
            now: Local::now(),
            issue_repo: issue_repo(),
        }
    }
}

fn issue_repo() -> String {
    #[cfg(debug_assertions)]
    if let Ok(repo) = std::env::var(ISSUE_REPO_OVERRIDE_ENV) {
        let repo = repo.trim();
        if is_repo_slug(repo) {
            return repo.to_string();
        }
        log::warn!("[feedback] 忽略无效的 {ISSUE_REPO_OVERRIDE_ENV}={repo}，应为 owner/repo");
    }
    DEFAULT_ISSUE_REPO.to_string()
}

/// `owner/repo`：恰好两段，仅含 GitHub 允许的字符
#[cfg_attr(not(debug_assertions), allow(dead_code))]
fn is_repo_slug(value: &str) -> bool {
    let mut parts = value.split('/');
    let valid_part = |p: Option<&str>| {
        p.is_some_and(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    };
    valid_part(parts.next()) && valid_part(parts.next()) && parts.next().is_none()
}

fn os_display_name(os: &'static str) -> &'static str {
    match os {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    }
}

fn build_feedback_report(log_dir: &Path, env: &FeedbackEnv) -> FeedbackReport {
    let date = env.now.format("%Y-%m-%d").to_string();
    let files = todays_log_files(log_dir, start_of_local_day(&env.now));
    let collected = collect_day_tail(&files, &date, MAX_TAIL_BYTES);
    // Issue 是公开的：无论代理侧隐私过滤开关是否打开，上传前都要脱敏。
    // 先脱敏再按长度预算挑选行，占位符会改变行长。
    let filter = privacy_filter::global();
    let placeholders = Placeholders::for_language(ISSUE_LANGUAGE);
    let lines: Vec<String> = collected
        .lines
        .into_iter()
        .map(|line| filter.redact(&line, placeholders).redacted)
        .collect();
    let (issue_url, included_lines) = build_issue_url(env, &date, &lines, collected.total_lines);

    FeedbackReport {
        date,
        total_lines: collected.total_lines,
        included_lines,
        issue_url,
    }
}

fn start_of_local_day(now: &chrono::DateTime<Local>) -> SystemTime {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
        .map(SystemTime::from)
        // 午夜恰逢夏令时跳变等极端情况：退回到 24 小时前，宁可多读也不漏
        .unwrap_or_else(|| SystemTime::from(*now) - std::time::Duration::from_secs(24 * 60 * 60))
}

/// 列出可能含当日内容的日志文件：当前文件 + 当日有写入的轮转归档。
/// 按时间先后排序（归档按修改时间升序，当前文件最后）。
fn todays_log_files(log_dir: &Path, day_start: SystemTime) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return Vec::new();
    };

    let mut files: Vec<(bool, SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let is_current = name == CURRENT_LOG_FILE;
            let is_archive = name.starts_with(ARCHIVE_LOG_PREFIX) && name.ends_with(".log");
            if !is_current && !is_archive {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let modified = metadata.modified().ok()?;
            // 最后写入早于今天零点的归档不可能含有今天的内容
            (modified >= day_start).then_some((is_current, modified, path))
        })
        .collect();

    files.sort_by_key(|(is_current, modified, _)| (*is_current, *modified));
    files.into_iter().map(|(_, _, path)| path).collect()
}

struct DayTail {
    /// 当日末尾的若干行（受 `max_bytes` 限制）
    lines: VecDeque<String>,
    /// 当日日志总行数
    total_lines: usize,
}

/// 日志行形如 `[2026-11-11][10:20:30][INFO][target] message`，返回行首的日期部分。
/// 多行消息的续行没有该前缀，返回 None。
fn entry_date(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if bytes.len() < 12 || bytes[0] != b'[' || bytes[11] != b']' {
        return None;
    }
    let date = &line[1..11];
    let shape_ok = date.bytes().enumerate().all(|(i, b)| match i {
        4 | 7 => b == b'-',
        _ => b.is_ascii_digit(),
    });
    shape_ok.then_some(date)
}

/// 逐行读取日志，只保留日期为 `day` 的条目（多行条目的续行跟随所属条目），
/// 内存里只留末尾 `max_bytes` 字节的行，同时统计当日总行数。
fn collect_day_tail(files: &[PathBuf], day: &str, max_bytes: usize) -> DayTail {
    let mut lines = VecDeque::new();
    let mut total_bytes = 0usize;
    let mut total_lines = 0usize;

    for path in files {
        let Ok(file) = File::open(path) else {
            continue;
        };
        let mut reader = BufReader::new(file);
        let mut buf = Vec::new();
        // 每个文件从「非当日」开始；文件开头的续行属于上一个文件中的条目，无法归属，直接跳过
        let mut in_day = false;

        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let raw = String::from_utf8_lossy(&buf);
            let line = raw.trim_end_matches(['\r', '\n']);
            if let Some(date) = entry_date(line) {
                in_day = date == day;
            }
            if !in_day {
                continue;
            }

            total_lines += 1;
            let line = truncate_chars(line, MAX_LOG_LINE_CHARS);
            total_bytes += line.len() + 1;
            lines.push_back(line);
            while total_bytes > max_bytes {
                match lines.pop_front() {
                    Some(dropped) => total_bytes -= dropped.len() + 1,
                    None => break,
                }
            }
        }
    }

    DayTail { lines, total_lines }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => format!("{}…", &text[..idx]),
        None => text.to_string(),
    }
}

/// 预填到模板「补充信息」栏的内容：环境信息 + 当日日志末尾 `tail` 行
fn build_issue_context(env: &FeedbackEnv, date: &str, tail: &[String], total: usize) -> String {
    let mut ctx = String::new();
    ctx.push_str("**Environment (auto-filled by CC Switch)**\n");
    ctx.push_str(&format!("- Version: v{}\n", env.version));
    ctx.push_str(&format!("- OS: {} ({})\n", env.os, env.arch));
    if total == 0 {
        ctx.push_str(&format!("- Log: no entries recorded on {date}.\n"));
        return ctx;
    }
    if tail.len() < total {
        ctx.push_str(&format!(
            "- Log: last {} of {total} lines from {date} (earlier lines omitted due to URL length)\n",
            tail.len()
        ));
    } else {
        ctx.push_str(&format!("- Log: all {total} lines from {date}\n"));
    }
    ctx.push_str(
        "- Privacy: emails, phone numbers, ID/bank card numbers, IPs and secrets in the log were replaced with placeholders\n",
    );
    if !tail.is_empty() {
        ctx.push_str("\n```text\n");
        for line in tail {
            // 日志里的代码围栏会提前结束 Markdown 代码块
            ctx.push_str(&line.replace("```", "'''"));
            ctx.push('\n');
        }
        ctx.push_str("```\n");
    }
    ctx
}

fn issue_url_with(env: &FeedbackEnv, date: &str, tail: &[String], total: usize) -> String {
    let context = build_issue_context(env, date, tail, total);
    let issue_new_url = format!("https://github.com/{}/issues/new", env.issue_repo);
    let mut params = vec![
        ("template", ISSUE_TEMPLATE),
        ("version", env.version.as_str()),
    ];
    // 下拉框按选项文字预填，只传模板里存在的选项
    if TEMPLATE_OS_OPTIONS.contains(&env.os) {
        params.push(("os", env.os));
    }
    params.push(("context", context.as_str()));
    url::Url::parse_with_params(&issue_new_url, &params)
        .map(String::from)
        .unwrap_or(issue_new_url)
}

/// 生成预填的 bug_report Issue 链接，在长度预算内放入尽可能多的末尾日志行。
/// 返回（链接, 实际放入的行数）。
fn build_issue_url(
    env: &FeedbackEnv,
    date: &str,
    lines: &[String],
    total: usize,
) -> (String, usize) {
    let tail = |k: usize| &lines[lines.len() - k..];
    // URL 长度随放入行数单调增长，二分找出不超预算的最大行数
    let (mut lo, mut hi) = (0usize, lines.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if issue_url_with(env, date, tail(mid), total).len() <= MAX_ISSUE_URL_LEN {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    (issue_url_with(env, date, tail(lo), total), lo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_env() -> FeedbackEnv {
        FeedbackEnv {
            version: "9.9.9".to_string(),
            os: "Windows",
            arch: "x86_64",
            now: Local.with_ymd_and_hms(2026, 11, 11, 15, 30, 12).unwrap(),
            issue_repo: DEFAULT_ISSUE_REPO.to_string(),
        }
    }

    #[test]
    fn repo_slug_validation() {
        assert!(is_repo_slug("funkpopo/cc-switch"));
        assert!(is_repo_slug("a.b/c_d-1"));
        for bad in ["", "owner", "owner/", "/repo", "a/b/c", "a/b?x=1", "a b/c"] {
            assert!(!is_repo_slug(bad), "{bad}");
        }
    }

    #[test]
    fn os_param_only_for_template_options() {
        assert!(TEMPLATE_OS_OPTIONS
            .iter()
            .all(|o| ["windows", "macos", "linux"]
                .iter()
                .any(|raw| os_display_name(raw) == *o)));

        let mut env = test_env();
        env.os = "freebsd";
        let (url, _) = build_issue_url(&env, "2026-11-11", &[], 0);
        let parsed = url::Url::parse(&url).unwrap();
        assert!(!parsed.query_pairs().any(|(k, _)| k == "os"));
    }

    #[test]
    fn issue_url_targets_configured_repo() {
        let mut env = test_env();
        let (url, _) = build_issue_url(&env, "2026-11-11", &[], 0);
        assert!(url.starts_with("https://github.com/farion1231/cc-switch/issues/new?"));
        env.issue_repo = "funkpopo/cc-switch".to_string();
        let (url, _) = build_issue_url(&env, "2026-11-11", &[], 0);
        assert!(url.starts_with("https://github.com/funkpopo/cc-switch/issues/new?"));
    }

    fn context_of(url: &str) -> String {
        let parsed = url::Url::parse(url).unwrap();
        parsed
            .query_pairs()
            .find(|(k, _)| k == "context")
            .map(|(_, v)| v.into_owned())
            .unwrap()
    }

    #[test]
    fn entry_date_parses_only_prefixed_lines() {
        assert_eq!(
            entry_date("[2026-11-11][10:20:30][INFO][x] hi"),
            Some("2026-11-11")
        );
        assert_eq!(entry_date("  at some::frame"), None);
        assert_eq!(entry_date("[proxy] something else"), None);
        assert_eq!(entry_date("[2026-1x-11][10:20:30]"), None);
        assert_eq!(entry_date(""), None);
    }

    #[test]
    fn collects_only_target_day_with_continuation_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CURRENT_LOG_FILE);
        std::fs::write(
            &path,
            "[2026-11-10][23:59:59][INFO][a] yesterday\n\
             continuation of yesterday\n\
             [2026-11-11][00:00:01][INFO][a] today 1\r\n\
             stack line of today\n\
             [2026-11-11][09:00:00][ERROR][a] today 2\n",
        )
        .unwrap();

        let tail = collect_day_tail(&[path], "2026-11-11", usize::MAX);
        assert_eq!(tail.total_lines, 3);
        assert_eq!(
            tail.lines.iter().cloned().collect::<Vec<_>>(),
            vec![
                "[2026-11-11][00:00:01][INFO][a] today 1",
                "stack line of today",
                "[2026-11-11][09:00:00][ERROR][a] today 2",
            ]
        );
    }

    #[test]
    fn tail_keeps_latest_lines_and_counts_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CURRENT_LOG_FILE);
        let content: String = (0..100)
            .map(|i| format!("[2026-11-11][10:00:00][INFO][a] line {i:03}\n"))
            .collect();
        std::fs::write(&path, content).unwrap();

        let line_len = "[2026-11-11][10:00:00][INFO][a] line 000".len() + 1;
        let tail = collect_day_tail(&[path], "2026-11-11", line_len * 10);
        assert_eq!(tail.total_lines, 100);
        assert_eq!(tail.lines.len(), 10);
        assert!(tail.lines.front().unwrap().ends_with("line 090"));
        assert!(tail.lines.back().unwrap().ends_with("line 099"));
    }

    #[test]
    fn selects_today_files_in_chronological_order() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let day_start = now - Duration::from_secs(3600);
        let touch = |name: &str, modified: SystemTime| {
            let path = dir.path().join(name);
            std::fs::write(&path, "x").unwrap();
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        };
        touch("cc-switch_old.log", now - Duration::from_secs(7200));
        touch("cc-switch_b.log", now - Duration::from_secs(600));
        touch("cc-switch_a.log", now - Duration::from_secs(1200));
        touch(CURRENT_LOG_FILE, now - Duration::from_secs(1800));
        touch("other.log", now);

        let names: Vec<String> = todays_log_files(dir.path(), day_start)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["cc-switch_a.log", "cc-switch_b.log", CURRENT_LOG_FILE]
        );
    }

    #[test]
    fn issue_url_fits_as_many_latest_lines_as_budget_allows() {
        let env = test_env();
        let lines: Vec<String> = (0..300)
            .map(|i| format!("[2026-11-11][10:00:00][INFO][a] 日志内容 line {i:03}"))
            .collect();
        let (url, included) = build_issue_url(&env, "2026-11-11", &lines, 1000);

        assert!(url.len() <= MAX_ISSUE_URL_LEN, "len = {}", url.len());
        assert!(included > 0 && included < lines.len());
        // 再多放一行就会超出预算
        let one_more = issue_url_with(
            &env,
            "2026-11-11",
            &lines[lines.len() - included - 1..],
            1000,
        );
        assert!(one_more.len() > MAX_ISSUE_URL_LEN);

        let parsed = url::Url::parse(&url).unwrap();
        let params: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
        assert_eq!(params["template"], ISSUE_TEMPLATE);
        assert_eq!(params["version"], "9.9.9");
        assert_eq!(params["os"], "Windows");
        let context = &params["context"];
        assert!(context.contains(&format!("last {included} of 1000 lines from 2026-11-11")));
        assert!(context.contains("line 299"));
        assert!(!context.contains(&format!("line {:03}", 299 - included)));
    }

    #[test]
    fn small_day_log_is_included_in_full() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CURRENT_LOG_FILE),
            "[2026-11-10][08:00:00][INFO][a] old\n\
             [2026-11-11][08:00:00][WARN][a] new warning ```x```\n",
        )
        .unwrap();

        let report = build_feedback_report(dir.path(), &test_env());
        assert_eq!(report.date, "2026-11-11");
        assert_eq!((report.total_lines, report.included_lines), (1, 1));
        let context = context_of(&report.issue_url);
        assert!(context.contains("all 1 lines from 2026-11-11"));
        assert!(context.contains("new warning '''x'''"));
        assert!(!context.contains("] old"));
        // 只读取原日志，不创建任何额外文件
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn log_is_privacy_filtered_before_prefill() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CURRENT_LOG_FILE),
            "[2026-11-11][08:00:00][INFO][a] user alice.zhang@corp-mail.com logged in\n\
             [2026-11-11][08:00:01][WARN][a] request failed token=aB3xK9pLmN2qR7sT5vW1zYQwErTyUiOp\n\
             [2026-11-11][08:00:02][INFO][a] opened /Users/alice/Documents/a.txt\n",
        )
        .unwrap();

        let report = build_feedback_report(dir.path(), &test_env());
        assert_eq!((report.total_lines, report.included_lines), (3, 3));
        let context = context_of(&report.issue_url);
        assert!(!context.contains("alice.zhang@corp-mail.com"), "{context}");
        assert!(
            !context.contains("aB3xK9pLmN2qR7sT5vW1zYQwErTyUiOp"),
            "{context}"
        );
        assert!(context.contains("[EMAIL]"), "{context}");
        assert!(context.contains("[SECRET]"), "{context}");
        // 普通路径不受影响
        assert!(
            context.contains("/Users/alice/Documents/a.txt"),
            "{context}"
        );
        assert!(context.contains("- Privacy: "), "{context}");
    }

    #[test]
    fn day_without_entries_still_prefills_environment() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CURRENT_LOG_FILE),
            "[2026-11-10][08:00:00][INFO][a] old\n",
        )
        .unwrap();

        let report = build_feedback_report(dir.path(), &test_env());
        assert_eq!((report.total_lines, report.included_lines), (0, 0));
        let context = context_of(&report.issue_url);
        assert!(context.contains("- Version: v9.9.9"));
        assert!(context.contains("no entries recorded on 2026-11-11"));
    }
}
