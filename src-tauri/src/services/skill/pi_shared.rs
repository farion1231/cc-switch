//! Pi 共享技能感知（方案：`.cache/plans/feat-pi-shared-skills.md`）。
//!
//! glob 语义依据 pi 源码 `core/package-manager.js` 的 `matchesAnyPattern`
//! （minimatch@10 默认选项、按段匹配算法的忠实移植，不支持花括号展开与
//! extglob，由 [`is_supported_pattern`] 上游过滤）。
//! 行为由 `testdata/glob-oracle.json` 锁定
//! （`scripts/glob-oracle/gen.mjs` 用与 pi 相同版本的 minimatch 生成）。

use regex::Regex;
use std::path::{Path, PathBuf};

/// minimatch@10（默认选项）glob 子集匹配。pattern 与 target 均为 posix 路径。
/// 不支持花括号展开与 extglob（调用方先经 [`is_supported_pattern`] 过滤）。
pub(crate) fn glob_match(pattern: &str, target: &str) -> bool {
    // minimatch 默认 nocomment：'#' 开头的 pattern 匹配不到任何东西
    if pattern.starts_with('#') {
        return false;
    }
    let file: Vec<&str> = target.split('/').collect();
    let segments: Vec<Seg> = pattern.split('/').map(parse_seg).collect();
    if segments.iter().any(|s| matches!(s, Seg::GlobStar)) {
        match_globstar(&file, &segments)
    } else {
        match_one(&file, &segments)
    }
}

/// 是否为支持的 glob 语法（花括号展开 `{…}`、extglob `+(…)`/`?(…)`/`*(…)`/`@(…)`/`!(…)` 不支持）。
pub(crate) fn is_supported_pattern(pattern: &str) -> bool {
    if pattern.contains('{') || pattern.contains('}') {
        return false;
    }
    // extglob：+ ( ? * @ ! 紧跟 '(' 才是 extglob；单独出现是普通字符
    pattern
        .chars()
        .zip(pattern.chars().skip(1))
        .all(|(a, b)| !(matches!(a, '+' | '?' | '*' | '@' | '!') && b == '('))
}

/// settings `skills` 数组的模式集合（前缀已剥离，均 posix 化）。
/// 语义依据 pi `applyPatterns` / `isEnabledByOverrides`。
#[derive(Default)]
pub(crate) struct PiPatternSet {
    /// 无前缀 glob 条目：白名单，仅作用于扫描根
    pub includes: Vec<String>,
    /// `!pattern`：glob 排除
    pub excludes: Vec<String>,
    /// `+path`：精确强制启用
    pub force_includes: Vec<String>,
    /// `-path`：精确强制排除（赢过 +）
    pub force_excludes: Vec<String>,
}

/// pi 的一个共享技能根目录
pub(crate) struct PiSharedRoot {
    /// 规范化后的绝对路径（canonicalize）
    pub path: PathBuf,
    /// 来源
    #[expect(dead_code)] // S5 判定 baseDir 时消费
    pub origin: PiSharedRootOrigin,
}

pub(crate) enum PiSharedRootOrigin {
    /// pi 全局自动发现的 `~/.agents/skills`（无条件存在）
    AgentsDir,
    /// settings.json `skills` 数组中的扫描根条目（保留原始字符串用于展示）
    SettingsEntry {
        #[expect(dead_code)] // S9 展示时消费
        raw: String,
    },
}

/// 一次解析出的 pi 技能来源全景（每次现算，不缓存）。
/// 解析失败与不支持语法均降级为 `approximate = true`，不阻断。
pub(crate) struct PiSkillSources {
    pub roots: Vec<PiSharedRoot>,
    pub patterns: PiPatternSet,
    pub warnings: Vec<String>,
    /// 任一不支持语法或 settings 解析失败 → 本次解析所有结果均近似
    pub approximate: bool,
    /// pi agent 目录（模式匹配与相对路径解析的基准）；None = 解析失败退化
    pub agent_dir: Option<PathBuf>,
}

/// 解析 pi 的全部共享技能根与模式（方案 §3.1）。
/// 固定追加 `~/.agents/skills`；settings 条目分类按 §1.1；
/// 任何失败路径均不报错，降级为近似结果 + warning。
#[expect(dead_code)] // S5 消费后移除
pub(crate) fn resolve_pi_skill_sources() -> PiSkillSources {
    let mut sources = PiSkillSources {
        roots: Vec::new(),
        patterns: PiPatternSet::default(),
        warnings: Vec::new(),
        approximate: false,
        agent_dir: None,
    };

    let agent_dir = crate::pi_config::get_pi_agent_dir().ok();
    sources.agent_dir = agent_dir.clone();

    // 1. ~/.agents/skills：pi 无条件自动发现（目录不存在也保留，S_exists 按技能判定）
    let home = crate::config::get_home_dir();
    let agents = home.join(".agents").join("skills");
    let agents = agents.canonicalize().unwrap_or(agents);
    sources.roots.push(PiSharedRoot {
        path: agents,
        origin: PiSharedRootOrigin::AgentsDir,
    });

    // 2. settings.json skills 数组
    match crate::pi_config::read_pi_settings_skills_array() {
        Ok(Some(entries)) => {
            for raw in entries {
                classify_entry(&raw, &agent_dir, &mut sources);
            }
        }
        Ok(None) => {}
        Err(err) => {
            sources.approximate = true;
            sources
                .warnings
                .push(format!("pi settings.json 解析失败：{err}"));
        }
    }

    sources
}

/// settings `skills` 条目分类（§1.1）：
/// package scheme 跳过；`!`/`+`/`-` 前缀与含通配符 → 模式；
/// 纯路径 → 扫描根（`~` 展开、相对 agent 目录、需存在且为目录）。
fn classify_entry(raw: &str, agent_dir: &Option<PathBuf>, sources: &mut PiSkillSources) {
    const SCHEMES: [&str; 7] = [
        "npm:", "git:", "github:", "http:", "https:", "ssh:", "builtin:",
    ];
    if SCHEMES.iter().any(|s| raw.starts_with(s)) {
        return; // Pi packages，out of scope
    }
    let stripped = match raw.as_bytes().first().copied() {
        Some(b'!') => {
            if let Some(w) = push_pattern(&mut sources.patterns.excludes, &raw[1..]) {
                sources.approximate = true;
                sources.warnings.push(w);
            }
            return;
        }
        Some(b'+') => {
            if let Some(w) = push_pattern(&mut sources.patterns.force_includes, &raw[1..]) {
                sources.approximate = true;
                sources.warnings.push(w);
            }
            return;
        }
        Some(b'-') => {
            if let Some(w) = push_pattern(&mut sources.patterns.force_excludes, &raw[1..]) {
                sources.approximate = true;
                sources.warnings.push(w);
            }
            return;
        }
        _ => raw,
    };
    if stripped.contains('*') || stripped.contains('?') {
        if let Some(w) = push_pattern(&mut sources.patterns.includes, stripped) {
            sources.approximate = true;
            sources.warnings.push(w);
        }
        return;
    }
    // 纯路径 → 扫描根
    let resolved = resolve_entry_path(stripped, agent_dir);
    if let Some(path) = resolved {
        push_root(
            &mut sources.roots,
            path,
            PiSharedRootOrigin::SettingsEntry {
                raw: raw.to_string(),
            },
        );
    }
}

/// 收集模式（posix 化）；不支持语法时返回 warning 文案
fn push_pattern(bucket: &mut Vec<String>, pattern: &str) -> Option<String> {
    let warning = (!is_supported_pattern(pattern))
        .then(|| format!("不支持的 glob 语法（近似处理）：{pattern:?}"));
    bucket.push(to_posix_str(pattern));
    warning
}

/// 条目路径解析：`~` 展开；相对路径相对 pi agent 目录（agent 目录不可用时跳过）。
fn resolve_entry_path(entry: &str, agent_dir: &Option<PathBuf>) -> Option<PathBuf> {
    let path = if let Some(rest) = entry.strip_prefix("~/") {
        crate::config::get_home_dir().join(rest)
    } else if entry == "~" {
        crate::config::get_home_dir()
    } else if entry.starts_with('/') {
        PathBuf::from(entry)
    } else {
        agent_dir.as_ref()?.join(entry)
    };
    // 仅目录成为扫描根（单文件技能/out of scope；不存在的路径跳过）
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

/// 追加扫描根（仅存在目录：canonicalize 后按路径去重，失败则跳过）
fn push_root(roots: &mut Vec<PiSharedRoot>, path: PathBuf, origin: PiSharedRootOrigin) {
    let Ok(canonical) = path.canonicalize() else {
        return;
    };
    if roots.iter().any(|r| r.path == canonical) {
        return;
    }
    roots.push(PiSharedRoot {
        path: canonical,
        origin,
    });
}

/// §1.2.B：自动发现目录（`~/.pi/agent/skills`、`~/.agents/skills`）的启用判定
/// （pi `isEnabledByOverrides`）：
/// 默认启用 → `!glob` 命中→禁用 → `+exact` 命中→启用 → `-exact` 命中→禁用。
/// 无前缀条目（include 白名单）不参与本判定。
#[expect(dead_code)] // S5 消费后移除
pub(crate) fn is_autodiscovered_enabled(md_path: &Path, base_dir: &Path, p: &PiPatternSet) -> bool {
    let mut enabled = true;
    if !p.excludes.is_empty() && matches_any_pattern(md_path, base_dir, &p.excludes) {
        enabled = false;
    }
    if !p.force_includes.is_empty() && matches_any_exact(md_path, base_dir, &p.force_includes) {
        enabled = true;
    }
    if !p.force_excludes.is_empty() && matches_any_exact(md_path, base_dir, &p.force_excludes) {
        enabled = false;
    }
    enabled
}

/// §1.2.A：settings 扫描根的启用判定（pi `applyPatterns`，作用于该根收集的全部文件）：
/// include 白名单（存在时仅保留匹配）→ `!glob` 排除 → `+exact` 从全量加回 → `-exact` 终局排除。
#[expect(dead_code)] // S5 消费后移除
pub(crate) fn is_scanned_enabled(md_path: &Path, base_dir: &Path, p: &PiPatternSet) -> bool {
    let mut result = p.includes.is_empty() || matches_any_pattern(md_path, base_dir, &p.includes);
    if result && !p.excludes.is_empty() && matches_any_pattern(md_path, base_dir, &p.excludes) {
        result = false;
    }
    if !result
        && !p.force_includes.is_empty()
        && matches_any_exact(md_path, base_dir, &p.force_includes)
    {
        result = true;
    }
    if result
        && !p.force_excludes.is_empty()
        && matches_any_exact(md_path, base_dir, &p.force_excludes)
    {
        result = false;
    }
    result
}

/// pi `matchesAnyPattern`：glob 匹配，SKILL.md 文件时 6 种目标逐一尝试：
/// rel(P,B)、basename(P)、P posix、rel(D,B)、basename(D)（即技能目录名）、D posix。
fn matches_any_pattern(md_path: &Path, base_dir: &Path, patterns: &[String]) -> bool {
    let is_skill_file = md_path.file_name().is_some_and(|n| n == "SKILL.md");
    let path_posix = to_posix(md_path);
    let rel = relative_posix(md_path, base_dir);
    let name = md_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent_rel = md_path
        .parent()
        .map(|d| relative_posix(d, base_dir))
        .unwrap_or_default();
    let parent_name = md_path
        .parent()
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent_posix = md_path.parent().map(to_posix).unwrap_or_default();
    patterns.iter().any(|pattern| {
        let pat = to_posix_str(pattern);
        glob_match(&pat, &rel)
            || glob_match(&pat, &name)
            || glob_match(&pat, &path_posix)
            || (is_skill_file
                && (glob_match(&pat, &parent_rel)
                    || glob_match(&pat, &parent_name)
                    || glob_match(&pat, &parent_posix)))
    })
}

/// pi `matchesAnyExactPattern`：精确匹配（去开头 `./` 后全等），4 种目标：
/// rel(P,B)、P posix、rel(D,B)、D posix（不含 basename 与父目录名）。
fn matches_any_exact(md_path: &Path, base_dir: &Path, patterns: &[String]) -> bool {
    let is_skill_file = md_path.file_name().is_some_and(|n| n == "SKILL.md");
    let path_posix = to_posix(md_path);
    let rel = relative_posix(md_path, base_dir);
    let parent_rel = md_path
        .parent()
        .map(|d| relative_posix(d, base_dir))
        .unwrap_or_default();
    let parent_posix = md_path.parent().map(to_posix).unwrap_or_default();
    patterns.iter().any(|pattern| {
        let normalized = pattern.strip_prefix("./").unwrap_or(pattern);
        normalized == rel
            || normalized == path_posix
            || (is_skill_file && (normalized == parent_rel || normalized == parent_posix))
    })
}

/// 路径 posix 化（对齐 pi 的 toPosixPath，Windows 兼容）
fn to_posix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn to_posix_str(s: &str) -> String {
    s.replace('\\', "/")
}

/// 相对路径（posix）：base_dir 不含 path 时向上攀升（对齐 node path.relative）
fn relative_posix(path: &Path, base_dir: &Path) -> String {
    let p: Vec<_> = path.components().collect();
    let b: Vec<_> = base_dir.components().collect();
    let mut common = 0;
    while common < p.len() && common < b.len() && p[common] == b[common] {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..b.len() {
        parts.push("..".to_string());
    }
    for c in &p[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

/// minimatch 段匹配：pattern 按段编译，file 按段对齐
#[derive(Clone)]
enum Seg {
    /// 整段 `**`
    GlobStar,
    /// 无魔法字符：字符串全等
    Literal(String),
    /// 含魔法字符：编译为段正则 + 点规则元数据（rust regex 无前瞻，守卫在正则外实现）
    Re {
        re: Regex,
        /// 段首魔法字符（`*` `?` 或非单点类）不允许匹配点开头段（dot:false）
        guard_dot: bool,
        /// 恰为单点类 `[.]`（minimatch 优化为字面量 `\.`，无守卫，可匹配 `.`）
        explicit_dot: bool,
    },
}

fn parse_seg(seg: &str) -> Seg {
    if seg == "**" {
        Seg::GlobStar
    } else if !seg.contains(['*', '?', '[']) {
        Seg::Literal(seg.to_string())
    } else {
        let (re, guard_dot, explicit_dot) = compile_seg(seg);
        Seg::Re {
            re,
            guard_dot,
            explicit_dot,
        }
    }
}

/// 段正则编译（对应 minimatch AST → regex，dot:false）：
/// - 纯星段（`*`+）为 `[^/]+`（minimatch 快路径要求非空）；
/// - 其余连续星折叠为 `[^/]*`；`?` 为 `[^/]`；字符类 `!` 取反；
/// - `\` 转义下一字符；未闭合的 `[` 按普通字符处理；
/// - 点守卫不在正则内（rust regex 无前瞻），返回 guard_dot/explicit_dot
///   元数据由 [`seg_hit`] 应用。
fn compile_seg(seg: &str) -> (Regex, bool, bool) {
    let chars: Vec<char> = seg.chars().collect();
    let mut re = String::new();
    let mut guard_dot = false;
    let mut explicit_dot = false;
    match chars.first().copied() {
        // 段首 * / ? 不允许匹配点开头段（dot:false）
        Some('*') | Some('?') => guard_dot = true,
        // 单点类 `[.]`/`[\.]` 优化为字面量且无守卫；其余类有守卫
        Some('[') => {
            if seg == "[.]" || seg == "[\\.]" {
                explicit_dot = true;
            } else {
                guard_dot = true;
            }
        }
        _ => {}
    }
    let all_stars = !chars.is_empty() && chars.iter().all(|&c| c == '*');
    let star_re = if all_stars { "[^/]+" } else { "[^/]*" };
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                if i + 1 < chars.len() {
                    re.push_str(&regex_escape(chars[i + 1]));
                    i += 2;
                } else {
                    re.push_str("\\\\");
                    i += 1;
                }
            }
            '*' => {
                while i < chars.len() && chars[i] == '*' {
                    i += 1;
                }
                re.push_str(star_re);
            }
            '?' => {
                re.push_str("[^/]");
                i += 1;
            }
            '[' => {
                if let Some((class, end)) = parse_class(&chars, i) {
                    re.push_str(&class);
                    i = end;
                } else {
                    re.push_str("\\[");
                    i += 1;
                }
            }
            c => {
                re.push_str(&regex_escape(c));
                i += 1;
            }
        }
    }
    (
        Regex::new(&format!("^{re}$")).expect("段正则必然合法"),
        guard_dot,
        explicit_dot,
    )
}

/// 解析 `[...]` 字符类 → 正则类。`[!` 取反；`]` 紧跟 `[` 时为字面成员；
/// 支持范围 `a-z`。返回 (正则片段, 结束下标)；未闭合返回 None。
fn parse_class(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut body = String::new();
    let mut i = start + 1;
    if i < chars.len() && chars[i] == '!' {
        body.push('^');
        i += 1;
    }
    // POSIX：']' 作为首个成员是字面量
    if i < chars.len() && chars[i] == ']' {
        body.push_str("\\]");
        i += 1;
    }
    while i < chars.len() && chars[i] != ']' {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            body.push('\\');
            body.push(chars[i + 1]);
            i += 2;
            continue;
        }
        // 保留范围连字符；正则元字符转义
        body.push_str(&if c == '-' {
            "-".to_string()
        } else {
            regex_escape(c)
        });
        i += 1;
    }
    if i >= chars.len() {
        return None; // 未闭合
    }
    Some((format!("[{body}]"), i + 1))
}

fn regex_escape(c: char) -> String {
    if c.is_ascii() && ".^$*+?()[]{}|\\".contains(c) {
        format!("\\{c}")
    } else {
        c.to_string()
    }
}

/// minimatch `#matchOne`（partial=false）：逐段匹配，长度相等；
/// pattern 耗尽时仅允许 file 恰剩一个尾部空段（`a/*` 匹配 `a/b/`）。
fn match_one(file: &[&str], pattern: &[Seg]) -> bool {
    let mut fi = 0;
    let mut pi = 0;
    while fi < file.len() && pi < pattern.len() {
        let hit = match &pattern[pi] {
            Seg::Literal(s) => file[fi] == s,
            Seg::Re {
                re,
                guard_dot,
                explicit_dot,
            } => seg_hit(re, *guard_dot, *explicit_dot, file[fi]),
            Seg::GlobStar => false, // 调用方已分流，不应出现
        };
        if !hit {
            return false;
        }
        fi += 1;
        pi += 1;
    }
    if fi == file.len() && pi == pattern.len() {
        return true;
    }
    if fi == file.len() {
        return false; // file 耗尽而 pattern 有余（partial=false）
    }
    // pattern 耗尽而 file 有余：仅允许恰剩一个尾部空段
    fi == file.len() - 1 && file[fi].is_empty()
}

/// minimatch `#matchGlobstar`（partial=false）：head 段必须匹配开头，
/// tail 段必须匹配结尾，中间 body 段落可任意位置按序出现；
/// 无 body 时（`a/**`）要求 head 之后至少一个段（sawSome 规则）。
fn match_globstar(file: &[&str], pattern: &[Seg]) -> bool {
    let firstgs = pattern
        .iter()
        .position(|s| matches!(s, Seg::GlobStar))
        .expect("调用方保证存在 GlobStar");
    let lastgs = pattern
        .iter()
        .rposition(|s| matches!(s, Seg::GlobStar))
        .expect("调用方保证存在 GlobStar");
    let head = &pattern[..firstgs];
    // JS slice 容忍 start>end（单 `**` 时 firstgs+1 > lastgs），Rust 需显式处理
    let body: &[Seg] = if firstgs < lastgs {
        &pattern[firstgs + 1..lastgs]
    } else {
        &[]
    };
    let tail = &pattern[lastgs + 1..];

    let mut file_index = 0usize;
    if !head.is_empty() {
        if file.len() < head.len() || !match_one(&file[..head.len()], head) {
            return false;
        }
        file_index = head.len();
    }

    let mut file_tail_match = 0usize;
    if !tail.is_empty() {
        if tail.len() + file_index > file.len() {
            return false;
        }
        let mut tail_start = file.len() - tail.len();
        if match_one(&file[tail_start..tail_start + tail.len()], tail) {
            file_tail_match = tail.len();
        } else {
            // 尾部空段让步（`a/**/*` 匹配 `a/b/`）
            if file.last() != Some(&"") || file_index + tail.len() == file.len() {
                return false;
            }
            tail_start -= 1;
            if !match_one(&file[tail_start..tail_start + tail.len()], tail) {
                return false;
            }
            file_tail_match = tail.len() + 1;
        }
    }

    if body.is_empty() {
        let mut saw_some = file_tail_match > 0;
        for f in &file[file_index..file.len() - file_tail_match] {
            saw_some = true;
            if bad_dot(f) {
                return false;
            }
        }
        return saw_some;
    }

    // body 按 GlobStar 切成段落（与 minimatch 相同：首个段落下标 0，
    // 每个 GlobStar 开新段落），各段落有最靠右可匹配位置上界
    let mut body_segments: Vec<(Vec<Seg>, usize)> = vec![(Vec::new(), 0)];
    let mut non_gs_parts = 0usize;
    let mut non_gs_sums = vec![0usize];
    for seg in body {
        if matches!(seg, Seg::GlobStar) {
            non_gs_sums.push(non_gs_parts);
            body_segments.push((Vec::new(), 0));
        } else {
            body_segments
                .last_mut()
                .expect("首个段落已存在")
                .0
                .push(seg.clone());
            non_gs_parts += 1;
        }
    }
    let file_length = file.len() - file_tail_match;
    let mut i = body_segments.len();
    for seg in &mut body_segments {
        i -= 1;
        seg.1 = file_length.saturating_sub(non_gs_sums[i] + seg.0.len());
    }
    matches!(
        match_body_sections(file, &body_segments, file_index, 0, 0, file_tail_match > 0),
        Some(true)
    )
}

/// minimatch `#matchGlobStarBodySections`：
/// Some(true)=命中，Some(false)=不命中，None=位置耗尽（等价不命中）。
fn match_body_sections(
    file: &[&str],
    body_segments: &[(Vec<Seg>, usize)],
    file_index: usize,
    body_index: usize,
    depth: usize,
    mut saw_tail: bool,
) -> Option<bool> {
    let Some((body, after)) = body_segments.get(body_index) else {
        // 末段之后：剩余段不得有点开头，且需已见到 tail 或仍有剩余段
        for f in &file[file_index..] {
            saw_tail = true;
            if bad_dot(f) {
                return Some(false);
            }
        }
        return Some(saw_tail);
    };
    let mut fi = file_index;
    while fi <= *after {
        if fi + body.len() <= file.len()
            && match_one(&file[fi..fi + body.len()], body)
            && depth < MAX_GLOBSTAR_RECURSION
        {
            if let Some(sub) = match_body_sections(
                file,
                body_segments,
                fi + body.len(),
                body_index + 1,
                depth + 1,
                saw_tail,
            )
            .filter(|r| *r)
            {
                return Some(sub);
            }
        }
        if fi >= file.len() || bad_dot(file[fi]) {
            return Some(false);
        }
        fi += 1;
    }
    None
}

/// 单段命中判定（含 minimatch dot:false 规则）：
/// - 正则先匹配；
/// - `.`/`..` 段仅允许字面量等值（在 Literal 分支）或单点类 `[.]` 匹配 `.`；
/// - 点开头段不被段首魔法字符（guard_dot）匹配。
fn seg_hit(re: &Regex, guard_dot: bool, explicit_dot: bool, f: &str) -> bool {
    if !re.is_match(f) {
        return false;
    }
    if f == "." || f == ".." {
        return explicit_dot && f == ".";
    }
    if f.starts_with('.') {
        return !guard_dot;
    }
    true
}

fn bad_dot(seg: &str) -> bool {
    seg == "." || seg == ".." || seg.starts_with('.')
}

/// minimatch maxGlobstarRecursion 默认值
const MAX_GLOBSTAR_RECURSION: usize = 200;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct OracleFixture {
        #[serde(rename = "minimatchVersion")]
        version: String,
        cases: Vec<OracleCase>,
        unsupported: Vec<String>,
    }

    #[derive(Deserialize)]
    struct OracleCase {
        pattern: String,
        target: String,
        matches: bool,
    }

    #[test]
    fn glob_match_follows_minimatch_oracle() {
        let fixture: OracleFixture =
            serde_json::from_str(include_str!("testdata/glob-oracle.json"))
                .expect("解析 oracle fixture");
        assert_eq!(fixture.version, "10.2.6");
        let mut checked = 0;
        for case in &fixture.cases {
            assert_eq!(
                glob_match(&case.pattern, &case.target),
                case.matches,
                "pattern={:?} target={:?}",
                case.pattern,
                case.target
            );
            checked += 1;
        }
        assert!(checked > 800, "fixture 用例数异常: {checked}");
    }

    #[test]
    fn unsupported_patterns_are_detected() {
        let fixture: OracleFixture =
            serde_json::from_str(include_str!("testdata/glob-oracle.json"))
                .expect("解析 oracle fixture");
        for pattern in &fixture.unsupported {
            assert!(!is_supported_pattern(pattern), "应判为不支持: {pattern:?}");
        }
        for pattern in ["*", "**/x", "[a-z]?.md", "a\\b", "a+b", "plain/path"] {
            assert!(is_supported_pattern(pattern), "应判为支持: {pattern:?}");
        }
    }

    // ===== S3：模式引擎两类语义（依 pi applyPatterns / isEnabledByOverrides）=====

    use super::{is_autodiscovered_enabled, is_scanned_enabled, PiPatternSet};
    use std::path::PathBuf;

    fn md(base: &str, skill: &str) -> PathBuf {
        PathBuf::from(base).join(skill).join("SKILL.md")
    }

    fn patterns(
        includes: &[&str],
        excludes: &[&str],
        force_includes: &[&str],
        force_excludes: &[&str],
    ) -> PiPatternSet {
        PiPatternSet {
            includes: includes.iter().map(|s| s.to_string()).collect(),
            excludes: excludes.iter().map(|s| s.to_string()).collect(),
            force_includes: force_includes.iter().map(|s| s.to_string()).collect(),
            force_excludes: force_excludes.iter().map(|s| s.to_string()).collect(),
        }
    }

    const AGENT: &str = "/home/u/.pi/agent";
    const AGENTS: &str = "/home/u/.agents";

    #[test]
    fn include_whitelist_only_affects_scanned_roots() {
        let p = patterns(&["pdf*"], &[], &[], &[]);
        // 扫描根：非白名单命中 → 禁用
        assert!(!is_scanned_enabled(
            &md("/home/u/.claude/skills", "doc"),
            Path::new(AGENT),
            &p
        ));
        assert!(is_scanned_enabled(
            &md("/home/u/.claude/skills", "pdf-tools"),
            Path::new(AGENT),
            &p
        ));
        // 自动发现目录：include 白名单不参与 → 默认启用
        assert!(is_autodiscovered_enabled(
            &md("/home/u/.pi/agent/skills", "doc"),
            Path::new(AGENT),
            &p
        ));
    }

    #[test]
    fn force_exclude_wins_over_force_include() {
        // - 赢 +：同时命中时禁用
        let p = patterns(&[], &[], &["skills/pdf"], &["skills/pdf"]);
        assert!(!is_autodiscovered_enabled(
            &md("/home/u/.pi/agent/skills", "pdf"),
            Path::new(AGENT),
            &p
        ));
        // 仅 +：从 ！ 排除中加回
        let p = patterns(&[], &["pdf"], &["skills/pdf"], &[]);
        assert!(is_autodiscovered_enabled(
            &md("/home/u/.pi/agent/skills", "pdf"),
            Path::new(AGENT),
            &p
        ));
        // 仅 ！：按父目录名 glob 命中 → 禁用
        let p = patterns(&[], &["pdf"], &[], &[]);
        assert!(!is_autodiscovered_enabled(
            &md("/home/u/.pi/agent/skills", "pdf"),
            Path::new(AGENT),
            &p
        ));
    }

    #[test]
    fn exact_match_ignores_basename_and_parent_name() {
        // +pdf（父目录名）不是精确匹配目标 → 无法加回
        let p = patterns(&[], &["pdf"], &["pdf"], &[]);
        assert!(!is_autodiscovered_enabled(
            &md("/home/u/.pi/agent/skills", "pdf"),
            Path::new(AGENT),
            &p
        ));
        // +skills/pdf（父目录相对路径）是精确目标 → 加回
        let p = patterns(&[], &["pdf"], &["skills/pdf"], &[]);
        assert!(is_autodiscovered_enabled(
            &md("/home/u/.pi/agent/skills", "pdf"),
            Path::new(AGENT),
            &p
        ));
    }

    #[test]
    fn base_dir_semantics() {
        // B=agent 与 B=~/.agents 的相对结构同为 skills/<dir>，
        // 相对路径模式（!skills/pdf）在两处均命中（父目录相对路径目标）
        let p = patterns(&[], &["skills/pdf"], &[], &[]);
        assert!(!is_autodiscovered_enabled(
            &md("/home/u/.pi/agent/skills", "pdf"),
            Path::new(AGENT),
            &p
        ));
        assert!(!is_autodiscovered_enabled(
            &md("/home/u/.agents/skills", "pdf"),
            Path::new(AGENTS),
            &p
        ));
        // 绝对路径模式仅命中对应来源（全路径目标不同）
        let p = patterns(&[], &["/home/u/.pi/agent/skills/pdf/SKILL.md"], &[], &[]);
        assert!(!is_autodiscovered_enabled(
            &md("/home/u/.pi/agent/skills", "pdf"),
            Path::new(AGENT),
            &p
        ));
        assert!(is_autodiscovered_enabled(
            &md("/home/u/.agents/skills", "pdf"),
            Path::new(AGENTS),
            &p
        ));
        // 扫描根的 B=agentDir：非 agent 目录下的技能相对路径会向上攀升
        let p = patterns(&[], &["../../.claude/skills/pdf"], &[], &[]);
        assert!(!is_scanned_enabled(
            &md("/home/u/.claude/skills", "pdf"),
            Path::new(AGENT),
            &p
        ));
    }

    #[test]
    fn scanned_force_include_adds_back_beyond_whitelist() {
        // + 从全量加回：即使 include 白名单未命中
        let p = patterns(&["pdf*"], &[], &["skills/doc"], &[]);
        assert!(is_scanned_enabled(
            &md("/home/u/.pi/agent/skills", "doc"),
            Path::new(AGENT),
            &p
        ));
    }

    // ===== S4：共享根解析 =====

    use super::{resolve_pi_skill_sources, PiSharedRootOrigin};
    use serial_test::serial;
    use std::fs;

    use crate::services::skill::tests::TestHomeGuard;

    fn write_dir(path: &std::path::Path) -> std::path::PathBuf {
        fs::create_dir_all(path).expect("mkdir");
        path.canonicalize().expect("canonicalize")
    }

    #[test]
    #[serial]
    fn resolve_classifies_entries_and_always_includes_agents_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = crate::services::skill::tests::TestHomeGuard::set(temp.path());
        let _agent = crate::pi_config::test_support::TestAgentDir::new();
        let agent_dir = crate::pi_config::get_pi_agent_dir().expect("agent dir");

        let claude = write_dir(&temp.path().join(".claude").join("skills"));
        let inside = write_dir(&agent_dir.join("my-skills"));
        let agents = temp.path().join(".agents").join("skills");
        fs::create_dir_all(&agents).expect("mkdir agents");
        let agents = agents.canonicalize().expect("canonicalize agents");
        // 不存在的路径 + 单文件
        fs::write(temp.path().join("loose.md"), "x").expect("write md");

        _agent.write_settings(&format!(
            r#"{{
              "skills": [
                "~/.claude/skills",
                "my-skills",
                "/nonexistent/path",
                "~/loose.md",
                "npm:@scope/pkg",
                "!pdf",
                "+skills/pdf",
                "-skills/doc",
                "foo*"
              ]
            }}"#
        ));

        let sources = resolve_pi_skill_sources();

        // 根：~/.agents/skills（AgentsDir）+ ~/.claude/skills + agent/my-skills；
        // 不存在路径、.md 文件、npm scheme 不产生根
        let root_paths: Vec<&PathBuf> = sources.roots.iter().map(|r| &r.path).collect();
        assert_eq!(root_paths.len(), 3, "roots: {root_paths:?}");
        assert!(root_paths.contains(&&agents));
        assert!(root_paths.contains(&&claude));
        assert!(root_paths.contains(&&inside));
        assert!(matches!(
            sources.roots[0].origin,
            PiSharedRootOrigin::AgentsDir
        ));

        // 模式分类
        assert_eq!(sources.patterns.excludes, vec!["pdf"]);
        assert_eq!(sources.patterns.force_includes, vec!["skills/pdf"]);
        assert_eq!(sources.patterns.force_excludes, vec!["skills/doc"]);
        assert_eq!(sources.patterns.includes, vec!["foo*"]);
        assert!(!sources.approximate);
        assert!(sources.agent_dir.is_some());
    }

    #[test]
    #[serial]
    fn broken_settings_degrades_to_approximate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = crate::services::skill::tests::TestHomeGuard::set(temp.path());
        let _agent = crate::pi_config::test_support::TestAgentDir::new();
        _agent.write_settings("{ broken");

        let sources = resolve_pi_skill_sources();
        assert!(sources.approximate);
        assert!(!sources.warnings.is_empty());
        // 降级仍有 ~/.agents/skills 根与空模式
        assert_eq!(sources.roots.len(), 1);
        assert!(sources.patterns.excludes.is_empty());
    }

    #[test]
    #[serial]
    fn unsupported_syntax_marks_global_approximate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = crate::services::skill::tests::TestHomeGuard::set(temp.path());
        let _agent = crate::pi_config::test_support::TestAgentDir::new();
        _agent.write_settings(r#"{ "skills": ["!{a,b}"] }"#);

        let sources = resolve_pi_skill_sources();
        assert!(sources.approximate);
        assert!(!sources.warnings.is_empty());
        // 模式仍被收集（供展示），但结果全部近似
        assert_eq!(sources.patterns.excludes, vec!["{a,b}"]);
    }
}
