//! `.env` 补丁器（Gemini CLI 的 `~/.gemini/.env`）：按行处理，注释、空行、
//! 认不出的行和其他变量的顺序都原样保留。

use std::collections::HashSet;
use std::path::Path;

use super::{decode_utf8, LivePatch, LiveWriteError};

#[derive(Clone, Default)]
pub struct DotenvPatch {
    /// 命中谓词的变量先全部删掉（关键字段）；`set` 里有的留给它原位改值。
    pub clear: Option<fn(&str) -> bool>,
    /// 目标值：第一处原位改写（保留 `export ` 前缀），重复的其余行删掉；没有就追加。
    /// 值按原样写成 `KEY=value`，和 CC Switch 现有的写法一致。
    pub set: Vec<(String, String)>,
    /// 当前值（去掉引号后）等于其中之一才删除。`set` 里有同名变量时跳过。
    pub remove_if: Vec<(String, Vec<String>)>,
    /// 按名删掉（所有重复的行）。`set` 里有同名变量时跳过。
    pub remove: Vec<String>,
}

#[derive(Debug, Clone)]
struct Line {
    raw: String,
    key: Option<String>,
}

impl LivePatch for DotenvPatch {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        let text = match pre {
            Some(bytes) => decode_utf8(path, bytes)?,
            None => "",
        };
        let crlf = text.contains("\r\n");
        let trailing_newline = pre.is_none() || text.is_empty() || text.ends_with('\n');
        let mut lines: Vec<Line> = if text.is_empty() {
            Vec::new()
        } else {
            text.strip_suffix('\n')
                .unwrap_or(text)
                .split('\n')
                .map(|raw| {
                    let raw = raw.strip_suffix('\r').unwrap_or(raw).to_string();
                    let key = parse_key(&raw).map(str::to_string);
                    Line { raw, key }
                })
                .collect()
        };

        let targets: HashSet<&str> = self.set.iter().map(|(key, _)| key.as_str()).collect();

        if let Some(is_floor) = self.clear {
            lines.retain(|line| {
                line.key
                    .as_deref()
                    .is_none_or(|key| !is_floor(key) || targets.contains(key))
            });
        }

        for (key, value) in &self.set {
            let mut seen = false;
            lines.retain_mut(|line| {
                if line.key.as_deref() != Some(key.as_str()) {
                    return true;
                }
                if seen {
                    return false;
                }
                seen = true;
                let export = if line.raw.trim_start().starts_with("export ") {
                    "export "
                } else {
                    ""
                };
                line.raw = format!("{export}{key}={value}");
                true
            });
            if !seen {
                lines.push(Line {
                    raw: format!("{key}={value}"),
                    key: Some(key.clone()),
                });
            }
        }

        lines.retain(|line| {
            line.key.as_deref().is_none_or(|key| {
                targets.contains(key) || !self.remove.iter().any(|doomed| doomed == key)
            })
        });

        for (key, values) in &self.remove_if {
            if targets.contains(key.as_str()) {
                continue;
            }
            lines.retain(|line| {
                line.key.as_deref() != Some(key.as_str())
                    || !values
                        .iter()
                        .any(|value| unquote(value_of(&line.raw)) == value)
            });
        }

        let separator = if crlf { "\r\n" } else { "\n" };
        let mut out = lines
            .iter()
            .map(|line| line.raw.as_str())
            .collect::<Vec<_>>()
            .join(separator);
        if trailing_newline && !lines.is_empty() {
            out.push_str(separator);
        }
        Ok(out.into_bytes())
    }
}

/// 文件里的变量和值（值原样，不去引号），按第一次出现的位置排；重复定义时取最后一个
/// 值（和 dotenv 解析的结果一致）。
pub fn entries(text: &str) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let Some(key) = parse_key(raw) else {
            continue;
        };
        let value = value_of(raw).to_string();
        match entries.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = value,
            None => entries.push((key.to_string(), value)),
        }
    }
    entries
}

/// `KEY=...` 或 `export KEY=...` 里的变量名；认不出的行返回 `None`，原样保留。
fn parse_key(raw: &str) -> Option<&str> {
    let line = raw.trim_start();
    if line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, _) = line.split_once('=')?;
    let key = key.trim();
    (!key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(key)
}

fn value_of(raw: &str) -> &str {
    raw.split_once('=').map_or("", |(_, value)| value.trim())
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::floor;

    fn apply(patch: &DotenvPatch, pre: Option<&str>) -> String {
        let out = patch
            .apply(Path::new(".env"), pre.map(str::as_bytes))
            .expect("apply");
        String::from_utf8(out).expect("utf8")
    }

    #[test]
    fn key_fields_change_and_other_lines_stay_in_order() {
        let patch = DotenvPatch {
            clear: Some(floor::gemini_floor_env),
            set: vec![("GEMINI_API_KEY".into(), "key-b".into())],
            ..DotenvPatch::default()
        };
        let pre = "# sandbox\nGEMINI_SANDBOX=docker\nexport GEMINI_API_KEY=\"key-a\"\nGOOGLE_GEMINI_BASE_URL=https://a.example\nDEBUG=1\nGEMINI_API_KEY=dup\n";
        assert_eq!(
            apply(&patch, Some(pre)),
            "# sandbox\nGEMINI_SANDBOX=docker\nexport GEMINI_API_KEY=key-b\nDEBUG=1\n"
        );
    }

    #[test]
    fn missing_keys_are_appended_and_crlf_is_kept() {
        let patch = DotenvPatch {
            set: vec![("GEMINI_MODEL".into(), "m".into())],
            ..DotenvPatch::default()
        };
        assert_eq!(
            apply(&patch, Some("DEBUG=1\r\n")),
            "DEBUG=1\r\nGEMINI_MODEL=m\r\n"
        );
        assert_eq!(apply(&patch, Some("DEBUG=1")), "DEBUG=1\nGEMINI_MODEL=m");
        assert_eq!(apply(&patch, None), "GEMINI_MODEL=m\n");
    }

    #[test]
    fn remove_drops_every_line_of_the_key_and_entries_take_the_last_value() {
        let patch = DotenvPatch {
            remove: vec!["X".into()],
            ..DotenvPatch::default()
        };
        assert_eq!(apply(&patch, Some("X=1\nY=2\nexport X=3\n")), "Y=2\n");
        assert_eq!(
            entries("# c\nX=1\nY = 2\nexport X=\"3\"\n"),
            vec![
                ("X".to_string(), "\"3\"".to_string()),
                ("Y".to_string(), "2".to_string())
            ]
        );
    }

    #[test]
    fn remove_if_compares_unquoted_values() {
        let patch = DotenvPatch {
            remove_if: vec![("X".into(), vec!["1".into()])],
            ..DotenvPatch::default()
        };
        assert_eq!(apply(&patch, Some("X=\"1\"\nY=2\n")), "Y=2\n");
        assert_eq!(apply(&patch, Some("X=0\n")), "X=0\n");
    }
}
