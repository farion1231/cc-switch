//! 结构化 PII：邮箱、手机号、身份证、银行卡、IPv4
//!
//! Rust `regex` 与 Go RE2 一样不支持前后向断言，数字边界在匹配后手工校验。

use super::secrets::{lookback_start, CONTEXT_LOOKBACK};
use super::{Kind, Span};
use once_cell::sync::Lazy;
use regex::Regex;
use std::net::Ipv4Addr;

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

/// 紧贴在数字前面、说明它是卡号的词。不含单独的 card：UI 代码里 `card.createdAt: 1700000000000` 太常见
static RE_CARD_CONTEXT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)卡号|银行卡|信用卡|储蓄卡|借记卡|(?:bank|credit|debit|payment)[\s_-]?card|\bcard[\s_-]?(?:no|num|number)\b",
    )
    .unwrap()
});

/// 常见卡组织的卡号前缀：(前 4 位下界, 前 4 位上界, 最短位数, 最长位数)。
/// 只收发卡量大的段位，其余卡号靠 RE_CARD_CONTEXT 兜底。
const CARD_BINS: &[(u32, u32, usize, usize)] = &[
    (4000, 4999, 16, 16), // Visa
    (2221, 2720, 16, 16), // Mastercard 2 系列
    (5100, 5599, 16, 16), // Mastercard
    (3400, 3499, 15, 15), // American Express
    (3700, 3799, 15, 15), // American Express
    (3528, 3589, 16, 19), // JCB
    (6011, 6011, 16, 19), // Discover
    (6013, 6013, 16, 19), // 中国银行
    (6200, 6299, 16, 19), // 银联
    (6440, 6599, 16, 19), // Discover
    (8100, 8199, 16, 19), // 银联
    (9555, 9559, 16, 19), // 国内银行早期自编卡号（招行 9555、工行 9558、农行 9559）
];

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

/// 回环 / 未指定 / 广播地址不指向具体主机，本地开发时随处可见，不算个人信息
fn is_non_identifying_ip(ip: &str) -> bool {
    ip.parse::<Ipv4Addr>()
        .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified() || ip.is_broadcast())
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

/// 前缀和位数落在已知卡组织范围内
fn has_card_bin(num: &str) -> bool {
    num[..4].parse::<u32>().is_ok_and(|prefix| {
        CARD_BINS.iter().any(|&(lo, hi, min, max)| {
            (lo..=hi).contains(&prefix) && (min..=max).contains(&num.len())
        })
    })
}

/// 数字前面紧贴「卡号 / card number」这类词
fn has_card_context(text: &str, start: usize) -> bool {
    RE_CARD_CONTEXT.is_match(&text[lookback_start(text, start, CONTEXT_LOOKBACK)..start])
}

/// 卡号判定。Luhn 约有 1/10 的随机数字串能通过，13 位毫秒时间戳就在其中，
/// 所以还要有卡组织前缀或卡号上下文。
fn is_bank_card(text: &str, start: usize, num: &str) -> bool {
    luhn_valid(num) && (has_card_bin(num) || has_card_context(text, start))
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
        if ip_bounded(b, m.start(), m.end()) && !is_non_identifying_ip(m.as_str()) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::Ip,
            });
        }
    }
    for m in RE_BANK_CARD.find_iter(text) {
        if digit_bounded(b, m.start(), m.end()) && is_bank_card(text, m.start(), m.as_str()) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::BankCard,
            });
        }
    }
    spans
}
