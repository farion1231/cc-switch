//! Grok Build 的投影：供应商行 → `config.toml` 的 `models.default` 和 CC Switch 写的那张
//! `[model."<名称>"]` 整表。
//!
//! 整表指表里的所有键（`model`、`base_url`、`api_key` / `env_key`，以及
//! `reasoning_summary`、`extra_headers` 等）：只换其中几个键会留下上一家的 `env_key`、
//! 模型名，同一张表换供应商后可能继续用旧凭据。用户自己的其他 `[model.*]` 表不碰。
//!
//! 切走时按写入记录删表，不按 live 里的 `models.default` 找：Grok 的 `/settings` 会把它
//! 改成内置模型，按它找就会漏删上一家的表（见 `mode::state::Written`）。
//!
//! Grok 没有供应商独有字段：兼容选项都在模型表里，随整表切换。

use std::path::Path;

use serde_json::{json, Value};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, TableLike};

use crate::error::AppError;
use crate::live::patch::toml::TomlDocPatch;
use crate::live::patch::{KeyPath, LiveWriteError};

/// 代理契约写进模型表的接口类型（本地代理只提供 Responses）。
pub const PROXY_API_BACKEND: &str = "responses";

/// 一个供应商在 `config.toml` 里拥有的东西。
#[derive(Debug, Clone)]
pub struct GrokProjection {
    /// `models.default` 指向的表名和整张表；官方卡没有（Grok 回落到内置模型和自带的
    /// xAI 登录）。
    pub table: Option<(String, Table)>,
}

impl GrokProjection {
    /// 从供应商行取出要写的模型表。`official` 由调用方按分类判定。
    ///
    /// 行里的 `models.default` 指不到表时（旧版回填存进了用户在 Grok 里选的内置模型），
    /// 取行里唯一的那张 `[model.*]` 表；有多张时报错，让用户在编辑器里选。
    pub fn of(settings: &Value, official: bool) -> Result<Self, AppError> {
        if official {
            return Ok(Self { table: None });
        }
        let text = settings
            .get("config")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::localized(
                    "provider.grokbuild.config.missing",
                    "Grok Build 配置缺少 config 字段",
                    "Grok Build configuration is missing the config field",
                )
            })?;
        let doc = text.parse::<DocumentMut>().map_err(|error| {
            AppError::localized(
                "provider.grokbuild.config.invalid_toml",
                format!("Grok Build config.toml 格式错误: {error}"),
                format!("Invalid Grok Build config.toml: {error}"),
            )
        })?;
        let name = resolve_table_name(&doc)?;
        let table = doc
            .get("model")
            .and_then(Item::as_table_like)
            .and_then(|models| models.get(&name))
            .and_then(fresh_table)
            .expect("resolve_table_name 只返回存在的表");

        // 形状校验沿用现有规则（model、base_url、name、api_key / env_key、api_backend、
        // context_window），对象是归一化后的这一张表。
        crate::grok_config::validate_config_toml(&normalized_text(&name, &table))?;
        Ok(Self {
            table: Some((name, table)),
        })
    }

    pub fn table_name(&self) -> Option<&str> {
        self.table.as_ref().map(|(name, _)| name.as_str())
    }

    /// 代理契约：路由供应商的表，地址、Key、接口类型换成本地代理的。`env_key` 原样保留：
    /// Grok 优先用表里的 `api_key`。官方卡没有模型表，不能作为路由（xAI 登录不经本地
    /// 代理）。
    pub fn proxy_contract(
        route: &Self,
        proxy_base_url: &str,
        placeholder: &str,
    ) -> Result<Self, AppError> {
        let (name, table) = route.table.as_ref().ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.proxy.official",
                "Grok Build 官方账号不能经本地路由使用",
                "The official Grok Build account cannot be used through local routing",
            )
        })?;
        let mut table = table.clone();
        table.insert("base_url", toml_edit::value(proxy_base_url));
        table.insert("api_key", toml_edit::value(placeholder));
        table.insert("api_backend", toml_edit::value(PROXY_API_BACKEND));
        Ok(Self {
            table: Some((name.clone(), table)),
        })
    }

    /// 摘要用的规范形式。
    pub fn to_value(&self) -> Value {
        match &self.table {
            Some((name, table)) => json!({"table": name, "toml": normalized_text(name, table)}),
            None => Value::Null,
        }
    }

    /// 写入记录里的表名。
    pub fn written_tables(&self) -> Vec<String> {
        self.table_name().map(str::to_string).into_iter().collect()
    }
}

/// 行里要写的表名：`models.default` 指得到表就用它，否则取唯一的那张表。
fn resolve_table_name(doc: &DocumentMut) -> Result<String, AppError> {
    let tables: Vec<String> = doc
        .get("model")
        .and_then(Item::as_table_like)
        .map(|models| {
            models
                .iter()
                .filter(|(_, item)| item.is_table_like())
                .map(|(name, _)| name.to_string())
                .collect()
        })
        .unwrap_or_default();
    let default = doc
        .get("models")
        .and_then(|models| models.get("default"))
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if let Some(default) = default {
        if tables.iter().any(|name| name == default) {
            return Ok(default.to_string());
        }
    }
    match tables.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(AppError::localized(
            "provider.grokbuild.model.missing",
            "Grok Build 配置缺少 [model.<name>]",
            "Grok Build configuration is missing [model.<name>]",
        )),
        _ => Err(AppError::localized(
            "provider.grokbuild.default_model.ambiguous",
            format!(
                "Grok Build 配置的 models.default 指不到模型表，而行里有多张表（{}），请在编辑器里把 models.default 设成要用的那张",
                tables.join("、")
            ),
            format!(
                "models.default in this Grok Build configuration does not name a model table and there are several ({}); set models.default to the one to use in the editor",
                tables.join(", ")
            ),
        )),
    }
}

/// `[models] default = <name>` 加这一张表，给校验和摘要用。
fn normalized_text(name: &str, table: &Table) -> String {
    let mut doc = DocumentMut::new();
    let mut models = Table::new();
    models.insert("default", toml_edit::value(name));
    doc.insert("models", Item::Table(models));
    let mut model = Table::new();
    model.set_implicit(true);
    model.insert(name, Item::Table(table.clone()));
    doc.insert("model", Item::Table(model));
    doc.to_string()
}

/// 把行里的表（标准表或内联表）复制成一张不带原文档排版和位置的新表。
///
/// 位置号是按原文档算的，带进 live 会和 live 自己的表按号交错排序；新建的表没有位置，
/// 输出时跟在前一张表后面。
fn fresh_table(item: &Item) -> Option<Table> {
    let source = item.as_table_like()?;
    Some(copy_table(source))
}

fn copy_table(source: &dyn TableLike) -> Table {
    let mut table = Table::new();
    for (key, item) in source.iter() {
        let copied = match item {
            Item::Value(value) => {
                let mut value = value.clone();
                value.decor_mut().clear();
                Item::Value(value)
            }
            Item::Table(sub) => Item::Table(copy_table(sub)),
            Item::ArrayOfTables(array) => {
                let mut copy = ArrayOfTables::new();
                for sub in array.iter() {
                    copy.push(copy_table(sub));
                }
                Item::ArrayOfTables(copy)
            }
            Item::None => continue,
        };
        table.insert(key, copied);
    }
    table
}

/// 补丁：`models.default` 换成目标的表名（官方卡删掉），按写入记录删上一家的表，写目标
/// 的整张表。
#[derive(Debug, Clone, Default)]
pub struct GrokConfigPatch {
    pub target: Option<(String, Table)>,
    /// 上次写入记录里的表。目标自己的表不删，由整表替换覆盖。
    pub retired: Vec<String>,
    /// 这个值作为 `api_key` 的表都是 CC Switch 的代理契约留下的，一律删（目标自己的
    /// 表除外）。
    pub placeholder: Option<String>,
}

impl GrokConfigPatch {
    pub fn direct(target: &GrokProjection, retired: Vec<String>, placeholder: &str) -> Self {
        Self {
            target: target.table.clone(),
            retired,
            placeholder: Some(placeholder.to_string()),
        }
    }

    pub fn apply_to(&self, path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        let target_name = self.target.as_ref().map(|(name, _)| name.as_str());
        let root = doc.as_table_mut();

        match target_name {
            Some(name) => {
                let models = table_mut(path, root, "models", true)?.expect("created");
                match models.get_mut("default") {
                    Some(Item::Value(slot)) => {
                        let decor = slot.decor().clone();
                        *slot = name.into();
                        *slot.decor_mut() = decor;
                    }
                    _ => {
                        models.insert("default", toml_edit::value(name));
                    }
                }
            }
            None => {
                if let Some(models) = table_mut(path, root, "models", false)? {
                    models.remove("default");
                }
            }
        }

        if let Some(tables) = table_mut(path, root, "model", false)? {
            let placeholder = self.placeholder.as_deref();
            let doomed: Vec<String> = tables
                .iter()
                .filter(|(name, item)| {
                    Some(*name) != target_name
                        && (self.retired.iter().any(|retired| retired == name)
                            || placeholder.is_some_and(|placeholder| {
                                item.as_table_like()
                                    .and_then(|table| table.get("api_key"))
                                    .and_then(Item::as_str)
                                    == Some(placeholder)
                            }))
                })
                .map(|(name, _)| name.to_string())
                .collect();
            for name in doomed {
                tables.remove(&name);
            }
        }

        if let Some((name, table)) = &self.target {
            let tables = table_mut(path, root, "model", true)?.expect("created");
            let mut table = table.clone();
            match tables.get_mut(name) {
                Some(slot) => {
                    if let Item::Table(old) = slot {
                        *table.decor_mut() = old.decor().clone();
                        if let Some(position) = old.position() {
                            table.set_position(position);
                        }
                    }
                    *slot = Item::Table(table);
                }
                None => {
                    tables.insert(name, Item::Table(table));
                }
            }
        }

        // 删空了的 `[models]`、`[model]` 一并去掉：Grok 靠「没有这两张表」认出官方状态。
        for key in ["model", "models"] {
            if root
                .get(key)
                .and_then(Item::as_table_like)
                .is_some_and(|table| table.is_empty())
            {
                root.remove(key);
            }
        }
        Ok(())
    }
}

impl TomlDocPatch for GrokConfigPatch {
    fn apply_to(&self, path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        Self::apply_to(self, path, doc)
    }
}

/// 根下的表；`create` 时缺失就建（`model` 建成隐式表，只输出 `[model."<名称>"]`）。
fn table_mut<'a>(
    path: &Path,
    root: &'a mut Table,
    key: &str,
    create: bool,
) -> Result<Option<&'a mut dyn TableLike>, LiveWriteError> {
    if !root.contains_key(key) {
        if !create {
            return Ok(None);
        }
        let mut table = Table::new();
        table.set_implicit(key == "model");
        root.insert(key, Item::Table(table));
    }
    root.get_mut(key)
        .and_then(Item::as_table_like_mut)
        .map(Some)
        .ok_or_else(|| LiveWriteError::Shape {
            path: path.to_path_buf(),
            key_path: KeyPath::new(&[key]),
            expected: "表",
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::patch::LivePatch;

    const PLACEHOLDER: &str = "PROXY_MANAGED";

    fn row(config: &str) -> Value {
        json!({ "config": config })
    }

    fn apply(patch: &GrokConfigPatch, pre: &str) -> String {
        String::from_utf8(
            patch
                .apply(Path::new("config.toml"), Some(pre.as_bytes()))
                .unwrap(),
        )
        .unwrap()
    }

    const ROW_A: &str = r#"[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "a-model"
name = "A"
base_url = "https://a.example/v1"
api_key = "key-a"
api_backend = "responses"
context_window = 500000
reasoning_summary = "none"
"#;

    const ROW_B: &str = r#"[models]
default = "b"

[model.b]
model = "b-model"
name = "B"
base_url = "https://b.example/v1"
env_key = "B_KEY"
api_backend = "chat_completions"
context_window = 200000
"#;

    #[test]
    fn a_default_that_points_nowhere_falls_back_to_the_only_table() {
        let polluted = ROW_A.replace("default = \"grok-4.5\"", "default = \"grok-4.6\"");
        let projection = GrokProjection::of(&row(&polluted), false).unwrap();
        assert_eq!(projection.table_name(), Some("grok-4.5"));

        let two = format!("{polluted}\n[model.other]\nmodel = \"x\"\n");
        let err = GrokProjection::of(&row(&two), false).unwrap_err();
        assert!(err.to_string().contains("grok-4.5"), "{err}");
    }

    #[test]
    fn switching_replaces_the_whole_table_and_keeps_user_tables() {
        let live = format!(
            "# mine\n[ui]\ntheme = \"dark\"\n\n{ROW_A}\n[model.mine]\nmodel = \"m\"\n\n[mcp_servers.fs]\ncommand = \"fs\"\n"
        );
        let b = GrokProjection::of(&row(ROW_B), false).unwrap();
        let out = apply(
            &GrokConfigPatch::direct(&b, vec!["grok-4.5".into()], PLACEHOLDER),
            &live,
        );
        let doc: DocumentMut = out.parse().unwrap();
        assert_eq!(doc["models"]["default"].as_str(), Some("b"));
        assert!(doc["model"].get("grok-4.5").is_none(), "{out}");
        assert_eq!(doc["model"]["mine"]["model"].as_str(), Some("m"));
        assert_eq!(doc["model"]["b"]["env_key"].as_str(), Some("B_KEY"));
        assert!(out.starts_with("# mine\n[ui]\ntheme = \"dark\"\n"), "{out}");
        assert!(out.contains("[mcp_servers.fs]"), "{out}");
    }

    #[test]
    fn the_client_changing_the_default_does_not_hide_the_old_table() {
        // 用户在 Grok 里把默认模型改成内置的 grok-4.6：按写入记录照样删掉 grok-4.5。
        let live = ROW_A.replace("default = \"grok-4.5\"", "default = \"grok-4.6\"");
        let official = GrokProjection::of(&row(""), true).unwrap();
        let out = apply(
            &GrokConfigPatch::direct(&official, vec!["grok-4.5".into()], PLACEHOLDER),
            &live,
        );
        assert_eq!(out, "");
    }

    #[test]
    fn a_renamed_table_replaces_the_one_written_before() {
        let live = ROW_A.to_string();
        let renamed = ROW_A.replace("grok-4.5", "grok-4.6");
        let target = GrokProjection::of(&row(&renamed), false).unwrap();
        let out = apply(
            &GrokConfigPatch::direct(&target, vec!["grok-4.5".into()], PLACEHOLDER),
            &live,
        );
        let doc: DocumentMut = out.parse().unwrap();
        let names: Vec<&str> = doc["model"]
            .as_table_like()
            .unwrap()
            .iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, vec!["grok-4.6"]);
        assert_eq!(doc["models"]["default"].as_str(), Some("grok-4.6"));
    }

    #[test]
    fn stale_proxy_tables_are_removed_and_user_keys_in_models_stay() {
        let live = r#"[models]
default = "grok-4.5"
web_search = "grok-4.6"

[model."grok-4.5"]
model = "a"
base_url = "http://127.0.0.1:15721/grokbuild/v1"
api_key = "PROXY_MANAGED"
"#;
        let official = GrokProjection::of(&row(""), true).unwrap();
        let out = apply(
            &GrokConfigPatch::direct(&official, Vec::new(), PLACEHOLDER),
            live,
        );
        assert_eq!(out, "[models]\nweb_search = \"grok-4.6\"\n");
    }

    #[test]
    fn proxy_contract_keeps_env_key_and_points_at_the_proxy() {
        let b = GrokProjection::of(&row(ROW_B), false).unwrap();
        let contract =
            GrokProjection::proxy_contract(&b, "http://127.0.0.1:15721/grokbuild/v1", PLACEHOLDER)
                .unwrap();
        let (name, table) = contract.table.unwrap();
        assert_eq!(name, "b");
        assert_eq!(table["api_key"].as_str(), Some(PLACEHOLDER));
        assert_eq!(table["env_key"].as_str(), Some("B_KEY"));
        assert_eq!(table["api_backend"].as_str(), Some("responses"));
    }

    #[test]
    fn sub_tables_follow_their_parent() {
        let with_headers = format!("{ROW_B}\n[model.b.extra_headers]\nX-Team = \"t\"\n");
        let target = GrokProjection::of(&row(&with_headers), false).unwrap();
        let live = "[a]\nx = 1\n\n[b]\ny = 2\n\n[c]\nz = 3\n";
        let out = apply(
            &GrokConfigPatch::direct(&target, Vec::new(), PLACEHOLDER),
            live,
        );
        let headers = out.find("[model.b.extra_headers]").unwrap();
        let table = out.find("[model.b]").unwrap();
        assert!(out.starts_with(live), "{out}");
        assert!(table < headers, "{out}");
    }
}
