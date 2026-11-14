//! TOML 配置（Codex、Grok Build 的 `config.toml`）编辑器的共用部分：把配置拆成可以单独
//! 改动的位置，算出打开编辑器之后用户改了哪些全局设置，再按三方比较写进 live。
//!
//! 哪些位置算全局设置由各应用的编辑器决定（关键字段、CC Switch 写的表不算）。

use std::path::Path;

use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::error::AppError;
use crate::live::patch::toml::TomlDocPatch;
use crate::live::patch::LiveWriteError;

use super::claude_editor::{ConflictPolicy, InactiveField};

/// 行（或编辑器内容）里的 TOML 文本。
pub(crate) fn config_text(settings: &Value) -> &str {
    settings.get("config").and_then(Value::as_str).unwrap_or("")
}

/// 解析编辑器里的 TOML。`key`、`app` 用来报错，`what` 说明是哪一份。
pub(crate) fn parse_text(
    text: &str,
    key: &'static str,
    app: &str,
    what: &str,
) -> Result<DocumentMut, AppError> {
    text.parse::<DocumentMut>().map_err(|err| {
        AppError::localized(
            key,
            format!("{app} 配置不是合法的 TOML（{what}）：{err}"),
            format!("The {app} configuration is not valid TOML ({what}): {err}"),
        )
    })
}

/// 行里保存着、但不随切换生效的全局设置（值和显示的不同才列出）。值是可以照抄的 TOML。
pub(crate) fn inactive_fields(
    row_entries: Vec<Entry>,
    display: &DocumentMut,
) -> Vec<InactiveField> {
    row_entries
        .into_iter()
        .filter(|entry| item_at(display, &entry.path).map(render) != Some(render(&entry.item)))
        .map(|entry| {
            let mut fragment = DocumentMut::new();
            insert_at(&mut fragment, &entry.path, entry.item.clone());
            InactiveField {
                path: entry.path,
                value: Value::String(fragment.to_string()),
            }
        })
        .collect()
}

/// 一个可以单独改动的位置和它的内容。
pub(crate) struct Entry {
    pub path: Vec<String>,
    pub item: Item,
}

/// 比较用的规范文本：不看两侧的空白和注释。
pub(crate) fn render(item: &Item) -> String {
    let mut doc = DocumentMut::new();
    let mut item = item.clone();
    match &mut item {
        Item::Value(value) => value.decor_mut().clear(),
        Item::Table(table) => {
            table.decor_mut().clear();
            table.set_implicit(false);
        }
        _ => {}
    }
    doc.insert("v", item);
    doc.to_string().trim().to_string()
}

pub(crate) fn item_at<'a>(doc: &'a DocumentMut, path: &[String]) -> Option<&'a Item> {
    let (last, parents) = path.split_last()?;
    let mut current: &dyn TableLike = doc.as_table();
    for segment in parents {
        current = current.get(segment)?.as_table_like()?;
    }
    current.get(last)
}

/// 编辑器里对全局设置的一处改动。
#[derive(Debug, Clone)]
struct Change {
    path: Vec<String>,
    /// 打开编辑器时的内容；`None` 表示当时没有。
    before: Option<String>,
    /// 保存的内容；`None` 表示删掉。
    after: Option<Item>,
}

/// 一次编辑器保存要写进 live 的全局改动。
#[derive(Debug, Clone)]
pub(crate) struct TomlEdits {
    changes: Vec<Change>,
    on_conflict: ConflictPolicy,
}

impl TomlEdits {
    /// 从打开编辑器时和保存时的全局设置算出改动。
    pub(crate) fn between(base: &[Entry], edited: &[Entry], on_conflict: ConflictPolicy) -> Self {
        Self {
            changes: changes(base, edited),
            on_conflict,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// 改动的位置（`a.b`），按顺序。
    #[cfg(test)]
    pub(crate) fn paths(&self) -> Vec<String> {
        self.changes
            .iter()
            .map(|change| change.path.join("."))
            .collect()
    }

    /// 按三方比较把改动应用到 live 文档上。
    pub(crate) fn apply_to(
        &self,
        path: &Path,
        doc: &mut DocumentMut,
    ) -> Result<(), LiveWriteError> {
        let accepted = self.on_conflict.resolve(path, &self.changes, |change| {
            let current = item_at(doc, &change.path).map(render);
            let after = change.after.as_ref().map(render);
            (current != change.before && current != after).then(|| change.path.join("."))
        })?;
        for change in accepted {
            match &change.after {
                Some(item) => insert_at(doc, &change.path, item.clone()),
                None => remove_at(doc, &change.path),
            }
        }
        Ok(())
    }
}

impl TomlDocPatch for TomlEdits {
    fn apply_to(&self, path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        Self::apply_to(self, path, doc)
    }
}

pub(crate) fn insert_at(doc: &mut DocumentMut, path: &[String], item: Item) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current: &mut dyn TableLike = doc.as_table_mut();
    // 内联表（`model_providers = { … }`）里只能放值：表要转成内联表。
    let mut inline = false;
    for segment in parents {
        if current.get(segment).and_then(Item::as_table_like).is_none() {
            current.insert(segment, Item::Table(Table::new()));
        }
        let child = current.get_mut(segment).expect("just ensured a table");
        inline = matches!(child, Item::Value(_));
        current = child.as_table_like_mut().expect("just ensured a table");
    }
    let item = match item {
        Item::Table(table) if inline => {
            Item::Value(toml_edit::Value::InlineTable(table.into_inline_table()))
        }
        other => other,
    };
    match current.get_mut(last) {
        Some(slot) => {
            let decor = match &*slot {
                Item::Value(value) => Some(value.decor().clone()),
                _ => None,
            };
            *slot = item;
            if let (Some(decor), Item::Value(value)) = (decor, slot) {
                *value.decor_mut() = decor;
            }
        }
        None => {
            current.insert(last, item);
        }
    }
}

pub(crate) fn remove_at(doc: &mut DocumentMut, path: &[String]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current: &mut dyn TableLike = doc.as_table_mut();
    for segment in parents {
        let Some(next) = current.get_mut(segment).and_then(Item::as_table_like_mut) else {
            return;
        };
        current = next;
    }
    current.remove(last);
}

fn changes(base_entries: &[Entry], edited_entries: &[Entry]) -> Vec<Change> {
    let render_of = |entries: &[Entry], path: &[String]| {
        entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| (render(&entry.item), entry.item.clone()))
    };
    let mut paths: Vec<Vec<String>> = base_entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    for entry in edited_entries {
        if !paths.contains(&entry.path) {
            paths.push(entry.path.clone());
        }
    }
    paths
        .into_iter()
        .filter_map(|path| {
            let before = render_of(base_entries, &path);
            let after = render_of(edited_entries, &path);
            if before.as_ref().map(|(text, _)| text) == after.as_ref().map(|(text, _)| text) {
                return None;
            }
            Some(Change {
                path,
                before: before.map(|(text, _)| text),
                after: after.map(|(_, item)| item),
            })
        })
        .collect()
}
