//! JSON 补丁器（Claude Code `settings.json`、Claude Desktop 的 profile 等）。
//!
//! `serde_json` 开了 `preserve_order`，键保持文件里的顺序。但 `Map::remove` 在
//! `preserve_order` 下等于 `swap_remove`，会把最后一个键挪到被删键的位置，所以这里
//! 删除一律用 `shift_remove`，替换一律原位改值，新增的键追加到所在对象末尾。
//!
//! 重新序列化不保留原始排版：缩进沿用文件原有的（空格或 Tab），换行符和末尾换行
//! 照原样，但 `é` 这类转义会变成原字符、数字可能换写法。所以承诺的是：键、值、
//! 顺序不变；第一次写入可能规范化空白和转义，之后的写入字节稳定。

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use super::{decode_utf8, KeyPath, LivePatch, LiveWriteError};

/// 按谓词清空的作用域：父对象里所有命中谓词的键（关键字段）。
#[derive(Clone)]
pub struct ClearScope {
    pub parent: KeyPath,
    pub is_floor: fn(&str) -> bool,
}

#[derive(Clone, Default)]
pub struct JsonPatch {
    /// 先清空的关键字段。目标值里也有的键不删，留给 `set` 原位改值。
    pub clear: Vec<ClearScope>,
    /// 按路径点名清掉的键（嵌在用户对象里的关键字段）。`set` 里有同一路径时跳过。
    pub remove: Vec<KeyPath>,
    /// 目标值：已存在就原位改值，不存在就追加到父对象末尾（父对象缺失时一并创建）。
    pub set: Vec<(KeyPath, Value)>,
    /// 缺失时才写入。
    pub seed: Vec<(KeyPath, Value)>,
    /// 当前值等于其中之一才删除（独有字段切走、残留清理）。`set` 里有同一路径时跳过。
    pub remove_if: Vec<(KeyPath, Vec<Value>)>,
}

impl JsonPatch {
    pub fn apply_to(&self, path: &Path, doc: &mut Value) -> Result<(), LiveWriteError> {
        let targets: HashSet<&KeyPath> = self.set.iter().map(|(key_path, _)| key_path).collect();

        for scope in &self.clear {
            let Some(map) = object_at_mut(path, doc, &scope.parent.0)? else {
                continue;
            };
            let doomed: Vec<String> = map
                .keys()
                .filter(|key| (scope.is_floor)(key) && !targets.contains(&scope.parent.child(key)))
                .cloned()
                .collect();
            for key in doomed {
                map.shift_remove(&key);
            }
        }

        for key_path in &self.remove {
            if targets.contains(key_path) {
                continue;
            }
            let (parent, key) = split(key_path);
            if let Some(map) = object_at_mut(path, doc, parent)? {
                map.shift_remove(key);
            }
        }

        for (key_path, value) in &self.set {
            let (parent, key) = split(key_path);
            let map = ensure_object_mut(path, doc, parent)?;
            match map.get_mut(key) {
                Some(slot) => *slot = value.clone(),
                None => {
                    map.insert(key.clone(), value.clone());
                }
            }
        }

        for (key_path, value) in &self.seed {
            let (parent, key) = split(key_path);
            let map = ensure_object_mut(path, doc, parent)?;
            if !map.contains_key(key) {
                map.insert(key.clone(), value.clone());
            }
        }

        for (key_path, values) in &self.remove_if {
            if targets.contains(key_path) {
                continue;
            }
            let (parent, key) = split(key_path);
            let Some(map) = object_at_mut(path, doc, parent)? else {
                continue;
            };
            if map.get(key).is_some_and(|current| values.contains(current)) {
                map.shift_remove(key);
            }
        }

        Ok(())
    }
}

/// 文档里 `path` 处的值。
pub fn value_at<'a>(doc: &'a Value, path: &KeyPath) -> Option<&'a Value> {
    path.0
        .iter()
        .try_fold(doc, |current, segment| current.get(segment))
}

impl LivePatch for JsonPatch {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        let (mut doc, style) = parse(path, pre)?;
        self.apply_to(path, &mut doc)?;
        serialize(path, &doc, &style)
    }
}

/// 文件原有的排版习惯，重新序列化时沿用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonStyle {
    indent: Vec<u8>,
    trailing_newline: bool,
    crlf: bool,
    bom: bool,
}

impl Default for JsonStyle {
    fn default() -> Self {
        Self {
            indent: b"  ".to_vec(),
            trailing_newline: false,
            crlf: false,
            bom: false,
        }
    }
}

/// 解析写前内容。文件不存在或只有空白时视为空对象（这是唯一允许的「空底」）；
/// 顶层不是对象时报错，不覆盖。
pub fn parse(path: &Path, pre: Option<&[u8]>) -> Result<(Value, JsonStyle), LiveWriteError> {
    let Some(bytes) = pre else {
        return Ok((Value::Object(Map::new()), JsonStyle::default()));
    };
    let text = decode_utf8(path, bytes)?;
    let (bom, text) = match text.strip_prefix('\u{feff}') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if text.trim().is_empty() {
        return Ok((
            Value::Object(Map::new()),
            JsonStyle {
                bom,
                ..JsonStyle::default()
            },
        ));
    }

    let doc: Value = serde_json::from_str(text).map_err(|err| LiveWriteError::Parse {
        path: path.to_path_buf(),
        line: err.line(),
        column: err.column(),
        message: err.to_string(),
    })?;
    if !doc.is_object() {
        return Err(LiveWriteError::Shape {
            path: path.to_path_buf(),
            key_path: KeyPath::root(),
            expected: "对象",
        });
    }

    let style = JsonStyle {
        indent: detect_indent(text).unwrap_or_else(|| b"  ".to_vec()),
        trailing_newline: text.ends_with('\n'),
        crlf: text.contains("\r\n"),
        bom,
    };
    Ok((doc, style))
}

pub fn serialize(path: &Path, doc: &Value, style: &JsonStyle) -> Result<Vec<u8>, LiveWriteError> {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(&style.indent);
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    doc.serialize(&mut serializer)
        .map_err(|err| LiveWriteError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::other(err),
        })?;
    if style.trailing_newline {
        out.push(b'\n');
    }
    if style.crlf {
        let mut converted = Vec::with_capacity(out.len() + out.len() / 16);
        for byte in out {
            if byte == b'\n' {
                converted.push(b'\r');
            }
            converted.push(byte);
        }
        out = converted;
    }
    if style.bom {
        let mut with_bom = "\u{feff}".as_bytes().to_vec();
        with_bom.extend_from_slice(&out);
        out = with_bom;
    }
    Ok(out)
}

/// 第一行缩进过的行，它的前导空白就是一级缩进（serde 的美化输出和 Claude Code 自己
/// 写的文件都是逐级缩进）。空格和 Tab 混用时不认，退回两个空格。
fn detect_indent(text: &str) -> Option<Vec<u8>> {
    text.lines().skip(1).find_map(|line| {
        let content = line.trim_start();
        if content.is_empty() {
            return None;
        }
        let leading = &line[..line.len() - content.len()];
        let uniform = leading.bytes().all(|b| b == b' ') || leading.bytes().all(|b| b == b'\t');
        (!leading.is_empty() && uniform).then(|| leading.as_bytes().to_vec())
    })
}

fn split(key_path: &KeyPath) -> (&[String], &String) {
    key_path
        .split_last()
        .expect("patch paths must name a key, not the document root")
}

fn shape_error(path: &Path, segments: &[String]) -> LiveWriteError {
    LiveWriteError::Shape {
        path: path.to_path_buf(),
        key_path: KeyPath(segments.to_vec()),
        expected: "对象",
    }
}

/// 沿路径找到对象；中途缺失返回 `None`，中途不是对象报错。
fn object_at_mut<'a>(
    path: &Path,
    doc: &'a mut Value,
    segments: &[String],
) -> Result<Option<&'a mut Map<String, Value>>, LiveWriteError> {
    let mut current = doc;
    for (depth, segment) in segments.iter().enumerate() {
        let map = current
            .as_object_mut()
            .ok_or_else(|| shape_error(path, &segments[..depth]))?;
        match map.get_mut(segment) {
            Some(next) => current = next,
            None => return Ok(None),
        }
    }
    current
        .as_object_mut()
        .map(Some)
        .ok_or_else(|| shape_error(path, segments))
}

/// 沿路径找到对象，缺失的层级追加到所在对象末尾。
fn ensure_object_mut<'a>(
    path: &Path,
    doc: &'a mut Value,
    segments: &[String],
) -> Result<&'a mut Map<String, Value>, LiveWriteError> {
    let mut current = doc;
    for (depth, segment) in segments.iter().enumerate() {
        let map = current
            .as_object_mut()
            .ok_or_else(|| shape_error(path, &segments[..depth]))?;
        current = map
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    current
        .as_object_mut()
        .ok_or_else(|| shape_error(path, segments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::floor;
    use serde_json::json;

    fn apply(patch: &JsonPatch, pre: &str) -> String {
        let out = patch
            .apply(Path::new("settings.json"), Some(pre.as_bytes()))
            .expect("apply");
        String::from_utf8(out).expect("utf8")
    }

    fn claude_patch(env: &[(&str, Value)]) -> JsonPatch {
        JsonPatch {
            clear: vec![
                ClearScope {
                    parent: KeyPath::root(),
                    is_floor: floor::claude_floor_top,
                },
                ClearScope {
                    parent: KeyPath::new(&["env"]),
                    is_floor: floor::claude_floor_env,
                },
            ],
            set: env
                .iter()
                .map(|(key, value)| (KeyPath::new(&["env", key]), value.clone()))
                .collect(),
            ..JsonPatch::default()
        }
    }

    const LIVE: &str = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://a.example",
    "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1",
    "ANTHROPIC_AUTH_TOKEN": "sk-a",
    "AWS_REGION": "us-east-1"
  },
  "model": "a-model",
  "hooks": {
    "Stop": []
  },
  "permissions": {
    "allow": [
      "Bash"
    ]
  }
}"#;

    #[test]
    fn key_fields_are_replaced_in_place_and_the_rest_keeps_its_order() {
        let out = apply(
            &claude_patch(&[
                ("ANTHROPIC_AUTH_TOKEN", json!("sk-b")),
                ("ANTHROPIC_BASE_URL", json!("https://b.example")),
            ]),
            LIVE,
        );
        assert_eq!(
            out,
            r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://b.example",
    "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1",
    "ANTHROPIC_AUTH_TOKEN": "sk-b"
  },
  "hooks": {
    "Stop": []
  },
  "permissions": {
    "allow": [
      "Bash"
    ]
  }
}"#
        );
    }

    #[test]
    fn removing_a_key_does_not_move_the_last_key_into_its_place() {
        let pre = r#"{
  "a": 1,
  "model": "x",
  "b": 2,
  "c": 3
}"#;
        let out = apply(&claude_patch(&[]), pre);
        assert_eq!(out, "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}");
    }

    #[test]
    fn new_keys_go_to_the_end_of_their_parent_and_missing_parents_are_created() {
        let pre = "{\n  \"hooks\": {}\n}";
        let out = apply(
            &claude_patch(&[("ANTHROPIC_BASE_URL", json!("https://b.example"))]),
            pre,
        );
        assert_eq!(
            out,
            "{\n  \"hooks\": {},\n  \"env\": {\n    \"ANTHROPIC_BASE_URL\": \"https://b.example\"\n  }\n}"
        );
    }

    #[test]
    fn remove_drops_named_paths_but_keeps_their_siblings() {
        let patch = JsonPatch {
            remove: vec![
                KeyPath::new(&["model", "name"]),
                KeyPath::new(&["security", "auth", "selectedType"]),
            ],
            ..JsonPatch::default()
        };
        let pre = r#"{"model": {"name": "m", "compressionThreshold": 0.5}, "ui": {}}"#;
        let value: Value = serde_json::from_str(&apply(&patch, pre)).unwrap();
        assert_eq!(
            value,
            json!({"model": {"compressionThreshold": 0.5}, "ui": {}})
        );
    }

    #[test]
    fn seed_writes_only_when_missing() {
        let patch = JsonPatch {
            seed: vec![
                (KeyPath::new(&["allowed"]), json!(["*"])),
                (KeyPath::new(&["chooser"]), json!(true)),
            ],
            ..JsonPatch::default()
        };
        let out = apply(&patch, r#"{"allowed": ["corp.example"]}"#);
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value, json!({"allowed": ["corp.example"], "chooser": true}));
    }

    #[test]
    fn remove_if_only_removes_matching_values_and_yields_to_targets() {
        let patch = JsonPatch {
            set: vec![(KeyPath::new(&["env", "WINDOW"]), json!("983616"))],
            remove_if: vec![
                (KeyPath::new(&["env", "WINDOW"]), vec![json!("983616")]),
                (KeyPath::new(&["env", "ARTIFACT"]), vec![json!("1")]),
                (KeyPath::new(&["env", "BETAS"]), vec![json!("1")]),
            ],
            ..JsonPatch::default()
        };
        let pre = r#"{"env": {"WINDOW": "262144", "ARTIFACT": "1", "BETAS": "0"}}"#;
        let value: Value = serde_json::from_str(&apply(&patch, pre)).unwrap();
        assert_eq!(value, json!({"env": {"WINDOW": "983616", "BETAS": "0"}}));
    }

    #[test]
    fn second_write_is_byte_stable() {
        let patch = claude_patch(&[("ANTHROPIC_AUTH_TOKEN", json!("sk-b"))]);
        let once = apply(&patch, LIVE);
        let twice = apply(&patch, &once);
        assert_eq!(once, twice);
    }

    #[test]
    fn keeps_indent_trailing_newline_and_line_endings() {
        let pre =
            "{\r\n    \"a\": {\r\n        \"model\": 1\r\n    },\r\n    \"model\": \"x\"\r\n}\r\n";
        let out = apply(&claude_patch(&[]), pre);
        assert_eq!(
            out,
            "{\r\n    \"a\": {\r\n        \"model\": 1\r\n    }\r\n}\r\n"
        );

        let tabbed = "{\n\t\"a\": 1\n}\n";
        assert_eq!(apply(&JsonPatch::default(), tabbed), tabbed);
    }

    #[test]
    fn absent_or_blank_files_start_from_an_empty_object() {
        let patch = claude_patch(&[("ANTHROPIC_BASE_URL", json!("u"))]);
        for pre in [None, Some(&b"  \n"[..])] {
            let out = patch.apply(Path::new("s.json"), pre).expect("apply");
            let value: Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(value, json!({"env": {"ANTHROPIC_BASE_URL": "u"}}));
        }
    }

    #[test]
    fn broken_files_are_refused_with_a_location() {
        let err = JsonPatch::default()
            .apply(Path::new("s.json"), Some(b"{\n  \"a\": 1,\n  oops\n}"))
            .expect_err("must refuse");
        match err {
            LiveWriteError::Parse { line, .. } => assert_eq!(line, 3),
            other => panic!("unexpected error: {other:?}"),
        }

        let err = JsonPatch::default()
            .apply(Path::new("s.json"), Some(b"[]"))
            .expect_err("must refuse");
        assert!(matches!(err, LiveWriteError::Shape { .. }), "{err:?}");
    }

    #[test]
    fn a_non_object_parent_is_refused_instead_of_overwritten() {
        let patch = claude_patch(&[("ANTHROPIC_BASE_URL", json!("u"))]);
        let err = patch
            .apply(Path::new("s.json"), Some(br#"{"env": "oops"}"#))
            .expect_err("must refuse");
        match err {
            LiveWriteError::Shape { key_path, .. } => assert_eq!(key_path, KeyPath::new(&["env"])),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
