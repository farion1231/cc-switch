//! 结构化 PII：邮箱、手机号、身份证、银行卡、IPv4
//!
//! Rust `regex` 与 Go RE2 一样不支持前后向断言，数字边界在匹配后手工校验。

use super::{Kind, Span};
use once_cell::sync::Lazy;
use regex::Regex;

static RE_EMAIL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap());
static RE_PHONE_CN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:\+?86[-\s]?)?1[3-9][0-9]{9}").unwrap());
static RE_ID_CARD: Lazy<Regex> = Lazy::new(|| Regex::new(r"[1-9][0-9]{16}[0-9Xx]").unwrap());
static RE_BANK_CARD: Lazy<Regex> = Lazy::new(|| Regex::new(r"[0-9]{13,19}").unwrap());
static RE_IPV4: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?:(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])\.){3}(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])",
    )
    .unwrap()
});

/// 远程命令前缀。user@host 出现在这些命令上下文里通常是 SSH 目标，不是邮箱。
const SSH_COMMANDS: &[&str] = &[
    "ssh ",
    "scp ",
    "rsync ",
    "sftp ",
    "ssh-copy-id ",
    "ssh-keygen ",
];

/// email 命中所在行（行首到命中起点）是否出现 ssh/scp/rsync 等命令前缀
fn is_in_ssh_command_context(text: &str, email_start: usize) -> bool {
    let line_start = text[..email_start].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..email_start];
    SSH_COMMANDS.iter().any(|cmd| line.contains(cmd))
}

/// 匹配两侧不是数字
fn digit_bounded(b: &[u8], start: usize, end: usize) -> bool {
    !(start > 0 && b[start - 1].is_ascii_digit() || end < b.len() && b[end].is_ascii_digit())
}

/// 匹配两侧不是数字或点
fn ip_bounded(b: &[u8], start: usize, end: usize) -> bool {
    let bad = |c: u8| c.is_ascii_digit() || c == b'.';
    !(start > 0 && bad(b[start - 1]) || end < b.len() && bad(b[end]))
}

/// Luhn 校验，过滤掉「长得像卡号的普通数字串」
fn luhn_valid(num: &str) -> bool {
    let mut sum = 0;
    let mut double = false;
    for c in num.bytes().rev() {
        let mut d = u32::from(c - b'0');
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum % 10 == 0
}

pub(super) fn detect(text: &str) -> Vec<Span> {
    let b = text.as_bytes();
    let mut spans = Vec::new();
    for m in RE_EMAIL.find_iter(text) {
        let (s, e) = (m.start(), m.end());
        // SSH 风格 URL：user@host:path —— 紧接 ":" + 非空白字符 → 视作 git@ URL
        if e + 1 < b.len() && b[e] == b':' && b[e + 1] != b' ' && b[e + 1] != b'\t' {
            continue;
        }
        if is_in_ssh_command_context(text, s) {
            continue;
        }
        spans.push(Span {
            start: s,
            end: e,
            kind: Kind::Email,
        });
    }
    for m in RE_PHONE_CN.find_iter(text) {
        if digit_bounded(b, m.start(), m.end()) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::Phone,
            });
        }
    }
    for m in RE_ID_CARD.find_iter(text) {
        if digit_bounded(b, m.start(), m.end()) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::IdCard,
            });
        }
    }
    for m in RE_IPV4.find_iter(text) {
        if ip_bounded(b, m.start(), m.end()) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::Ip,
            });
        }
    }
    for m in RE_BANK_CARD.find_iter(text) {
        if digit_bounded(b, m.start(), m.end()) && luhn_valid(m.as_str()) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::BankCard,
            });
        }
    }
    spans
}
