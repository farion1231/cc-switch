# 阶段一：按模型上下文 catalog 生成（原型）实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 让 CodexPlusPlus 应用 profile 时，根据 `model_list` 后缀语法（如 `deepseek-v4-pro[1M]`）自动生成 codex 原生 `model_catalog_json` 文件并写入 config.toml 指针，使 codex 客户端按模型识别真实上下文窗口。

**架构：** 新增纯函数模块 `model_suffix.rs`（后缀解析 + catalog JSON 构建，无副作用、易测）；在 `relay_config.rs` 新增一个可选步骤 `apply_model_catalog_to_config`，接入现有 3 个 apply 入口（`apply_context_limits_to_config` 之后、落盘之前），靠后缀 opt-in，无后缀则 no-op，不破坏现有 per-profile 单值行为。

**技术栈：** Rust + toml_edit + serde_json + tempfile（测试）。不引入新依赖（regex 不在依赖里，后缀解析手写）。

**设计依据：** `docs/specs/2026-06-23-model-catalog-prototype-design.md`、`docs/research/01-调研结果.md`、issue #1171 / #931。

**验证锚点：** `codex debug models`（不带 `--bundled` 时读取 live config 含 `model_catalog_json`；可用 `-c key=value` 内联覆盖不污染 `~/.codex/config.toml`）输出模型 catalog JSON，其中 `context_window` 字段即观测目标。

---

## 文件结构

- 创建：`crates/codex-plus-core/src/model_suffix.rs`
  - 职责：后缀语法解析（`slug[1M]` → `(slug, window)`）、收集 profile 全部模型条目、构建 catalog JSON 字符串。纯函数，无 IO，无对 relay_config 私有函数的依赖。
- 创建：`crates/codex-plus-core/examples/generate_model_catalog.rs`
  - 职责：命令行小工具，调用 `model_suffix` 公开 API 生成 catalog JSON，供手工验证（B 对拍）使用。
- 修改：`crates/codex-plus-core/src/lib.rs`
  - 职责：注册 `pub mod model_suffix;`。
- 修改：`crates/codex-plus-core/src/relay_config.rs`
  - 职责：新增 `apply_model_catalog_to_config`（访问本模块私有 toml 工具函数）+ `sanitize_catalog_filename`；接入 3 个 apply 入口。
- 测试：`crates/codex-plus-core/tests/model_suffix.rs`（新）
- 测试：`crates/codex-plus-core/tests/relay_config.rs`（加用例）

**为何 `apply_model_catalog_to_config` 放在 `relay_config.rs` 而非新模块：** 它需要 `parse_toml_document` / `normalize_optional_toml` / `parse_optional_positive_u64`（私有）和 `root_key_string`（pub）。放在同模块可访问私有函数，避免为单个函数提升 3 个私有工具为 pub(crate)，改动最小。纯逻辑（解析/构建）拆到 `model_suffix.rs` 独立可测。

---

## 任务 1：后缀解析器（model_suffix 模块基础）

**文件：**
- 创建：`crates/codex-plus-core/src/model_suffix.rs`
- 修改：`crates/codex-plus-core/src/lib.rs`（注册模块）
- 测试：`crates/codex-plus-core/tests/model_suffix.rs`

- [ ] **步骤 1：编写失败的测试**

创建 `crates/codex-plus-core/tests/model_suffix.rs`：

```rust
use codex_plus_core::model_suffix::parse_model_suffix;

#[test]
fn parse_suffix_extracts_k_and_m_units() {
    assert_eq!(
        parse_model_suffix("deepseek-v4-pro[1M]"),
        ("deepseek-v4-pro".to_string(), Some(1_000_000))
    );
    assert_eq!(
        parse_model_suffix("claude-sonnet-4[200K]"),
        ("claude-sonnet-4".to_string(), Some(200_000))
    );
    assert_eq!(
        parse_model_suffix("gpt-5.5[512k]"),
        ("gpt-5.5".to_string(), Some(512_000))
    );
    assert_eq!(
        parse_model_suffix("gpt-5.5[1000000]"),
        ("gpt-5.5".to_string(), Some(1_000_000))
    );
}

#[test]
fn parse_suffix_returns_none_without_bracket() {
    assert_eq!(
        parse_model_suffix("gpt-5.5"),
        ("gpt-5.5".to_string(), None)
    );
    assert_eq!(
        parse_model_suffix("  qwen3-coder  "),
        ("qwen3-coder".to_string(), None)
    );
}

#[test]
fn parse_suffix_keeps_original_slug_when_bracket_invalid() {
    // 括号内非合法窗口 token 时，整串（含括号）作为 slug，window=None
    let (slug, window) = parse_model_suffix("foo[bar]");
    assert_eq!(slug, "foo[bar]");
    assert_eq!(window, None);

    // 括号未闭合：不剥离
    let (slug2, window2) = parse_model_suffix("foo[1M");
    assert_eq!(slug2, "foo[1M");
    assert_eq!(window2, None);
}

#[test]
fn parse_suffix_rejects_zero_and_negative() {
    assert_eq!(
        parse_model_suffix("foo[0K]"),
        ("foo[0K]".to_string(), None)
    );
}
```

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test -p codex-plus-core --test model_suffix 2>&1 | tail -20`
预期：编译失败，报错 `unresolved module model_suffix` 或 `cannot find function parse_model_suffix`。

- [ ] **步骤 3：编写最少实现代码**

在 `crates/codex-plus-core/src/lib.rs` 找到模块声明区（`pub mod model_catalog;` 附近，约第 16 行），新增一行：

```rust
pub mod model_suffix;
```

创建 `crates/codex-plus-core/src/model_suffix.rs`：

```rust
//! model_list 后缀语法解析与 catalog JSON 构建。
//!
//! 后缀语法：`deepseek-v4-pro[1M]` 表示 slug=deepseek-v4-pro、context_window=1000000。
//! 单位 K/k=1000、M/m=1000000；纯数字也接受。后缀在生成 catalog 时剥离。

use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    pub slug: String,
    pub display_name: String,
    /// 来自后缀的窗口值；None 表示该条目无后缀（回落顶层默认）。
    pub suffix_window: Option<u64>,
}

/// 解析单个模型条目的后缀，返回 (slug, 可选窗口)。
/// 括号内非合法窗口 token 时，整串作为 slug 且 window=None（不剥离括号）。
pub fn parse_model_suffix(raw: &str) -> (String, Option<u64>) {
    let raw = raw.trim();
    if let Some(close) = raw.rfind(']') {
        // 仅当 ] 是最后一个字符时才视为后缀
        if close == raw.len() - 1 {
            if let Some(open) = raw[..close].rfind('[') {
                let inner = raw[open + 1..close].trim();
                let slug = raw[..open].trim();
                if !slug.is_empty() {
                    if let Some(window) = parse_window_token(inner) {
                        return (slug.to_string(), Some(window));
                    }
                }
            }
        }
    }
    (raw.to_string(), None)
}

/// 解析括号内的窗口 token，如 "1M" / "200K" / "1000000"。非法或 0 返回 None。
fn parse_window_token(token: &str) -> Option<u64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let (num_part, multiplier) = match token.chars().last() {
        Some('K' | 'k') => (&token[..token.len() - 1], 1_000u64),
        Some('M' | 'm') => (&token[..token.len() - 1], 1_000_000u64),
        Some(_) => (token, 1u64),
        None => return None,
    };
    num_part
        .trim()
        .parse::<u64>()
        .ok()
        .map(|value| value * multiplier)
        .filter(|value| *value > 0)
}
```

- [ ] **步骤 4：运行测试验证通过**

运行：`cargo test -p codex-plus-core --test model_suffix 2>&1 | tail -20`
预期：4 个测试 PASS。

- [ ] **步骤 5：Commit**

```bash
git add crates/codex-plus-core/src/model_suffix.rs crates/codex-plus-core/src/lib.rs crates/codex-plus-core/tests/model_suffix.rs
git commit -m "feat(model_suffix): 新增 model_list 后缀解析器

解析 deepseek-v4-pro[1M] 这类后缀语法，剥离后缀得到 slug 与窗口值。
纯函数无副作用，为按模型 catalog 生成做准备。"
```

---

## 任务 2：条目收集与 catalog JSON 构建

**文件：**
- 修改：`crates/codex-plus-core/src/model_suffix.rs`（追加函数）
- 测试：`crates/codex-plus-core/tests/model_suffix.rs`（追加用例）

- [ ] **步骤 1：编写失败的测试**

在 `crates/codex-plus-core/tests/model_suffix.rs` 末尾追加：

```rust
use codex_plus_core::model_suffix::{collect_catalog_entries, build_model_catalog_json};

#[test]
fn collect_entries_includes_current_model_and_strips_suffix() {
    let entries = collect_catalog_entries("deepseek-v4-pro[1M]\nqwen3-coder", "deepseek-v4-pro");
    // 当前 model 与列表去重后共 2 条
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].slug, "deepseek-v4-pro");
    assert_eq!(entries[0].suffix_window, Some(1_000_000));
    assert_eq!(entries[1].slug, "qwen3-coder");
    assert_eq!(entries[1].suffix_window, None);
}

#[test]
fn collect_entries_deduplicates() {
    let entries = collect_catalog_entries("qwen3-coder\nqwen3-coder", "qwen3-coder");
    assert_eq!(entries.len(), 1);
}

#[test]
fn build_catalog_json_writes_context_window_and_strips_suffix() {
    let entries = collect_catalog_entries("deepseek-v4-pro[1M]\nclaude-sonnet-4[200K]", "");
    let catalog = build_model_catalog_json(&entries, None);
    assert!(catalog.contains(r#""slug": "deepseek-v4-pro""#));
    assert!(catalog.contains(r#""context_window": 1000000"#));
    assert!(catalog.contains(r#""max_context_window": 1000000"#));
    assert!(catalog.contains(r#""slug": "claude-sonnet-4""#));
    assert!(catalog.contains(r#""context_window": 200000"#));
    // 后缀不得进入 catalog
    assert!(!catalog.contains("[1M]"));
    assert!(!catalog.contains("[200K]"));
    // auto_compact 留 null（codex 按比例算）
    assert!(catalog.contains(r#""auto_compact_token_limit": null"#));
}

#[test]
fn build_catalog_json_uses_fallback_for_no_suffix_entries() {
    let entries = collect_catalog_entries("qwen3-coder", "");
    let catalog = build_model_catalog_json(&entries, Some(272_000));
    assert!(catalog.contains(r#""slug": "qwen3-coder""#));
    assert!(catalog.contains(r#""context_window": 272000"#));
}
```

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test -p codex-plus-core --test model_suffix 2>&1 | tail -20`
预期：编译失败，报错 `cannot find function collect_catalog_entries` / `build_model_catalog_json`。

- [ ] **步骤 3：编写最少实现代码**

在 `crates/codex-plus-core/src/model_suffix.rs` 末尾追加：

```rust
/// 收集 profile 的全部模型条目（当前 model + model_list），去重并解析后缀。
/// 返回顺序：当前 model 在前。用于生成 catalog，包含全部模型以避免
/// #1064 单模型副作用（catalog 只剩当前 model）。
pub fn collect_catalog_entries(model_list: &str, current_model: &str) -> Vec<ModelCatalogEntry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for raw in std::iter::once(current_model)
        .chain(model_list.split(['\r', '\n', ',']))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let (slug, suffix_window) = parse_model_suffix(raw);
        if slug.is_empty() || !seen.insert(slug.clone()) {
            continue;
        }
        entries.push(ModelCatalogEntry {
            display_name: slug.clone(),
            slug,
            suffix_window,
        });
    }
    entries
}

/// 构建 codex model_catalog_json 内容。条目字段对齐 cc-switch 覆盖集与 codex
/// 内置目录必要字段（见 docs/research/01-调研结果.md 第五节）。
/// 无后缀条目用 fallback_window；fallback 也无时回落 272000（codex 默认）。
/// auto_compact_token_limit 留 null：codex 内置模型即 null（按比例算，调研第六节）。
pub fn build_model_catalog_json(
    entries: &[ModelCatalogEntry],
    fallback_window: Option<u64>,
) -> String {
    let models: Vec<Value> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let context_window = entry
                .suffix_window
                .or(fallback_window)
                .unwrap_or(272_000);
            json!({
                "slug": entry.slug,
                "display_name": entry.display_name,
                "description": entry.display_name,
                "context_window": context_window,
                "max_context_window": context_window,
                "auto_compact_token_limit": Value::Null,
                "priority": 1000 + index,
                "visibility": "list",
                "supported_in_api": true,
                "additional_speed_tiers": [],
                "service_tiers": [],
                "availability_nux": Value::Null,
                "upgrade": Value::Null,
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "models": models })).unwrap_or_default()
}
```

- [ ] **步骤 4：运行测试验证通过**

运行：`cargo test -p codex-plus-core --test model_suffix 2>&1 | tail -20`
预期：8 个测试全部 PASS。

- [ ] **步骤 5：Commit**

```bash
git add crates/codex-plus-core/src/model_suffix.rs crates/codex-plus-core/tests/model_suffix.rs
git commit -m "feat(model_suffix): 新增条目收集与 catalog JSON 构建

collect_catalog_entries 收集 profile 全部模型（解 #1064 单模型副作用），
build_model_catalog_json 生成 codex 原生 catalog 格式，auto_compact 留 null。"
```

---

## 任务 3：apply_model_catalog_to_config + 接入 3 个入口

**文件：**
- 修改：`crates/codex-plus-core/src/relay_config.rs`（新增函数 + 3 处接入）
- 测试：`crates/codex-plus-core/tests/relay_config.rs`（新增用例）

- [ ] **步骤 1：编写失败的测试**

在 `crates/codex-plus-core/tests/relay_config.rs` 末尾追加：

```rust
#[test]
fn apply_relay_profile_generates_model_catalog_for_suffixed_models() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "deepseek-v4-pro".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "deepseek-v4-pro"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "deepseek-v4-pro[1M]\nclaude-sonnet-4[200K]".to_string(),
        context_window: "272000".to_string(),
        auto_compact_limit: String::new(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "model-catalogs/relay-a.json""#));
    let catalog_path = temp.path().join("model-catalogs").join("relay-a.json");
    assert!(catalog_path.exists());
    let catalog = std::fs::read_to_string(&catalog_path).unwrap();
    assert!(catalog.contains(r#""slug": "deepseek-v4-pro""#));
    assert!(catalog.contains(r#""context_window": 1000000"#));
    assert!(catalog.contains(r#""slug": "claude-sonnet-4""#));
    assert!(catalog.contains(r#""context_window": 200000"#));
    // 后缀不得进入 catalog 或 config
    assert!(!catalog.contains("[1M]"));
    assert!(!config.contains("[1M]"));
}
```

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test -p codex-plus-core --test relay_config apply_relay_profile_generates_model_catalog_for_suffixed_models 2>&1 | tail -20`
预期：FAIL，断言 `config.contains("model_catalog_json")` 失败（当前不生成 catalog）。

- [ ] **步骤 3：编写最少实现代码**

在 `crates/codex-plus-core/src/relay_config.rs` 中，找到 `apply_context_limits_to_config` 函数（约第 1355 行），在其后新增两个函数：

```rust
fn apply_model_catalog_to_config(
    home: &Path,
    profile: &RelayProfile,
    config_text: &str,
) -> anyhow::Result<String> {
    // 用户已手写 model_catalog_json 指针时保留，不覆盖（保 preserves_user_model_catalog_json 测试）
    if root_key_string(config_text, "model_catalog_json").is_some() {
        return Ok(config_text.to_string());
    }
    let entries = crate::model_suffix::collect_catalog_entries(&profile.model_list, &profile.model);
    // 无后缀条目则 no-op，保持现有 per-profile 单值行为（保 does_not_write 测试）
    if !entries.iter().any(|entry| entry.suffix_window.is_some()) {
        return Ok(config_text.to_string());
    }
    let fallback = parse_optional_positive_u64(&profile.context_window, "上下文大小")?;
    let catalog_relative = format!(
        "model-catalogs/{}.json",
        sanitize_catalog_filename(&profile.id)
    );
    let catalog_path = home.join(&catalog_relative);
    if let Some(parent) = catalog_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let catalog_json = crate::model_suffix::build_model_catalog_json(&entries, fallback);
    std::fs::write(&catalog_path, catalog_json)?;
    let mut doc = parse_toml_document(config_text)?;
    doc["model_catalog_json"] = toml_edit::value(catalog_relative);
    Ok(normalize_optional_toml(doc))
}

fn sanitize_catalog_filename(id: &str) -> String {
    id.chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || char == '-' || char == '_' {
                char
            } else {
                '-'
            }
        })
        .collect()
}
```

- [ ] **步骤 4：接入第一个 apply 入口（files_to_home_with_context）**

找到 `apply_relay_profile_files_to_home_with_context`（约第 349 行），将其末尾：

```rust
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;
    apply_relay_files_to_home(home, &config_with_limits, &profile.auth_contents)
```

改为：

```rust
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;
    let config_with_catalog = apply_model_catalog_to_config(home, profile, &config_with_limits)?;
    apply_relay_files_to_home(home, &config_with_catalog, &profile.auth_contents)
```

- [ ] **步骤 5：接入第二个 apply 入口（switch_rules_and_computer_use_guard）**

找到 `apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard`（约第 384 行），将其中的 `config_with_limits` 段：

```rust
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;

    if profile.relay_mode == crate::settings::RelayMode::PureApi {
        apply_relay_files_to_home_with_computer_use_guard(
            home,
            &config_with_limits,
            &profile.auth_contents,
            preserve_computer_use_guard,
        )
    } else {
        let auth_contents = official_profile_auth_for_switch(home, &profile.auth_contents)?;
        apply_relay_files_to_home_with_computer_use_guard(
            home,
            &config_with_limits,
            &auth_contents,
            preserve_computer_use_guard,
        )
    }
```

改为：

```rust
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;
    let config_with_catalog = apply_model_catalog_to_config(home, profile, &config_with_limits)?;

    if profile.relay_mode == crate::settings::RelayMode::PureApi {
        apply_relay_files_to_home_with_computer_use_guard(
            home,
            &config_with_catalog,
            &profile.auth_contents,
            preserve_computer_use_guard,
        )
    } else {
        let auth_contents = official_profile_auth_for_switch(home, &profile.auth_contents)?;
        apply_relay_files_to_home_with_computer_use_guard(
            home,
            &config_with_catalog,
            &auth_contents,
            preserve_computer_use_guard,
        )
    }
```

- [ ] **步骤 6：接入第三个 apply 入口（config_to_home_with_context）**

找到 `apply_relay_profile_config_to_home_with_context`（约第 423 行），将其末尾：

```rust
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;
    apply_relay_config_file_to_home(home, &config_with_limits)
```

改为：

```rust
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &profile.context_window,
        &profile.auto_compact_limit,
    )?;
    let config_with_catalog = apply_model_catalog_to_config(home, profile, &config_with_limits)?;
    apply_relay_config_file_to_home(home, &config_with_catalog)
```

- [ ] **步骤 7：运行新测试验证通过**

运行：`cargo test -p codex-plus-core --test relay_config apply_relay_profile_generates_model_catalog_for_suffixed_models 2>&1 | tail -20`
预期：PASS。

- [ ] **步骤 8：Commit**

```bash
git add crates/codex-plus-core/src/relay_config.rs crates/codex-plus-core/tests/relay_config.rs
git commit -m "feat(relay_config): apply 时按后缀生成 model_catalog_json

新增 apply_model_catalog_to_config，接入 3 个 apply 入口（limits 之后、落盘之前）。
有后缀条目才生成 catalog + 写相对路径指针；无后缀 no-op；用户手写指针不覆盖。"
```

---

## 任务 4：兼容性与回归测试

**文件：**
- 测试：`crates/codex-plus-core/tests/relay_config.rs`（新增用例 + 跑现有回归）

- [ ] **步骤 1：编写无后缀 no-op 测试**

在 `crates/codex-plus-core/tests/relay_config.rs` 末尾追加：

```rust
#[test]
fn apply_relay_profile_no_catalog_when_model_list_has_no_suffix() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "qwen3-coder".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "qwen3-coder"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_list: "deepseek-coder\nqwen3-coder".to_string(),
        context_window: "200000".to_string(),
        auto_compact_limit: "160000".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("model_catalog_json"));
    assert!(config.contains("model_context_window = 200000"));
    assert!(!temp.path().join("model-catalogs").exists());
}

#[test]
fn apply_relay_profile_does_not_overwrite_user_model_catalog_json() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "deepseek-v4-pro".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "deepseek-v4-pro"
model_catalog_json = "/old/catalog.json"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        // 即使有后缀，用户已手写指针也应保留不覆盖
        model_list: "deepseek-v4-pro[1M]".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "/old/catalog.json""#));
    assert!(!config.contains("model-catalogs/relay-a.json"));
    assert!(!temp.path().join("model-catalogs").exists());
}
```

- [ ] **步骤 2：运行新测试验证通过**

运行：`cargo test -p codex-plus-core --test relay_config apply_relay_profile_no_catalog_when_model_list_has_no_suffix apply_relay_profile_does_not_overwrite_user_model_catalog_json 2>&1 | tail -20`
预期：2 个测试 PASS。

- [ ] **步骤 3：跑现有「不写 catalog」回归测试确认未破坏**

运行：`cargo test -p codex-plus-core --test relay_config apply_relay_profile_does_not_write_model_catalog_json_for_selected_models apply_relay_profile_preserves_user_model_catalog_json 2>&1 | tail -20`
预期：2 个现有测试仍 PASS（无后缀 no-op + 手写指针保留，行为不变）。

- [ ] **步骤 4：跑 relay_config 全量回归**

运行：`cargo test -p codex-plus-core --test relay_config 2>&1 | tail -20`
预期：全部 PASS。

- [ ] **步骤 5：Commit**

```bash
git add crates/codex-plus-core/tests/relay_config.rs
git commit -m "test(relay_config): 补充 catalog 生成的兼容性与回归用例

无后缀 no-op、用户手写指针不覆盖；确认现有 does_not_write / preserves 用例未破坏。"
```

---

## 任务 5：手工验证工具（example）

**文件：**
- 创建：`crates/codex-plus-core/examples/generate_model_catalog.rs`

- [ ] **步骤 1：创建 example**

创建 `crates/codex-plus-core/examples/generate_model_catalog.rs`：

```rust
//! 手工验证工具：从命令行参数生成 catalog JSON。
//! 用法：
//!   cargo run -p codex-plus-core --example generate_model_catalog -- \
//!       "deepseek-v4-pro[1M]" "claude-sonnet-4[200K]" > catalog.json

use codex_plus_core::model_suffix::{build_model_catalog_json, collect_catalog_entries};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model_list = args.join("\n");
    let entries = collect_catalog_entries(&model_list, "");
    print!("{}", build_model_catalog_json(&entries, None));
}
```

- [ ] **步骤 2：编译验证**

运行：`cargo build -p codex-plus-core --example generate_model_catalog 2>&1 | tail -10`
预期：编译成功，无错误。

- [ ] **步骤 3：Commit**

```bash
git add crates/codex-plus-core/examples/generate_model_catalog.rs
git commit -m "feat(example): 新增 generate_model_catalog 手工验证工具"
```

---

## 任务 6：实跑验证（A 预检 + B 对拍）

**目标：** 确认 codex 客户端真正读取原型 catalog 并按模型报 1M（而非 272000）。这是阶段一的核心验收。

- [ ] **步骤 1：A 预检——手写最小 catalog，确认 codex 读取格式**

准备最小 catalog（仅必要字段，与 `build_model_catalog_json` 产物一致）：

```bash
cat > /tmp/cpp-proto-catalog.json <<'EOF'
{
  "models": [
    {
      "slug": "deepseek-v4-pro",
      "display_name": "deepseek-v4-pro",
      "description": "deepseek-v4-pro",
      "context_window": 1000000,
      "max_context_window": 1000000,
      "auto_compact_token_limit": null,
      "priority": 1000,
      "visibility": "list",
      "supported_in_api": true,
      "additional_speed_tiers": [],
      "service_tiers": [],
      "availability_nux": null,
      "upgrade": null
    }
  ]
}
EOF
```

用 `-c` 内联覆盖（不污染 `~/.codex/config.toml`），观察 codex 读到的窗口：

```bash
codex debug models \
  -c model="deepseek-v4-pro" \
  -c model_catalog_json="/tmp/cpp-proto-catalog.json" \
  | python3 -c "import sys,json; d=json.load(sys.stdin); print(json.dumps([{'slug':m.get('slug'),'context_window':m.get('context_window')} for m in d.get('models',[])],ensure_ascii=False,indent=2))"
```

预期输出：`deepseek-v4-pro` 的 `context_window = 1000000`（而非 272000）。

- 若显示 272000 或不含该模型 → catalog 未被读取。排查：
  - 路径：Mac 绝对路径 `/tmp/...` 是否被 codex 接受（#931 转义结论是 Windows 的，Mac 待验证）
  - 字段名：试改用相对路径，或检查 codex 0.128.0 是否需 `effective_context_window_percent`
- 记录结论到 `docs/research/01-调研结果.md` 第八节。

- [ ] **步骤 2：B 对拍——原型产物实跑 codex**

用 example 生成原型 catalog：

```bash
cargo run -p codex-plus-core --example generate_model_catalog -- "deepseek-v4-pro[1M]" "claude-sonnet-4[200K]" > /tmp/cpp-proto-out.json
cat /tmp/cpp-proto-out.json
```

预期：JSON 含 `deepseek-v4-pro` context_window=1000000、`claude-sonnet-4` context_window=200000，无 `[1M]` 后缀。

让 codex 读原型产物：

```bash
codex debug models \
  -c model="deepseek-v4-pro" \
  -c model_catalog_json="/tmp/cpp-proto-out.json" \
  | python3 -c "import sys,json; d=json.load(sys.stdin); print(json.dumps([{'slug':m.get('slug'),'context_window':m.get('context_window')} for m in d.get('models',[])],ensure_ascii=False,indent=2))"
```

预期：`deepseek-v4-pro` = 1000000、`claude-sonnet-4` = 200000，与 A 预检一致。

- [ ] **步骤 3：必验项核对**

对照 `docs/specs/2026-06-23-model-catalog-prototype-design.md` 第五节逐项记录：

1. **Mac 路径**：`-c model_catalog_json="/tmp/..."`（绝对路径）是否生效；若生效，原型用相对路径 `model-catalogs/<id>.json` 须额外验证（codex 相对 `~/.codex/` 解析）
2. **字段名**：`context_window` + `max_context_window` 是否让 codex 报 1M
3. **auto_compact null**：1M 窗口是否在合理比例压缩（非 220K 低值）；若异常，fallback 改写 `context_window × 0.85` 进 `auto_compact_token_limit`
4. **单模型副作用**：catalog 含 2 个模型时，`codex debug models` 是否列出全部 2 个（解 #1064）

- [ ] **步骤 4：记录验证结论并 Commit**

将 A/B 验证结论写入 `docs/research/01-调研结果.md` 第八节（Mac 路径格式、字段生效情况、auto_compact 行为、单模型副作用是否解决）。

```bash
git add docs/research/01-调研结果.md
git commit -m "docs(research): 补充阶段一 A/B 实跑验证结论（Mac 路径/字段/auto_compact/副作用）"
```

---

## 全量验收

- [ ] **步骤 1：codex-plus-core 全量测试**

运行：`cargo test -p codex-plus-core 2>&1 | tail -30`
预期：全部 PASS（含 model_suffix 8 个 + relay_config 新增 3 个 + 现有全部）。

- [ ] **步骤 2：全 workspace 编译**

运行：`cargo build 2>&1 | tail -10`
预期：编译成功。

- [ ] **阶段一完成标准**
  - `cargo test -p codex-plus-core` 全过，旧测试未破坏
  - `codex debug models` 读原型 catalog 报 1M（非 272000）
  - catalog 含全部模型，无单模型副作用
  - 验证结论已记录

---

## 风险与回退

| 风险 | 回退 |
| --- | --- |
| codex 不认原型 catalog 字段（A 预检失败） | 试加 `effective_context_window_percent: 100`；试克隆 codex-models.json 完整 entry 模板（抄 cc-switch `find_codex_model_template`） |
| Mac 相对路径不生效 | config.toml 改写绝对路径 `home.join(...)` 全路径注入 |
| auto_compact null 致低值压缩 | `build_model_catalog_json` 改写 `auto_compact_token_limit = context_window * 0.85` |
| 单模型副作用未解 | 确认 catalog 含全部模型；查 codex 0.128.0 是否仍只显示 `model =` 指定项 |
