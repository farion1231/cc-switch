#![cfg(test)]
//! 前置工程 A:Provider 写面认证测试套件 v5(测试先行)
//!
//! 本文件是认证契约的可执行字面,固化 R1–R4 盲审与三轮对抗审查揭示的全部
//! 写面故障场景。规则:
//! - 全绿是进入前置 A 盲审的前置条件,但不是充分条件;
//! - 对写面(provider_write.rs)新增任何函数、对 infra 三文件的任何改动、
//!   对本文件清单的任何调整,均须先经裁决。
//!
//! ## 已裁决的语义决定
//! 1. `created_at` 不可变:update 不得改写创建时间;`ProviderRowUpdate`
//!    必须不含 `created_at` 字段。create/restore 所需创建时间由各自入参单独
//!    携带(restore 已由裁决方加 `created_at` 参数;create 拆分是实现方职责)。
//!    `tests/fixtures/pi/provider-write-api-v1.json` 与
//!    `architecture_tests.rs` 的 snapshot 生成器随 DTO 拆分同步更新(生成器
//!    必须收录新建的 create/restore 专属类型)——这是实现方义务;拆分落地前
//!    旧 fixture 保持一致属预期。
//! 2. 结构化冲突:重复 create(含并发输家)必须返回 `AppError::Conflict`。
//!    注:`AppError` 的 `Serialize` 目前把错误序列化为字符串,IPC 层的结构化
//!    discriminant 是后续裁决项(P1),不在本 PR 强制。
//! 3. reconcile 显式前置期望(T9):脚手架已落地(`ReconcilePrecondition`、
//!    `provider_row_fingerprint`(规范化排序哈希,不含 endpoint)、
//!    `reconcile_provider_record_with_precondition`,故意保留旧语义使 T9 红)。
//!    实现方必须以**单事务原语**实现:ExpectAbsent → `create_provider`
//!    (冲突 → Conflict);ExpectPresent → 新 DAO 原语
//!    `update_provider_if_content_fingerprint`(单事务内读-比-写,过期 →
//!    Conflict)。reconcile 函数体内禁止内联 aggregate 读取后再分支
//!    (`certify_reconcile_uses_single_transaction_primitives` 机械强制)。
//!    **盲审重点核查项**:`update_provider_if_content_fingerprint` 内部必须
//!    在单次连接锁/单事务内完成读-比-写(本仓库为单连接 Mutex,持锁即全局
//!    串行);静态测试只能约束委托关系,原语内部"读后释放锁再写"的变体由
//!    组件盲审逐行核查——reviewer 材料必须包含本条。
//!    完成后迁移全部调用方并删除旧 `reconcile_provider_record`,由裁决方将
//!    旧符号加入禁止清单。
//!
//! ## 扫描器 authority 表(精确相对路径 × DML 种类 × 列集合)
//! - `database/dao/provider_write.rs`:全部 provider DML 允许(写面本体);
//! - `database/dao/providers.rs`:仅 `UPDATE providers`(列 ⊆ {is_current})
//!   与 `DELETE FROM providers`;
//! - `database/dao/failover.rs`:仅 `UPDATE providers`(列 ⊆ {in_failover_queue});
//! - infra 三文件:Deferred to 前置工程 B,由 SHA-256 基线冻结兜底;
//! - 测试专属文件必须自带文件级 `#![cfg(test)]`(注册元测试机械强制;借用
//!   他处注册的伪测试名生产文件在此失败),扫描器凭该属性天然跳过其内容;
//! - cfg 判定按布尔语义:仅当谓词蕴含 test 才跳过;`not(test)`、
//!   `any(test, unix)` 一律扫描;
//! - 宏 token 纳入扫描:宏内字符串经 `syn::LitStr::value()` 解码(覆盖
//!   `\xNN`/`\u{}`);纯字符串宏(`concat!`)拼接整体参与分类;含非字面量
//!   token 且出现 provider DML 锚点的宏(`format!`、`stringify!` 构造)
//!   一律 fail-closed 记为违规;`include!` 全生产源禁止,`include_str!`
//!   token 含 `.sql` 时禁止;
//! - 解析 fail-closed:SET 子句引号/括号不闭合或列集无法确定时产出
//!   `!unparseable` 哨兵列,任何 authority 不放行;
//! - 已知残余风险(接受,由盲审与前置 B 兜底,须向 reviewer 声明):
//!   完全运行时构造、无任何可识别字面锚点的动态 SQL;trigger/view 间接写;
//!   SQLite Backup API 整库复制;`r#"..."#` 多井号原始字符串宏字面量;
//!   `#[path]`/非 `.rs` 重定向包含;非 `.sql` 扩展名文件装载 SQL 文本;
//!   宏展开生成的 impl(inventory 已禁 item 级宏与 out-of-line 子模块,
//!   属性宏路径由盲审兜底);T10 的 identifier 探测只证明"标识符存在",
//!   真实调用行为由盲审核对。
//!   本清单为对抗加固的**收口边界**:静态扫描是护栏,组件盲审才是认证;
//!   清单外的新绕过按盲审 finding 处理,不再无限扩充扫描器。
//!

use crate::database::dao::provider_write::{
    self, NewEndpoint, NewProviderAggregate, ProviderKey, ProviderRowUpdate, RenameProvider,
};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::{ProviderMeta, ProviderMutationInput};
use crate::services::provider::{
    provider_row_fingerprint, reconcile_provider_record_with_precondition, ReconcilePrecondition,
};
use crate::settings::CustomEndpoint;
use regex::Regex;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use syn::visit::{self, Visit};
use syn::{Attribute, ExprLit, ImplItem, Item, Lit, Meta};

// ---------------------------------------------------------------------------
// 测试基建
// ---------------------------------------------------------------------------

fn db() -> Database {
    Database::memory().expect("memory db")
}

fn base_input(id: &str, name: &str) -> ProviderMutationInput {
    ProviderMutationInput {
        id: id.to_string(),
        name: name.to_string(),
        settings_config: json!({"env": {"KEY": "v"}}),
        website_url: None,
        category: None,
        created_at: Some(1_700_000_000),
        sort_index: None,
        notes: None,
        meta: None,
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

fn with_endpoints(
    mut input: ProviderMutationInput,
    endpoints: &[(&str, Option<i64>, Option<i64>)],
) -> ProviderMutationInput {
    let mut map = HashMap::new();
    for (url, added_at, last_used) in endpoints {
        map.insert(
            url.to_string(),
            CustomEndpoint {
                url: url.to_string(),
                added_at: *added_at,
                last_used: *last_used,
            },
        );
    }
    let mut meta = input.meta.take().unwrap_or_default();
    meta.custom_endpoints = map;
    input.meta = Some(meta);
    input
}

type RowSnapshot = (
    String,         // name
    String,         // settings_config
    Option<String>, // website_url
    Option<String>, // category
    Option<i64>,    // created_at
    Option<i64>,    // sort_index
    Option<String>, // notes
    Option<String>, // icon
    Option<String>, // icon_color
    String,         // meta
    i64,            // is_current
    i64,            // in_failover_queue
);

type EndpointRows = Vec<(String, Option<i64>, Option<i64>)>;

/// 逐列快照,用于"零副作用"断言。绕过 hydration 直接读库,以免 hydration
/// 自身的有损转换掩盖破坏;查询错误必须炸出来,不得伪装成"不存在"。
fn snapshot(database: &Database, app_type: &str, id: &str) -> (Option<RowSnapshot>, EndpointRows) {
    use rusqlite::OptionalExtension;
    let conn = database.conn.lock().expect("lock certification database");
    let row = conn
        .query_row(
            "SELECT name, settings_config, website_url, category, created_at,
                    sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
               FROM providers WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![id, app_type],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                ))
            },
        )
        .optional()
        .expect("snapshot row query must not error");
    let mut stmt = conn
        .prepare(
            "SELECT url, added_at, last_used FROM provider_endpoints
              WHERE provider_id = ?1 AND app_type = ?2 ORDER BY url",
        )
        .expect("prepare endpoint snapshot");
    let endpoints = stmt
        .query_map(rusqlite::params![id, app_type], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .expect("query endpoints")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect endpoints");
    (row, endpoints)
}

const ENDPOINT_REJECT_MESSAGE: &str = "certification injected endpoint failure";

fn install_endpoint_reject_trigger(database: &Database) {
    let conn = database.conn.lock().expect("lock certification database");
    conn.execute_batch(
        "CREATE TRIGGER certification_reject_endpoint_insert
         BEFORE INSERT ON provider_endpoints
         BEGIN SELECT RAISE(ABORT, 'certification injected endpoint failure'); END;",
    )
    .expect("install endpoint reject trigger");
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn relative_source_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .expect("source file under root")
        .to_string_lossy()
        .replace('\\', "/")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// cfg 布尔语义:仅当谓词蕴含 test 才视为 test-only
// ---------------------------------------------------------------------------

fn split_top_level(args: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth: i32 = 0;
    let mut current = String::new();
    for c in args.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                parts.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn strip_call<'a>(expr: &'a str, name: &str) -> Option<&'a str> {
    let rest = expr.strip_prefix(name)?.trim_start();
    let rest = rest.strip_prefix('(')?;
    rest.strip_suffix(')')
}

/// `test` → true;`all(..)` 任一分支蕴含 test → true;`any(..)` 需全部分支
/// 蕴含 test;`not(..)` 与其他谓词一律 false(保守:继续扫描)。
fn cfg_expr_requires_test(expr: &str) -> bool {
    let expr = expr.trim();
    if expr == "test" {
        return true;
    }
    if let Some(args) = strip_call(expr, "all") {
        return split_top_level(args)
            .iter()
            .any(|part| cfg_expr_requires_test(part));
    }
    if let Some(args) = strip_call(expr, "any") {
        let parts = split_top_level(args);
        return !parts.is_empty() && parts.iter().all(|part| cfg_expr_requires_test(part));
    }
    false
}

fn attrs_mark_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && matches!(
                &attribute.meta,
                Meta::List(list) if cfg_expr_requires_test(&list.tokens.to_string())
            )
    })
}

// ---------------------------------------------------------------------------
// 扫描器 v4:syn AST(含宏 token)+ 列敏感 DML 分类
// ---------------------------------------------------------------------------

const STATE_COLUMNS_PROVIDERS_RS: [&str; 1] = ["is_current"];
const STATE_COLUMNS_FAILOVER_RS: [&str; 1] = ["in_failover_queue"];
/// restore 面基础设施文件:DML 列权限扫描对它们另有归属规则。
const INFRA_FILES: [&str; 3] = [
    "database/schema.rs",
    "database/migration.rs",
    "database/backup.rs",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Dml {
    Insert { table: String },
    Delete { table: String },
    Update { table: String, columns: Vec<String> },
}

/// 表 token:引号成对匹配的交替(裸形式带 `\b`)。R8 终审:引号表名在
/// "可选闭引号 + \b" 的写法上必然失配,必须成对交替。
const TABLE_TOKEN: &str = r#"("provider_endpoints"|"providers"|'provider_endpoints'|'providers'|`provider_endpoints`|`providers`|\[provider_endpoints\]|\[providers\]|provider_endpoints\b|providers\b)"#;
const NAME_PREFIX: &str =
    r#"(?:(?:"(?:[^"]|"")*"|'(?:[^']|'')*'|`[^`]*`|\[[^\]]*\]|\w+)\s*\.\s*)?"#;
const NAME_TOKEN: &str = r#"(?:"(?:[^"]|"")*"|'(?:[^']|'')*'|`[^`]*`|\[[^\]]*\]|\w+)"#;

fn table_from_capture(raw: &str) -> String {
    raw.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .to_lowercase()
}

static INSERT_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?is)\b(?:REPLACE|INSERT(?:\s+OR\s+(?:ABORT|FAIL|IGNORE|REPLACE|ROLLBACK))?)\s+INTO\s+{NAME_PREFIX}{TABLE_TOKEN}"#
    ))
    .expect("compile insert head")
});
static DELETE_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?is)\bDELETE\s+FROM\s+{NAME_PREFIX}{TABLE_TOKEN}"#
    ))
    .expect("compile delete head")
});
static UPDATE_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?is)\bUPDATE(?:\s+OR\s+(?:ABORT|FAIL|IGNORE|REPLACE|ROLLBACK))?\s+{NAME_PREFIX}{TABLE_TOKEN}(?:\s+(?:AS\s+{NAME_TOKEN}|NOT\s+INDEXED|INDEXED\s+BY\s+{NAME_TOKEN}|{NAME_TOKEN}))*?\s+SET\b"#
    ))
    .expect("compile update head")
});
static MACRO_STRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""(?:[^"\\]|\\.)*"|r"[^"]*""#).expect("compile macro string extractor")
});
static FORBIDDEN_SYMBOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bupdate_provider_settings_config\b").expect("compile forbidden symbol")
});

fn contains_provider_dml_anchor(text: &str) -> bool {
    INSERT_HEAD.is_match(text) || DELETE_HEAD.is_match(text) || UPDATE_HEAD.is_match(text)
}

/// 去掉 SQL 注释;引号感知(单引号/双引号/反引号/方括号内的 `--`、`/*`
/// 不是注释)。
fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            out.push(b as char);
            let closing = match q {
                b'[' => b']',
                other => other,
            };
            if b == closing {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' | b'`' | b'[' => {
                quote = Some(b);
                out.push(b as char);
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                out.push(' ');
            }
            b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

const UNPARSEABLE: &str = "!unparseable";

/// SET 列解析:括号深度 + 四类引号感知,顶层 `WHERE`(前一字符不得是
/// `:@$?` 参数记号)或 `;` 终止;列名剥离 alias 前缀与引号;引号/括号
/// 不闭合或列集为空时 fail-closed 产出哨兵列。
fn parse_set_columns(tail: &str) -> Vec<String> {
    let upper = tail.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    let raw = tail.as_bytes();
    let mut depth: i32 = 0;
    let mut quote: Option<u8> = None;
    let mut end = tail.len();
    let mut i = 0;
    while i < bytes.len() {
        let b = raw[i];
        if let Some(q) = quote {
            let closing = match q {
                b'[' => b']',
                other => other,
            };
            if b == closing {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' | b'`' | b'[' => quote = Some(b),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return vec![UNPARSEABLE.to_string()];
                }
            }
            b'W' | b'w' if depth == 0 => {
                let prev = if i == 0 { b' ' } else { raw[i - 1] };
                let boundary_before = !(prev.is_ascii_alphanumeric()
                    || prev == b'_'
                    || matches!(prev, b':' | b'@' | b'$' | b'?'));
                if boundary_before && upper[i..].starts_with("WHERE") {
                    let after = i + 5;
                    let boundary_after = after >= bytes.len()
                        || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_');
                    if boundary_after {
                        end = i;
                        break;
                    }
                }
            }
            b';' if depth == 0 => {
                end = i;
                break;
            }
            _ => {}
        }
        i += 1;
    }
    if quote.is_some() || depth != 0 {
        return vec![UNPARSEABLE.to_string()];
    }
    let clause = &tail[..end];
    let mut columns = Vec::new();
    let mut segment_start = 0;
    let mut depth: i32 = 0;
    let mut quote: Option<u8> = None;
    let clause_bytes = clause.as_bytes();
    let push_segment = |segment: &str, columns: &mut Vec<String>| {
        if let Some(identifier) = segment.split('=').next() {
            let identifier = identifier
                .trim()
                .rsplit('.')
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .to_lowercase();
            if !identifier.is_empty() {
                columns.push(identifier);
            }
        }
    };
    for (i, &b) in clause_bytes.iter().enumerate() {
        if let Some(q) = quote {
            let closing = match q {
                b'[' => b']',
                other => other,
            };
            if b == closing {
                quote = None;
            }
            continue;
        }
        match b {
            b'\'' | b'"' | b'`' | b'[' => quote = Some(b),
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' if depth == 0 => {
                push_segment(&clause[segment_start..i], &mut columns);
                segment_start = i + 1;
            }
            _ => {}
        }
    }
    push_segment(&clause[segment_start..], &mut columns);
    if columns.is_empty() {
        return vec![UNPARSEABLE.to_string()];
    }
    columns
}

fn classify_sql(literal: &str) -> Vec<Dml> {
    let sql = strip_sql_comments(literal);
    let mut found = Vec::new();
    for capture in INSERT_HEAD.captures_iter(&sql) {
        found.push(Dml::Insert {
            table: table_from_capture(&capture[1]),
        });
    }
    for capture in DELETE_HEAD.captures_iter(&sql) {
        found.push(Dml::Delete {
            table: table_from_capture(&capture[1]),
        });
    }
    for capture in UPDATE_HEAD.captures_iter(&sql) {
        let whole = capture.get(0).expect("capture 0");
        found.push(Dml::Update {
            table: table_from_capture(&capture[1]),
            columns: parse_set_columns(&sql[whole.end()..]),
        });
    }
    found
}

#[derive(Default)]
struct ProductionCollector {
    literals: Vec<String>,
    ident_text: String,
    macro_violations: Vec<String>,
}

impl ProductionCollector {
    fn record_macro_tokens(&mut self, macro_name: &str, tokens: &str) {
        self.ident_text.push_str(tokens);
        self.ident_text.push(' ');

        if macro_name == "include" {
            self.macro_violations
                .push("include! smuggles unscanned production code".to_string());
        }

        let mut pieces = Vec::new();
        let mut stripped = String::with_capacity(tokens.len());
        let mut cursor = 0;
        for matched in MACRO_STRING.find_iter(tokens) {
            stripped.push_str(&tokens[cursor..matched.start()]);
            cursor = matched.end();
            let raw = matched.as_str();
            // 经 syn 解码转义(覆盖 \xNN、\u{});r"..." 直接取内容。
            let value = syn::parse_str::<syn::LitStr>(raw)
                .map(|lit| lit.value())
                .unwrap_or_else(|_| {
                    raw.trim_start_matches("r\"")
                        .trim_start_matches('"')
                        .trim_end_matches('"')
                        .to_string()
                });
            self.literals.push(value.clone());
            pieces.push(value);
        }
        stripped.push_str(&tokens[cursor..]);
        // include_str!/include_bytes! 的 .sql 判定作用于原 token 与拼接体
        // (覆盖 concat!("query.", "sql") 拆分),大小写不敏感。
        let joined_pieces = pieces.join("");
        if matches!(macro_name, "include_str" | "include_bytes")
            && (tokens.to_ascii_lowercase().contains(".sql")
                || joined_pieces.to_ascii_lowercase().contains(".sql"))
        {
            self.macro_violations
                .push(format!("{macro_name}! loads external SQL"));
        }
        // 纯字面量:剥离字符串后仅剩标点,且字面量内无 format 插值花括号
        // (`format!("... SET {col} = ...")` 的隐式捕获只有一个字符串 token,
        // 必须按非纯字面量处理——R9 终审绕过)。
        let pure_literal = stripped
            .chars()
            .all(|c| c.is_whitespace() || c == ',' || c == '(' || c == ')')
            && !pieces.iter().any(|piece| piece.contains('{'));

        if pure_literal {
            // concat! 相邻拼接:拼接体整体参与常规分类。
            if pieces.len() > 1 {
                self.literals.push(pieces.join(""));
            }
        } else {
            // 含非字面量 token 的宏(format!/stringify! 构造):一旦出现
            // provider DML 锚点即 fail-closed,不猜插值后的语义。
            let joined = pieces.join("");
            if contains_provider_dml_anchor(tokens)
                || contains_provider_dml_anchor(&joined)
                || pieces.iter().any(|p| contains_provider_dml_anchor(p))
            {
                self.macro_violations.push(format!(
                    "{macro_name}! builds provider DML from non-literal tokens"
                ));
            }
        }
    }
}

impl<'ast> Visit<'ast> for ProductionCollector {
    fn visit_item(&mut self, item: &'ast Item) {
        let attrs = match item {
            Item::Const(item) => Some(&item.attrs),
            Item::Enum(item) => Some(&item.attrs),
            Item::Fn(item) => Some(&item.attrs),
            Item::Impl(item) => Some(&item.attrs),
            Item::Macro(item) => Some(&item.attrs),
            Item::Mod(item) => Some(&item.attrs),
            Item::Static(item) => Some(&item.attrs),
            Item::Struct(item) => Some(&item.attrs),
            Item::Trait(item) => Some(&item.attrs),
            Item::Type(item) => Some(&item.attrs),
            Item::Union(item) => Some(&item.attrs),
            Item::Use(item) => Some(&item.attrs),
            _ => None,
        };
        if attrs.is_some_and(|attrs| attrs_mark_test_only(attrs)) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        let attrs = match item {
            ImplItem::Const(item) => Some(&item.attrs),
            ImplItem::Fn(item) => Some(&item.attrs),
            ImplItem::Type(item) => Some(&item.attrs),
            ImplItem::Macro(item) => Some(&item.attrs),
            _ => None,
        };
        if attrs.is_some_and(|attrs| attrs_mark_test_only(attrs)) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_expr_lit(&mut self, expression: &'ast ExprLit) {
        if let Lit::Str(literal) = &expression.lit {
            self.literals.push(literal.value());
        }
        visit::visit_expr_lit(self, expression);
    }

    fn visit_ident(&mut self, identifier: &'ast syn::Ident) {
        self.ident_text.push_str(&identifier.to_string());
        self.ident_text.push(' ');
    }

    // syn 默认不遍历宏 token:自行提取字符串与符号。
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let name = mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        self.record_macro_tokens(&name, &mac.tokens.to_string());
    }
}

fn collect_production(source: &str) -> Result<ProductionCollector, String> {
    let syntax = syn::parse_file(source).map_err(|error| error.to_string())?;
    if attrs_mark_test_only(&syntax.attrs) {
        return Ok(ProductionCollector::default());
    }
    let mut collector = ProductionCollector::default();
    collector.visit_file(&syntax);
    Ok(collector)
}

fn is_test_convention_file(relative: &str) -> bool {
    let stem = Path::new(relative)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    stem == "tests" || stem.ends_with("_tests") || stem.ends_with("_certification")
}

fn is_infra_file(relative: &str) -> bool {
    INFRA_FILES.contains(&relative)
}

/// authority 判定使用精确相对路径,杜绝 `ends_with` 伪路径冒充。
fn dml_allowed(relative: &str, dml: &Dml) -> bool {
    match relative {
        "database/dao/provider_write.rs" => true,
        "database/dao/providers.rs" => match dml {
            Dml::Delete { table } => table == "providers",
            Dml::Update { table, columns } => {
                table == "providers"
                    && !columns.is_empty()
                    && columns
                        .iter()
                        .all(|c| STATE_COLUMNS_PROVIDERS_RS.contains(&c.as_str()))
            }
            Dml::Insert { .. } => false,
        },
        "database/dao/failover.rs" => match dml {
            Dml::Update { table, columns } => {
                table == "providers"
                    && !columns.is_empty()
                    && columns
                        .iter()
                        .all(|c| STATE_COLUMNS_FAILOVER_RS.contains(&c.as_str()))
            }
            _ => false,
        },
        _ => false,
    }
}

#[test]
fn certify_provider_dml_column_authority() {
    let root = source_root();
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    assert!(
        files.len() > 100,
        "scanner must see the full source tree, found only {} files",
        files.len()
    );
    let mut violations = Vec::new();
    for file in &files {
        let relative = relative_source_path(&root, file);
        if is_infra_file(&relative) {
            continue;
        }
        let source = fs::read_to_string(file).expect("read source file");
        let collector = match collect_production(&source) {
            Ok(collector) => collector,
            Err(error) => {
                violations.push(format!("{relative}: syn parse error: {error}"));
                continue;
            }
        };
        for literal in &collector.literals {
            for dml in classify_sql(literal) {
                if !dml_allowed(&relative, &dml) {
                    violations.push(format!("{relative}: {dml:?}"));
                }
            }
        }
        for violation in &collector.macro_violations {
            violations.push(format!("{relative}: {violation}"));
        }
    }
    assert!(
        violations.is_empty(),
        "provider DML outside the column-granular authority table:\n{}",
        violations.join("\n")
    );
}

#[test]
fn certify_forbidden_symbols_are_zero_treewide() {
    // update_provider_settings_config 是 R4 认定的绕面 mutator:目标是符号
    // 全树归零(定义与调用点一并消失),不是把它搬进写面文件让扫描器沉默。
    // 宏 token 中的出现同样命中(macro_rules 隐藏)。
    let root = source_root();
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    let mut hits = Vec::new();
    for file in &files {
        let relative = relative_source_path(&root, file);
        let source = fs::read_to_string(file).expect("read source file");
        if collect_production(&source)
            .is_ok_and(|collector| FORBIDDEN_SYMBOL.is_match(&collector.ident_text))
        {
            hits.push(relative.clone());
        }
    }
    assert!(
        hits.is_empty(),
        "forbidden mutator symbol still present in: {hits:?}"
    );
}

#[test]
fn certify_update_dto_has_no_created_at() {
    // 裁决 1:created_at 不可变,update DTO 不得携带该字段。
    let root = source_root();
    let source = fs::read_to_string(root.join("database/dao/provider_write.rs"))
        .expect("read provider_write.rs");
    let syntax = syn::parse_file(&source).expect("parse provider_write.rs");
    for item in &syntax.items {
        let Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != "ProviderRowUpdate" {
            continue;
        }
        let has_created_at = item_struct.fields.iter().any(|field| {
            field
                .ident
                .as_ref()
                .is_some_and(|ident| ident == "created_at")
        });
        assert!(
            !has_created_at,
            "ProviderRowUpdate must not carry created_at (immutability ruling)"
        );
        return;
    }
    panic!("ProviderRowUpdate struct not found in provider_write.rs");
}

fn find_fn<'a>(items: &'a [Item], name: &str) -> Option<&'a syn::ItemFn> {
    for item in items {
        match item {
            Item::Fn(function) if function.sig.ident == name => return Some(function),
            Item::Mod(item_mod) => {
                let nested = item_mod.content.as_ref().map(|(_, items)| items.as_slice());
                if let Some(found) = nested.and_then(|items| find_fn(items, name)) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

#[derive(Default)]
struct IdentProbe {
    found: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for IdentProbe {
    fn visit_ident(&mut self, identifier: &'ast syn::Ident) {
        self.found.insert(identifier.to_string());
    }
}

#[test]
fn certify_reconcile_precondition_enum_shape() {
    fn find_enum<'a>(items: &'a [Item], name: &str) -> Option<&'a syn::ItemEnum> {
        for item in items {
            match item {
                Item::Enum(item_enum) if item_enum.ident == name => return Some(item_enum),
                Item::Mod(item_mod) => {
                    let nested = item_mod.content.as_ref().map(|(_, items)| items.as_slice());
                    if let Some(found) = nested.and_then(|items| find_enum(items, name)) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let root = source_root();
    let source = fs::read_to_string(root.join("services/provider/mod.rs"))
        .expect("read services/provider/mod.rs");
    let syntax = syn::parse_file(&source).expect("parse services/provider/mod.rs");
    let precondition =
        find_enum(&syntax.items, "ReconcilePrecondition").expect("ReconcilePrecondition exists");
    let variants: Vec<String> = precondition
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect();
    assert_eq!(
        variants,
        vec!["ExpectAbsent".to_string(), "ExpectPresent".to_string()],
        "ReconcilePrecondition variants drifted from the adjudicated contract"
    );
    let expect_present = precondition
        .variants
        .iter()
        .find(|variant| variant.ident == "ExpectPresent")
        .expect("ExpectPresent variant");
    let field_names: Vec<String> = expect_present
        .fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| ident.to_string()))
        .collect();
    assert_eq!(
        field_names,
        vec!["fingerprint".to_string()],
        "ExpectPresent must carry exactly a fingerprint field"
    );
}

#[test]
fn certify_reconcile_uses_single_transaction_primitives() {
    // 裁决 3:reconcile 不得在函数体内内联读取 aggregate 再分支(那是
    // check-then-act 的新形态);必须委托单事务 DAO 原语。脚手架当前委托旧
    // 函数 → 本测试红,实现方按裁决实现后转绿。
    let root = source_root();
    let source = fs::read_to_string(root.join("services/provider/mod.rs"))
        .expect("read services/provider/mod.rs");
    let syntax = syn::parse_file(&source).expect("parse services/provider/mod.rs");
    let function = find_fn(&syntax.items, "reconcile_provider_record_with_precondition")
        .expect("reconcile_provider_record_with_precondition exists");
    let mut probe = IdentProbe::default();
    probe.visit_block(&function.block);
    for banned in [
        "get_provider_aggregate",
        "get_provider_by_id",
        "get_all_providers",
        "get_all_provider_aggregates",
        "reconcile_provider_record",
    ] {
        assert!(
            !probe.found.contains(banned),
            "reconcile body must not use '{banned}'; delegate to single-transaction DAO primitives"
        );
    }
    for required in ["create_provider", "update_provider_if_content_fingerprint"] {
        assert!(
            probe.found.contains(required),
            "reconcile body must delegate to '{required}'"
        );
    }
}

#[test]
fn certify_test_convention_files_are_cfg_test_gated() {
    // 命名约定的测试文件必须自带文件级 #![cfg(test)]:借用他处注册的伪测试名
    // 生产文件在此失败;带该属性的文件在任何构建里都不进入生产目标。
    let root = source_root();
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    for file in &files {
        let relative = relative_source_path(&root, file);
        if !is_test_convention_file(&relative) {
            continue;
        }
        let source = fs::read_to_string(file).expect("read source file");
        let syntax = syn::parse_file(&source).expect("parse convention file");
        assert!(
            attrs_mark_test_only(&syntax.attrs),
            "{relative} uses a test naming convention but lacks a file-level #![cfg(test)]"
        );
    }
}

#[test]
fn certify_scanner_negative_matrix() {
    let content = |sql: &str| -> bool {
        classify_sql(sql).iter().any(|dml| match dml {
            Dml::Insert { table } | Dml::Delete { table } => table == "providers",
            Dml::Update { table, columns } => {
                table == "providers"
                    && columns.iter().any(|c| {
                        c == "settings_config" || c == "name" || c == "meta" || c == UNPARSEABLE
                    })
            }
        })
    };
    // 大小写
    assert!(content(
        "update providers set settings_config = ?1 where id = ?2"
    ));
    // 引号/反引号/方括号表名(R8:引号表名曾在 \b 上失配)
    assert!(content(r#"UPDATE "providers" SET name = ?1 WHERE id = ?2"#));
    assert!(content("UPDATE `providers` SET name = ?1 WHERE id = ?2"));
    assert!(content("UPDATE [providers] SET name = ?1 WHERE id = ?2"));
    assert!(content(r#"DELETE FROM "providers" WHERE id = ?1"#));
    assert!(content(r#"INSERT INTO "providers" (id) VALUES (?1)"#));
    // schema 前缀(含引号 schema)
    assert!(content("UPDATE main.providers SET meta = ?1 WHERE id = ?2"));
    assert!(content(
        r#"UPDATE "main".providers SET meta = ?1 WHERE id = ?2"#
    ));
    // 别名:AS、AS 带引号(含非 \w 字符)、裸别名、INDEXED BY / NOT INDEXED
    assert!(content(
        "UPDATE providers AS p SET settings_config = ?1 WHERE p.id = ?2"
    ));
    assert!(content(
        r#"UPDATE providers AS "p-x" SET name = ?1 WHERE id = ?2"#
    ));
    assert!(content(
        r#"UPDATE providers AS "p""x" SET name = ?1 WHERE id = ?2"#
    ));
    assert!(content(
        r#"UPDATE providers "bare-alias" SET name = ?1 WHERE id = ?2"#
    ));
    assert!(content(
        "UPDATE providers p SET p.settings_config = ?1 WHERE p.id = ?2"
    ));
    assert!(content(
        "UPDATE providers INDEXED BY idx SET name = ?1 WHERE id = ?2"
    ));
    assert!(content(
        r#"UPDATE providers INDEXED BY "i-1" SET name = ?1 WHERE id = ?2"#
    ));
    assert!(content(
        "UPDATE providers NOT INDEXED SET name = ?1 WHERE id = ?2"
    ));
    // providers_seed 等相邻表名不得误报
    assert!(!content("INSERT INTO providers_seed (id) VALUES (?1)"));
    assert!(!content(
        "UPDATE universal_providers SET name = ?1 WHERE id = ?2"
    ));
    // OR 冲突子句与 REPLACE INTO
    assert!(content(
        "UPDATE OR REPLACE providers SET name = ?1 WHERE id = ?2"
    ));
    assert!(content(
        "REPLACE INTO providers (id, app_type) VALUES (?1, ?2)"
    ));
    assert!(content("INSERT OR REPLACE INTO providers (id) VALUES (?1)"));
    // 注释拆词
    assert!(content(
        "UPDATE /* sneak */ providers SET settings_config = ?1 WHERE id = ?2"
    ));
    assert!(content(
        "UPDATE providers -- x\n SET name = ?1 WHERE id = ?2"
    ));
    // 引号内的注释记号不是注释('--' 字符串吞列绕过)
    let quoted_comment = "UPDATE providers SET is_current = '--', name = ?1 WHERE id = ?2";
    let classified = classify_sql(quoted_comment);
    assert!(
        classified.iter().any(|dml| matches!(
            dml,
            Dml::Update { columns, .. } if columns.contains(&"name".to_string())
        )),
        "comment markers inside SQL strings must not swallow columns: {classified:?}"
    );
    // 命名参数 :where 不得截断列解析
    let named_param = "UPDATE providers SET is_current = :where, name = ?1 WHERE id = ?2";
    let classified = classify_sql(named_param);
    assert!(
        classified.iter().any(|dml| matches!(
            dml,
            Dml::Update { columns, .. } if columns.contains(&"name".to_string())
        )),
        "named parameter :where must not terminate column parsing: {classified:?}"
    );
    // 双引号内容破坏深度 → fail-closed 哨兵
    let sabotage = r#"UPDATE providers SET is_current = ")", name = ?1 WHERE id = ?2"#;
    assert!(content(sabotage), "quoted parens must not hide columns");
    // 子查询误导:内层 WHERE 不得截断列解析
    let subquery = "UPDATE providers SET is_current = (SELECT max(id) FROM t WHERE y = 1), settings_config = ?1 WHERE id = ?2";
    let classified = classify_sql(subquery);
    assert!(
        classified.iter().any(|dml| matches!(
            dml,
            Dml::Update { table, columns }
                if table == "providers"
                    && columns.contains(&"is_current".to_string())
                    && columns.contains(&"settings_config".to_string())
        )),
        "subquery WHERE must not truncate column parsing: {classified:?}"
    );
    // 多语句
    let batch = "UPDATE providers SET is_current = 1 WHERE id = 1; UPDATE providers SET settings_config = 'x' WHERE id = 2";
    assert_eq!(classify_sql(batch).len(), 2);
    assert!(content(batch));
    // 不闭合引号 → fail-closed
    assert!(content(
        "UPDATE providers SET is_current = 'unterminated WHERE id = 1"
    ));
    // 状态列合法写法必须放行(防过杀)
    let state_only = classify_sql("UPDATE providers SET is_current = 0 WHERE app_type = ?1");
    assert!(state_only
        .iter()
        .all(|dml| dml_allowed("database/dao/providers.rs", dml)));
    // 内容列即使在状态 authority 内也必须拦下(R4 逃逸场景)
    let escaped = classify_sql("UPDATE providers SET settings_config = ?1 WHERE id = ?2");
    assert!(escaped
        .iter()
        .any(|dml| !dml_allowed("database/dao/providers.rs", dml)));
    // endpoints:touch-only 放行于写面,内容列到处拦
    let touch = classify_sql("UPDATE provider_endpoints SET last_used = ?1 WHERE provider_id = ?2");
    assert!(touch
        .iter()
        .all(|dml| dml_allowed("database/dao/provider_write.rs", dml)));
    let ep_content = classify_sql("UPDATE provider_endpoints SET url = ?1 WHERE provider_id = ?2");
    assert!(ep_content
        .iter()
        .any(|dml| !dml_allowed("database/dao/providers.rs", dml)));
    // 精确路径:伪路径不得冒充 authority
    let state = Dml::Update {
        table: "providers".to_string(),
        columns: vec!["is_current".to_string()],
    };
    assert!(dml_allowed("database/dao/providers.rs", &state));
    assert!(!dml_allowed("services/database/dao/providers.rs", &state));
    assert!(!dml_allowed(
        "evil/database/dao/provider_write.rs",
        &Dml::Insert {
            table: "providers".to_string()
        }
    ));
    // cfg 语义:not(test) 与 any(test, unix) 是生产代码
    assert!(!cfg_expr_requires_test("not (test)"));
    assert!(!cfg_expr_requires_test("any (test , unix)"));
    assert!(cfg_expr_requires_test("test"));
    assert!(cfg_expr_requires_test("all (test , unix)"));
    assert!(cfg_expr_requires_test("any (test , all (test , unix))"));
    // 宏:concat! 纯字面量拼接 → 正常分类,不误报违规
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens(
        "concat",
        r#""UPDATE providers " , "SET name = ?1 WHERE id = ?2""#,
    );
    assert!(
        collector.literals.iter().any(|lit| content(lit)),
        "concat!-joined SQL must be classified"
    );
    assert!(collector.macro_violations.is_empty());
    // 宏:format! 含非字面量 + DML 锚点 → 无条件违规(即使可见列全是状态列)
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens(
        "format",
        r#""UPDATE providers SET is_current = 0 , {} = ?1 WHERE id = ?2" , column"#,
    );
    assert!(
        !collector.macro_violations.is_empty(),
        "format!-built provider DML must fail closed even when visible columns look like state"
    );
    // 宏:stringify! 式 ident 构造 SQL → token 锚点命中
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens("stringify", "UPDATE providers SET name = x WHERE id = y");
    assert!(
        !collector.macro_violations.is_empty(),
        "ident-built provider DML must fail closed"
    );
    // 宏:无 DML 锚点的普通 format! 不误报
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens("format", r#""hello {}" , name"#);
    assert!(collector.macro_violations.is_empty());
    // 宏:include! 全禁,include_str!(.sql) 禁,普通资源不误报
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens("include", r#""../generated.rs""#);
    assert!(!collector.macro_violations.is_empty());
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens("include_str", r#""queries/update.sql""#);
    assert!(!collector.macro_violations.is_empty());
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens("include_bytes", r#""queries/update.SQL""#);
    assert!(
        !collector.macro_violations.is_empty(),
        "include_bytes and case variants must be banned for SQL"
    );
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens("include_str", r#"concat ! ("query." , "sql")"#);
    assert!(
        !collector.macro_violations.is_empty(),
        "extension split via concat! must still be detected"
    );
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens("include_str", r#""resources/template.json""#);
    assert!(collector.macro_violations.is_empty());
    // 隐式 format 捕获:单字符串 + 花括号插值不得被当纯字面量放行(R9)
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens(
        "format",
        r#""UPDATE providers SET {is_current} = ?1 WHERE id = ?2""#,
    );
    assert!(
        !collector.macro_violations.is_empty(),
        "implicit format captures must fail closed"
    );
    // 宏字符串转义:\x55(U)解码后仍识别
    let mut collector = ProductionCollector::default();
    collector.record_macro_tokens(
        "concat",
        r#""\x55PDATE providers " , "SET name = ?1 WHERE id = ?2""#,
    );
    assert!(
        collector.literals.iter().any(|lit| content(lit)),
        "escaped SQL must be decoded via LitStr::value before classification"
    );
}

// ---------------------------------------------------------------------------
// T3:create 冲突原子性与结构化 Conflict(裁决 2)
// ---------------------------------------------------------------------------

#[test]
fn certify_duplicate_create_returns_structured_conflict() {
    let database = db();
    let first = with_endpoints(
        {
            let mut input = base_input("dup", "第一次创建");
            input.sort_index = Some(5);
            input.in_failover_queue = true;
            input
        },
        &[("https://a.example", Some(11), None)],
    );
    database
        .create_provider(NewProviderAggregate::from_input("claude", first).unwrap())
        .expect("first create");
    let before = snapshot(&database, "claude", "dup");

    let second = with_endpoints(
        base_input("dup", "冒名顶替"),
        &[("https://b.example", Some(22), None)],
    );
    let err = database
        .create_provider(NewProviderAggregate::from_input("claude", second).unwrap())
        .expect_err("duplicate create must fail, not upsert");
    assert!(
        matches!(err, AppError::Conflict(_)),
        "duplicate create must surface a structured Conflict, got: {err:?}"
    );
    assert_eq!(
        snapshot(&database, "claude", "dup"),
        before,
        "duplicate create must leave row, endpoints and state untouched"
    );
}

#[test]
fn certify_create_does_not_touch_current_state() {
    let database = db();
    database
        .create_provider(
            NewProviderAggregate::from_input("claude", base_input("first", "既有")).unwrap(),
        )
        .expect("create first");
    database
        .set_current_provider("claude", "first")
        .expect("set current");
    database
        .create_provider(
            NewProviderAggregate::from_input("claude", base_input("second", "新建")).unwrap(),
        )
        .expect("create second");
    let (first_row, _) = snapshot(&database, "claude", "first");
    let (second_row, _) = snapshot(&database, "claude", "second");
    assert_eq!(
        first_row.expect("first row").10,
        1,
        "create must not clear another provider's is_current"
    );
    assert_eq!(
        second_row.expect("second row").10,
        0,
        "create must never set is_current on the new row"
    );
}

#[test]
fn certify_create_endpoint_failure_rolls_back_row() {
    let database = db();
    install_endpoint_reject_trigger(&database);
    let input = with_endpoints(
        base_input("halfway", "半途失败"),
        &[("https://blocked.example", Some(1), None)],
    );
    database
        .create_provider(NewProviderAggregate::from_input("claude", input).unwrap())
        .expect_err("endpoint insert failure must fail the create");
    let (row, endpoints) = snapshot(&database, "claude", "halfway");
    assert!(
        row.is_none() && endpoints.is_empty(),
        "failed create must leave no partial row"
    );
}

// ---------------------------------------------------------------------------
// T4:update 严格单行、created_at 不可变、状态列保全、全内容往返
// ---------------------------------------------------------------------------

#[test]
fn certify_update_missing_provider_is_notfound_and_creates_nothing() {
    let database = db();
    let key = ProviderKey::new("claude", "ghost").unwrap();
    let row = ProviderRowUpdate::from_input(&base_input("ghost", "幽灵")).unwrap();
    let err = database.update_provider(&key, &row).expect_err("must fail");
    assert!(matches!(err, AppError::NotFound(_)), "got: {err:?}");
    let (row_after, endpoints_after) = snapshot(&database, "claude", "ghost");
    assert!(row_after.is_none() && endpoints_after.is_empty());
}

#[test]
fn certify_update_cannot_change_created_at() {
    let database = db();
    let mut created = base_input("epoch", "创建时间");
    created.created_at = Some(111);
    database
        .create_provider(NewProviderAggregate::from_input("claude", created).unwrap())
        .expect("create");
    let key = ProviderKey::new("claude", "epoch").unwrap();
    let mut edited = base_input("epoch", "被编辑");
    edited.created_at = Some(222);
    database
        .update_provider(&key, &ProviderRowUpdate::from_input(&edited).unwrap())
        .expect("update");
    let (row, _) = snapshot(&database, "claude", "epoch");
    assert_eq!(
        row.expect("row").4,
        Some(111),
        "update must never rewrite created_at"
    );
}

#[test]
fn certify_update_preserves_all_state_columns() {
    let database = db();
    let created = {
        let mut input = base_input("stately", "状态在身");
        input.sort_index = Some(9);
        input.in_failover_queue = true;
        input
    };
    database
        .create_provider(NewProviderAggregate::from_input("claude", created).unwrap())
        .expect("create");
    database
        .set_current_provider("claude", "stately")
        .expect("set current");
    let key = ProviderKey::new("claude", "stately").unwrap();
    database
        .update_provider(
            &key,
            &ProviderRowUpdate::from_input(&base_input("stately", "改名")).unwrap(),
        )
        .expect("update");
    let (row, _) = snapshot(&database, "claude", "stately");
    let row = row.expect("row");
    assert_eq!(row.5, Some(9), "sort_index must survive row update");
    assert_eq!(row.10, 1, "is_current must survive row update");
    assert_eq!(row.11, 1, "in_failover_queue must survive row update");
}

#[test]
fn certify_full_content_roundtrip_via_create_and_update() {
    // 全列往返认证(含 meta):忽略任一内容列的实现都不得变绿。
    let database = db();
    let mut created = base_input("full", "全字段");
    created.settings_config = json!({"base_url": "https://one.example", "model": "m1"});
    created.website_url = Some("https://site.example".to_string());
    created.category = Some("cat-a".to_string());
    created.notes = Some("初始备注".to_string());
    created.icon = Some("icon-a".to_string());
    created.icon_color = Some("#111111".to_string());
    created.meta = Some(ProviderMeta {
        common_config_enabled: Some(true),
        ..Default::default()
    });
    database
        .create_provider(NewProviderAggregate::from_input("claude", created).unwrap())
        .expect("create");
    let (row, _) = snapshot(&database, "claude", "full");
    let row = row.expect("row");
    assert_eq!(row.0, "全字段");
    assert!(row.1.contains("https://one.example"));
    assert_eq!(row.2.as_deref(), Some("https://site.example"));
    assert_eq!(row.3.as_deref(), Some("cat-a"));
    assert_eq!(row.6.as_deref(), Some("初始备注"));
    assert_eq!(row.7.as_deref(), Some("icon-a"));
    assert_eq!(row.8.as_deref(), Some("#111111"));
    let meta_json: serde_json::Value =
        serde_json::from_str(&row.9).expect("stored meta must be valid JSON");
    assert_eq!(
        meta_json["commonConfigEnabled"],
        json!(true),
        "meta content must round-trip through create, got: {}",
        row.9
    );

    let key = ProviderKey::new("claude", "full").unwrap();
    let mut edited = base_input("full", "全字段二版");
    edited.settings_config = json!({"base_url": "https://two.example", "model": "m2"});
    edited.website_url = Some("https://site2.example".to_string());
    edited.category = Some("cat-b".to_string());
    edited.notes = Some("二版备注".to_string());
    edited.icon = Some("icon-b".to_string());
    edited.icon_color = Some("#222222".to_string());
    edited.meta = Some(ProviderMeta {
        common_config_enabled: Some(false),
        ..Default::default()
    });
    database
        .update_provider(&key, &ProviderRowUpdate::from_input(&edited).unwrap())
        .expect("update");
    let (row, _) = snapshot(&database, "claude", "full");
    let row = row.expect("row");
    assert_eq!(row.0, "全字段二版");
    assert!(row.1.contains("https://two.example"));
    assert_eq!(row.2.as_deref(), Some("https://site2.example"));
    assert_eq!(row.3.as_deref(), Some("cat-b"));
    assert_eq!(row.6.as_deref(), Some("二版备注"));
    assert_eq!(row.7.as_deref(), Some("icon-b"));
    assert_eq!(row.8.as_deref(), Some("#222222"));
    let meta_json: serde_json::Value =
        serde_json::from_str(&row.9).expect("stored meta must be valid JSON");
    assert_eq!(
        meta_json["commonConfigEnabled"],
        json!(false),
        "meta content must round-trip through update, got: {}",
        row.9
    );
}

// ---------------------------------------------------------------------------
// T5:陈旧快照下并发 endpoint 变更存活(R3 核心场景)+ endpoint 严格性
// ---------------------------------------------------------------------------

#[test]
fn certify_concurrent_endpoint_changes_survive_row_update() {
    let database = db();
    let created = with_endpoints(
        base_input("surv", "并发存活"),
        &[("https://old.example", Some(1), None)],
    );
    database
        .create_provider(NewProviderAggregate::from_input("claude", created).unwrap())
        .expect("create");
    let key = ProviderKey::new("claude", "surv").unwrap();

    database
        .add_provider_endpoint(
            &key,
            NewEndpoint::new("https://new.example", Some(2), None).unwrap(),
        )
        .expect("add");
    database
        .remove_provider_endpoint(&key, "https://old.example")
        .expect("remove");
    database
        .touch_provider_endpoint(&key, "https://new.example", 99)
        .expect("touch");

    let row = ProviderRowUpdate::from_input(&base_input("surv", "改名")).unwrap();
    database.update_provider(&key, &row).expect("row update");

    let (_, endpoints) = snapshot(&database, "claude", "surv");
    assert_eq!(
        endpoints,
        vec![("https://new.example".to_string(), Some(2), Some(99))],
        "all concurrent endpoint mutations must survive a row update"
    );
}

#[test]
fn certify_update_payload_with_endpoints_is_rejected_explicitly() {
    let stale = with_endpoints(
        base_input("surv", "夹带"),
        &[("https://smuggle.example", Some(3), None)],
    );
    let err = ProviderRowUpdate::from_input(&stale).expect_err("must reject");
    assert!(matches!(err, AppError::InvalidInput(_)), "got: {err:?}");
}

#[test]
fn certify_endpoint_mutations_are_strict() {
    let database = db();
    database
        .create_provider(
            NewProviderAggregate::from_input(
                "claude",
                with_endpoints(
                    base_input("strict", "严格"),
                    &[("https://one.example", Some(7), None)],
                ),
            )
            .unwrap(),
        )
        .expect("create");
    let key = ProviderKey::new("claude", "strict").unwrap();
    let before = snapshot(&database, "claude", "strict");

    database
        .add_provider_endpoint(
            &key,
            NewEndpoint::new("https://one.example", Some(8), None).unwrap(),
        )
        .expect_err("duplicate endpoint add must fail");
    assert_eq!(snapshot(&database, "claude", "strict"), before);

    assert!(matches!(
        database.remove_provider_endpoint(&key, "https://none.example"),
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        database.touch_provider_endpoint(&key, "https://none.example", 1),
        Err(AppError::NotFound(_))
    ));

    database
        .touch_provider_endpoint(&key, "https://one.example", 55)
        .expect("touch");
    let (_, endpoints) = snapshot(&database, "claude", "strict");
    assert_eq!(
        endpoints,
        vec![("https://one.example".to_string(), Some(7), Some(55))],
        "touch must change last_used only"
    );
}

// ---------------------------------------------------------------------------
// T6:added_at NULL 全链路无损
// ---------------------------------------------------------------------------

#[test]
fn certify_null_added_at_roundtrips_losslessly() {
    let database = db();
    let created = with_endpoints(
        base_input("nulls", "空值"),
        &[("https://n.example", None, None)],
    );
    database
        .create_provider(NewProviderAggregate::from_input("claude", created).unwrap())
        .expect("create");

    let (_, raw) = snapshot(&database, "claude", "nulls");
    assert_eq!(
        raw,
        vec![("https://n.example".to_string(), None, None)],
        "storage must keep NULL, not 0"
    );

    let aggregate = database
        .get_provider_aggregate("claude", "nulls")
        .expect("hydrate")
        .expect("exists");
    let endpoint = aggregate
        .endpoints
        .get("https://n.example")
        .expect("endpoint present in hydration");
    assert_eq!(
        endpoint.added_at, None,
        "hydration must not coerce NULL added_at to 0"
    );
    assert_eq!(endpoint.last_used, None);
}

// ---------------------------------------------------------------------------
// T7:rename 认证矩阵
// ---------------------------------------------------------------------------

fn create_opencode_provider(database: &Database, id: &str) {
    let created = with_endpoints(
        {
            let mut input = base_input(id, "opencode 源");
            input.sort_index = Some(3);
            input.in_failover_queue = true;
            input
        },
        &[("https://keep.example", None, Some(42))],
    );
    database
        .create_provider(NewProviderAggregate::from_input("opencode", created).unwrap())
        .expect("create opencode provider");
}

#[test]
fn certify_rename_preserves_endpoints_nulls_state_and_current() {
    let database = db();
    create_opencode_provider(&database, "old-key");
    database
        .set_current_provider("opencode", "old-key")
        .expect("set current");
    let source = ProviderKey::new("opencode", "old-key").unwrap();
    let rename =
        RenameProvider::from_input(source, &base_input("new-key", "改键")).expect("build rename");
    database
        .rename_db_only_additive_provider(rename)
        .expect("rename");

    let (old_row, old_eps) = snapshot(&database, "opencode", "old-key");
    assert!(
        old_row.is_none() && old_eps.is_empty(),
        "source must be gone"
    );

    let (new_row, new_eps) = snapshot(&database, "opencode", "new-key");
    let new_row = new_row.expect("target row");
    assert_eq!(new_row.0, "改键", "row content must come from rename input");
    assert_eq!(new_row.5, Some(3), "sort_index must carry over");
    assert_eq!(new_row.10, 1, "is_current must carry over");
    assert_eq!(new_row.11, 1, "in_failover_queue must carry over");
    assert_eq!(
        new_eps,
        vec![("https://keep.example".to_string(), None, Some(42))],
        "endpoints must carry over with NULL timestamps intact"
    );
}

#[test]
fn certify_rename_target_conflict_has_zero_side_effects() {
    let database = db();
    create_opencode_provider(&database, "src");
    create_opencode_provider(&database, "dst");
    let before_src = snapshot(&database, "opencode", "src");
    let before_dst = snapshot(&database, "opencode", "dst");

    let source = ProviderKey::new("opencode", "src").unwrap();
    let rename = RenameProvider::from_input(source, &base_input("dst", "撞车")).expect("build");
    database
        .rename_db_only_additive_provider(rename)
        .expect_err("rename onto an existing key must fail");

    assert_eq!(snapshot(&database, "opencode", "src"), before_src);
    assert_eq!(snapshot(&database, "opencode", "dst"), before_dst);
}

#[test]
fn certify_rename_endpoint_copy_failure_is_atomic() {
    let database = db();
    create_opencode_provider(&database, "guarded");
    let before = snapshot(&database, "opencode", "guarded");
    install_endpoint_reject_trigger(&database);

    let source = ProviderKey::new("opencode", "guarded").unwrap();
    let rename = RenameProvider::from_input(source, &base_input("moved", "搬家")).expect("build");
    database
        .rename_db_only_additive_provider(rename)
        .expect_err("endpoint copy failure must fail the rename");

    assert_eq!(
        snapshot(&database, "opencode", "guarded"),
        before,
        "failed rename must leave the source fully intact"
    );
    let (moved_row, moved_eps) = snapshot(&database, "opencode", "moved");
    assert!(
        moved_row.is_none() && moved_eps.is_empty(),
        "failed rename must leave no partial target"
    );
}

#[test]
fn certify_rename_scope_restrictions() {
    let claude_source = ProviderKey::new("claude", "any").unwrap();
    assert!(matches!(
        RenameProvider::from_input(claude_source, &base_input("other", "x")),
        Err(AppError::InvalidInput(_))
    ));

    let database = db();
    for category in ["omo", "omo-slim"] {
        let id = format!("omo-{category}");
        let mut omo = base_input(&id, "omo");
        omo.category = Some(category.to_string());
        database
            .create_provider(NewProviderAggregate::from_input("opencode", omo).unwrap())
            .expect("create omo provider");
        let source = ProviderKey::new("opencode", &id).unwrap();
        let rename =
            RenameProvider::from_input(source, &base_input("omo-target", "y")).expect("build");
        assert!(
            matches!(
                database.rename_db_only_additive_provider(rename),
                Err(AppError::InvalidInput(_))
            ),
            "{category} providers must not be renamable"
        );
    }

    let ghost = ProviderKey::new("opencode", "ghost").unwrap();
    let rename = RenameProvider::from_input(ghost, &base_input("anywhere", "z")).expect("build");
    assert!(matches!(
        database.rename_db_only_additive_provider(rename),
        Err(AppError::NotFound(_))
    ));
}

// ---------------------------------------------------------------------------
// D1:delete 补偿原语必须能重建完整 aggregate
// ---------------------------------------------------------------------------

#[test]
fn certify_delete_compensation_recreates_exact_aggregate() {
    let database = db();
    let created = with_endpoints(
        {
            let mut input = base_input("comp", "补偿对象");
            input.sort_index = Some(5);
            input.in_failover_queue = true;
            input.notes = Some("完整字段".to_string());
            input
        },
        &[
            ("https://a.example", Some(11), Some(20)),
            ("https://b.example", None, None),
        ],
    );
    database
        .create_provider(NewProviderAggregate::from_input("claude", created).unwrap())
        .expect("create");
    database
        .set_current_provider("claude", "comp")
        .expect("set current");
    let before = snapshot(&database, "claude", "comp");

    database
        .delete_provider("claude", "comp")
        .expect("delete provider");
    let (gone_row, gone_eps) = snapshot(&database, "claude", "comp");
    assert!(
        gone_row.is_none() && gone_eps.is_empty(),
        "delete must cascade endpoints"
    );

    // 补偿:必须能从快照原样重建已删除的 aggregate(update-first 语义在此
    // 必败——补偿原语要求 insert-or-restore 语义)。created_at 由专属参数
    // 携带(裁决 1)。
    let key = ProviderKey::new("claude", "comp").unwrap();
    let row = ProviderRowUpdate::from_input(&{
        let mut input = base_input("comp", "补偿对象");
        input.notes = Some("完整字段".to_string());
        input
    })
    .unwrap();
    let endpoints = [
        NewEndpoint::new("https://a.example", Some(11), Some(20)).unwrap(),
        NewEndpoint::new("https://b.example", None, None).unwrap(),
    ];
    {
        let mut conn = database.conn.lock().expect("lock certification database");
        let tx = conn.transaction().expect("open compensation transaction");
        provider_write::restore_provider_aggregate_on_tx(
            &tx,
            &key,
            &row,
            Some(1_700_000_000),
            Some(5),
            true,
            true,
            &endpoints,
        )
        .expect("compensation must recreate a deleted aggregate");
        tx.commit().expect("commit compensation");
    }
    assert_eq!(
        snapshot(&database, "claude", "comp"),
        before,
        "restored aggregate must be byte-identical to the pre-delete snapshot"
    );
}

#[test]
fn certify_delete_compensation_failure_leaves_no_partial_state() {
    let database = db();
    let created = with_endpoints(
        base_input("comp2", "补偿失败"),
        &[("https://c.example", Some(1), None)],
    );
    database
        .create_provider(NewProviderAggregate::from_input("claude", created).unwrap())
        .expect("create");
    database
        .delete_provider("claude", "comp2")
        .expect("delete provider");
    install_endpoint_reject_trigger(&database);

    let key = ProviderKey::new("claude", "comp2").unwrap();
    let row = ProviderRowUpdate::from_input(&base_input("comp2", "补偿失败")).unwrap();
    let endpoints = [NewEndpoint::new("https://c.example", Some(1), None).unwrap()];
    let err = {
        let mut conn = database.conn.lock().expect("lock certification database");
        let tx = conn.transaction().expect("open compensation transaction");
        provider_write::restore_provider_aggregate_on_tx(
            &tx,
            &key,
            &row,
            Some(1_700_000_000),
            None,
            false,
            false,
            &endpoints,
        )
        .expect_err("endpoint restore failure must fail the compensation")
        // 事务随 drop 回滚
    };
    assert!(
        err.to_string().contains(ENDPOINT_REJECT_MESSAGE),
        "compensation must fail at the injected endpoint restore, not before it; got: {err}"
    );
    let (row_after, eps_after) = snapshot(&database, "claude", "comp2");
    assert!(
        row_after.is_none() && eps_after.is_empty(),
        "failed compensation must not leave a row without its endpoints"
    );
}

// ---------------------------------------------------------------------------
// T8/T9:reconcile 显式前置期望
// ---------------------------------------------------------------------------

#[test]
fn certify_reconcile_expect_present_preserves_endpoints_and_state() {
    let database = db();
    let created = with_endpoints(
        {
            let mut input = base_input("recon", "用户创建");
            input.sort_index = Some(7);
            input.in_failover_queue = true;
            input
        },
        &[("https://user.example", Some(5), None)],
    );
    database
        .create_provider(NewProviderAggregate::from_input("claude", created).unwrap())
        .expect("create");
    database
        .set_current_provider("claude", "recon")
        .expect("set current");
    let aggregate = database
        .get_provider_aggregate("claude", "recon")
        .expect("hydrate")
        .expect("exists");
    let fingerprint = provider_row_fingerprint(&aggregate.provider);

    reconcile_provider_record_with_precondition(
        &database,
        "claude",
        base_input("recon", "同步覆盖"),
        ReconcilePrecondition::ExpectPresent { fingerprint },
    )
    .expect("reconcile existing with fresh fingerprint");

    let (row, endpoints) = snapshot(&database, "claude", "recon");
    let row = row.expect("row");
    assert_eq!(row.0, "同步覆盖", "row content may be reconciled");
    assert_eq!(
        row.5,
        Some(7),
        "sort_index is state, reconcile must not clear it"
    );
    assert_eq!(
        row.10, 1,
        "is_current is state, reconcile must not clear it"
    );
    assert_eq!(
        row.11, 1,
        "failover membership is state, reconcile must not clear it"
    );
    assert_eq!(
        endpoints,
        vec![("https://user.example".to_string(), Some(5), None)],
        "reconcile of an existing provider must never touch endpoints"
    );
}

#[test]
fn certify_reconcile_expect_absent_creates_with_initial_endpoints() {
    let database = db();
    reconcile_provider_record_with_precondition(
        &database,
        "claude",
        with_endpoints(
            base_input("fresh", "同步新建"),
            &[("https://seed.example", Some(9), None)],
        ),
        ReconcilePrecondition::ExpectAbsent,
    )
    .expect("reconcile missing");
    let (row, endpoints) = snapshot(&database, "claude", "fresh");
    assert!(row.is_some());
    assert_eq!(
        endpoints,
        vec![("https://seed.example".to_string(), Some(9), None)]
    );
}

#[test]
fn certify_reconcile_expect_absent_loser_cannot_overwrite_winner() {
    // T9(TOCTOU 本体):观察为 Absent 后输掉竞争,必须结构化 Conflict,
    // 绝不退化为覆盖更新。
    let database = db();
    let winner = with_endpoints(
        base_input("race-slot", "竞争赢家"),
        &[("https://winner.example", Some(1), None)],
    );
    database
        .create_provider(NewProviderAggregate::from_input("claude", winner).unwrap())
        .expect("winner create");
    let before = snapshot(&database, "claude", "race-slot");

    let err = reconcile_provider_record_with_precondition(
        &database,
        "claude",
        base_input("race-slot", "迟到输家"),
        ReconcilePrecondition::ExpectAbsent,
    )
    .expect_err("losing an ExpectAbsent race must surface an error");
    assert!(
        matches!(err, AppError::Conflict(_)),
        "race loser must get a structured Conflict, got: {err:?}"
    );
    assert_eq!(
        snapshot(&database, "claude", "race-slot"),
        before,
        "the winner's row must remain byte-identical"
    );
}

#[test]
fn certify_reconcile_expect_present_stale_fingerprint_conflicts() {
    let database = db();
    database
        .create_provider(
            NewProviderAggregate::from_input("claude", base_input("staleful", "第一版")).unwrap(),
        )
        .expect("create");
    let aggregate = database
        .get_provider_aggregate("claude", "staleful")
        .expect("hydrate")
        .expect("exists");
    let stale_fingerprint = provider_row_fingerprint(&aggregate.provider);

    // 其他写者更新了行内容,持旧指纹的 reconcile 必须 Conflict。
    let key = ProviderKey::new("claude", "staleful").unwrap();
    database
        .update_provider(
            &key,
            &ProviderRowUpdate::from_input(&base_input("staleful", "第二版")).unwrap(),
        )
        .expect("interleaved update");

    let err = reconcile_provider_record_with_precondition(
        &database,
        "claude",
        base_input("staleful", "第三版"),
        ReconcilePrecondition::ExpectPresent {
            fingerprint: stale_fingerprint,
        },
    )
    .expect_err("stale fingerprint must surface an error");
    assert!(
        matches!(err, AppError::Conflict(_)),
        "stale fingerprint must get a structured Conflict, got: {err:?}"
    );
    let (row, _) = snapshot(&database, "claude", "staleful");
    assert_eq!(
        row.expect("row").0,
        "第二版",
        "stale reconcile must not overwrite the interleaved writer"
    );
}

#[test]
fn certify_fingerprint_is_deterministic_and_endpoint_blind() {
    // preserve_order + HashMap 意味着朴素序列化指纹不稳定(伪 Conflict);
    // 指纹必须走规范化排序哈希,且不受 endpoint 填充差异影响。
    let database = db();
    database
        .create_provider(
            NewProviderAggregate::from_input(
                "claude",
                with_endpoints(
                    {
                        let mut input = base_input("fp", "指纹");
                        input.meta = Some(ProviderMeta {
                            common_config_enabled: Some(true),
                            ..Default::default()
                        });
                        input
                    },
                    &[("https://e.example", Some(1), None)],
                ),
            )
            .unwrap(),
        )
        .expect("create");
    let via_aggregate = database
        .get_provider_aggregate("claude", "fp")
        .expect("hydrate")
        .expect("exists");
    let fp1 = provider_row_fingerprint(&via_aggregate.provider);
    let fp2 = provider_row_fingerprint(&via_aggregate.provider);
    assert_eq!(fp1, fp2, "fingerprint must be deterministic");

    // endpoint 填充差异(get_provider_by_id 会把 endpoints 合回 meta)不得
    // 改变指纹。
    let mut with_endpoints_in_meta = via_aggregate.provider.clone();
    let mut meta = with_endpoints_in_meta.meta.take().unwrap_or_default();
    meta.custom_endpoints.insert(
        "https://e.example".to_string(),
        CustomEndpoint {
            url: "https://e.example".to_string(),
            added_at: Some(1),
            last_used: None,
        },
    );
    with_endpoints_in_meta.meta = Some(meta);
    assert_eq!(
        fp1,
        provider_row_fingerprint(&with_endpoints_in_meta),
        "endpoint hydration differences must not change the content fingerprint"
    );

    // preserve_order 下键插入顺序不同但逻辑相等的对象必须同指纹
    // (旧的朴素序列化对同一实例稳定,骗得过"哈希两次"断言,骗不过这个)。
    let mut ordered_a = via_aggregate.provider.clone();
    ordered_a.settings_config =
        serde_json::from_str(r#"{"alpha": 1, "zeta": {"x": 1, "y": 2}}"#).unwrap();
    let mut ordered_b = via_aggregate.provider.clone();
    ordered_b.settings_config =
        serde_json::from_str(r#"{"zeta": {"y": 2, "x": 1}, "alpha": 1}"#).unwrap();
    assert_eq!(
        provider_row_fingerprint(&ordered_a),
        provider_row_fingerprint(&ordered_b),
        "logically equal objects with different key insertion order must share a fingerprint"
    );

    // 长度前缀:边界粘连的不同内容必须得到不同指纹(碰撞对)。
    let mut collide_a = via_aggregate.provider.clone();
    collide_a.settings_config = json!(["a", "b"]);
    let mut collide_b = via_aggregate.provider.clone();
    collide_b.settings_config = json!(["a\u{0}sb"]);
    assert_ne!(
        provider_row_fingerprint(&collide_a),
        provider_row_fingerprint(&collide_b),
        "canonical encoding must be collision-free across value boundaries"
    );
}

// ---------------------------------------------------------------------------
// T11:并发线性化
// ---------------------------------------------------------------------------

#[test]
fn certify_concurrent_create_single_winner() {
    let database = db();
    let barrier = std::sync::Barrier::new(2);
    let contenders = [
        ("赢家甲", "https://alpha.example"),
        ("赢家乙", "https://beta.example"),
    ];
    let results: Vec<Result<&str, AppError>> = std::thread::scope(|scope| {
        contenders
            .iter()
            .map(|(name, url)| {
                let database = &database;
                let barrier = &barrier;
                scope.spawn(move || {
                    let input = with_endpoints(base_input("race", name), &[(url, Some(1), None)]);
                    let aggregate = NewProviderAggregate::from_input("claude", input).unwrap();
                    barrier.wait();
                    database.create_provider(aggregate).map(|_| *url)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().expect("thread join"))
            .collect()
    });
    let winners: Vec<&str> = results
        .iter()
        .filter_map(|r| r.as_ref().ok().copied())
        .collect();
    assert_eq!(winners.len(), 1, "exactly one concurrent create must win");
    let (row, endpoints) = snapshot(&database, "claude", "race");
    let row = row.expect("winner row");
    let winner_url = winners[0];
    let winner_name = contenders
        .iter()
        .find(|(_, url)| *url == winner_url)
        .map(|(name, _)| *name)
        .expect("winner name");
    assert_eq!(row.0, winner_name, "row must belong entirely to the winner");
    assert_eq!(
        endpoints,
        vec![(winner_url.to_string(), Some(1), None)],
        "endpoints must belong entirely to the same winner"
    );
}

#[test]
fn certify_concurrent_full_updates_do_not_tear() {
    let database = db();
    database
        .create_provider(
            NewProviderAggregate::from_input("claude", base_input("tear", "初始")).unwrap(),
        )
        .expect("create");
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        for suffix in ["一号", "二号"] {
            let database = &database;
            let barrier = &barrier;
            scope.spawn(move || {
                let mut input = base_input("tear", &format!("名-{suffix}"));
                input.website_url = Some(format!("https://site-{suffix}.example"));
                input.notes = Some(format!("注-{suffix}"));
                input.icon = Some(format!("icon-{suffix}"));
                let key = ProviderKey::new("claude", "tear").unwrap();
                let row = ProviderRowUpdate::from_input(&input).unwrap();
                barrier.wait();
                database.update_provider(&key, &row).expect("update");
            });
        }
    });
    let (row, _) = snapshot(&database, "claude", "tear");
    let row = row.expect("row");
    let suffix = row.0.strip_prefix("名-").expect("name written by a writer");
    assert_eq!(
        row.2.as_deref(),
        Some(format!("https://site-{suffix}.example").as_str()),
        "row content must come from a single writer, not interleaved"
    );
    assert_eq!(row.6.as_deref(), Some(format!("注-{suffix}").as_str()));
    assert_eq!(row.7.as_deref(), Some(format!("icon-{suffix}").as_str()));
}

#[test]
fn certify_concurrent_endpoint_interleaving_is_consistent() {
    let database = db();
    database
        .create_provider(
            NewProviderAggregate::from_input(
                "claude",
                with_endpoints(
                    base_input("weave", "交错"),
                    &[("https://c.example", Some(1), None)],
                ),
            )
            .unwrap(),
        )
        .expect("create");
    let key = ProviderKey::new("claude", "weave").unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        {
            let database = &database;
            let key = &key;
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                database
                    .add_provider_endpoint(
                        key,
                        NewEndpoint::new("https://a.example", Some(2), None).unwrap(),
                    )
                    .expect("add a");
                database
                    .touch_provider_endpoint(key, "https://a.example", 7)
                    .expect("touch a");
            });
        }
        {
            let database = &database;
            let key = &key;
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                database
                    .add_provider_endpoint(
                        key,
                        NewEndpoint::new("https://b.example", Some(3), None).unwrap(),
                    )
                    .expect("add b");
                database
                    .remove_provider_endpoint(key, "https://c.example")
                    .expect("remove c");
            });
        }
    });
    let (_, endpoints) = snapshot(&database, "claude", "weave");
    assert_eq!(
        endpoints,
        vec![
            ("https://a.example".to_string(), Some(2), Some(7)),
            ("https://b.example".to_string(), Some(3), None),
        ],
        "interleaved endpoint operations must all land exactly once"
    );
}

// ---------------------------------------------------------------------------
// T10:服务入口认证绑定(syn 级:真实 #[test] 函数且真的触达 ProviderService)
// ---------------------------------------------------------------------------

#[test]
fn certify_service_entry_tests_present() {
    let source = fs::read_to_string(source_root().join("services/provider/mod.rs"))
        .expect("read services/provider/mod.rs");
    let syntax = syn::parse_file(&source).expect("parse services/provider/mod.rs");
    for required in [
        "provider_service_create_owns_initial_endpoints_and_duplicate_is_atomic",
        "provider_service_stale_edit_payload_cannot_overwrite_endpoint_operations",
        "provider_service_db_only_rename_matrix_is_atomic_and_lossless",
    ] {
        let function = find_fn(&syntax.items, required)
            .unwrap_or_else(|| panic!("bound service-entry test '{required}' is missing"));
        assert!(
            function
                .attrs
                .iter()
                .any(|attribute| attribute.path().is_ident("test")),
            "'{required}' must be a #[test] function"
        );
        let mut probe = IdentProbe::default();
        probe.visit_block(&function.block);
        assert!(
            probe.found.contains("ProviderService"),
            "'{required}' must exercise ProviderService (empty stubs cannot pass)"
        );
    }
}
