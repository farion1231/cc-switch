//! 代理模式的附加模型（UI 叫「附加模型」，代码叫 pool）。
//!
//! 代理模式下，用户把几家第三方供应商标成「附加」：它们的模型以带保留前缀的 id 发布给
//! 客户端，选中后请求直达那一家；不带前缀的请求照旧走代理路由和故障转移。
//!
//! - 名单和 key 登记簿存在 `live-state.json`（[`PoolState`]），增删和客户端文件在同一个
//!   操作里提交（`controller::set_pool_member`）；
//! - key 一经分配永久归这家（[`allocate_key`]）：客户端会一直带着选中过的 id，key 改了
//!   指向，旧 id 就会被悄悄发到另一家；
//! - 带保留前缀的 id 解不出来（成员已移除、供应商已删除、key 没登记）一律报错，不回落到
//!   默认路由（[`resolve`]）：回落会用别家的钱、别家的模型回答，用户看不出来；
//! - 附加请求不读也不写任何路由状态（熔断器、故障转移、「正在使用」、代理统计），见
//!   `proxy::forwarder` 的 `routing_state_enabled`。

use serde::Serialize;
use serde_json::{Map, Value};

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::DeviceStore;
use crate::live::project::claude::{has_one_m_marker, ONE_M_MARKER_FOR_CLIENT};
use crate::provider::Provider;
use crate::proxy::model_mapper::strip_one_m_suffix_for_upstream;

use super::state::{self, PoolState};

/// Claude Code 的附加模型 id：`ccs-claude-<key>--<model>`。id 里要有 `claude` 才进
/// `/model` 选择器，不以 `claude-` 开头 MAX 窗口才生效。
const CLAUDE_PREFIX: &str = "ccs-claude-";
const CLAUDE_SEPARATOR: &str = "--";
/// Codex 的附加模型 id：`ccs-<key>/<model>`。
const CODEX_PREFIX: &str = "ccs-";
const CODEX_SEPARATOR: char = '/';

/// key 的最大长度：只是为了模型 id 不至于太长。
const KEY_MAX_LEN: usize = 24;

/// Claude Code 在没有 `CLAUDE_CODE_MAX_CONTEXT_TOKENS` 时按这个窗口算。
pub const CLAUDE_DEFAULT_WINDOW: u64 = 200_000;

/// 支持附加模型的应用。
pub fn supports_pool(app: &AppType) -> bool {
    matches!(app, AppType::Claude | AppType::Codex)
}

/// 给 `provider` 一个 key：登记过就用原来的（移除后重新加入，旧会话里的 id 继续有效），
/// 没有就按图标、名称生成一个新的写进登记簿。
///
/// 新 key 和登记簿里所有的 key 去重，不只是当前成员：已移除、已删除的供应商的 key 也
/// 占着位置，旧 id 才不会被发给新来的这家。
pub fn allocate_key(pool: &mut PoolState, provider: &Provider) -> String {
    if let Some(key) = pool.key_of(&provider.id) {
        return key.to_string();
    }
    let base = [provider.icon.as_deref(), Some(provider.name.as_str())]
        .into_iter()
        .flatten()
        .map(slug)
        .find(|candidate| !candidate.is_empty())
        .unwrap_or_else(|| {
            let id: String = slug(&provider.id)
                .chars()
                .filter(|c| *c != '-')
                .take(6)
                .collect();
            format!("p{id}")
        });
    let mut key = base.clone();
    let mut suffix = 2;
    while pool.keys.contains_key(&key) {
        key = format!("{base}-{suffix}");
        suffix += 1;
    }
    pool.keys.insert(key.clone(), provider.id.clone());
    key
}

/// 小写 ASCII，只保留 `[a-z0-9-]`，其余字符换成 `-`，连续的 `-` 合并，首尾的 `-` 去掉。
/// 结果里不会有 `--`（Claude id 的分隔符）和 `/`（Codex id 的分隔符）。
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= KEY_MAX_LEN {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// 附加模型 id。Claude 的上游是 1M 窗口时末尾带 `[1M]`，Claude Code 才按 1M 计算。
pub fn encode(app: &AppType, key: &str, model: &str, one_m: bool) -> String {
    match app {
        AppType::Codex => format!("{CODEX_PREFIX}{key}{CODEX_SEPARATOR}{model}"),
        _ => {
            let marker = if one_m { ONE_M_MARKER_FOR_CLIENT } else { "" };
            format!("{CLAUDE_PREFIX}{key}{CLAUDE_SEPARATOR}{model}{marker}")
        }
    }
}

/// 解码的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoded<'a> {
    /// 不带保留前缀：普通模型名，照旧走代理路由。
    Plain,
    /// 带保留前缀，但切不出 key 和模型。
    Malformed,
    /// 带保留前缀。`model` 可以含 `/` 和 `--`（key 里没有这两种分隔符，在第一个处切开）。
    Pool {
        key: &'a str,
        model: &'a str,
        /// Claude id 末尾带着 1M 标记。
        one_m: bool,
    },
}

/// 按客户端的格式解码模型 id。Claude：`ccs-claude-` 开头；Codex：`ccs-` 开头且含 `/`。
pub fn decode<'a>(app: &AppType, id: &'a str) -> Decoded<'a> {
    let (rest, separator, one_m) = match app {
        AppType::Claude => {
            let Some(rest) = id.strip_prefix(CLAUDE_PREFIX) else {
                return Decoded::Plain;
            };
            let stripped = strip_one_m_suffix_for_upstream(rest);
            (stripped, CLAUDE_SEPARATOR, stripped.len() != rest.len())
        }
        AppType::Codex => {
            let Some(rest) = id.strip_prefix(CODEX_PREFIX) else {
                return Decoded::Plain;
            };
            if !rest.contains(CODEX_SEPARATOR) {
                return Decoded::Plain;
            }
            (rest, "/", false)
        }
        _ => return Decoded::Plain,
    };
    match rest.split_once(separator) {
        Some((key, model)) if !key.is_empty() && !model.is_empty() => {
            Decoded::Pool { key, model, one_m }
        }
        _ => Decoded::Malformed,
    }
}

/// 附加供应商发布给客户端的一个模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolModel {
    /// 发布给客户端的 id（带保留前缀）。
    pub id: String,
    /// 发往上游的模型名：行里配置的原值（可能带 `[1M]`，转发时和路由请求一样处理）。
    pub upstream: String,
    /// 选择器里显示的名字：`<显示名>（<供应商名>）`。
    pub display_name: String,
    /// 选择器里的说明。
    pub description: String,
    /// 上游是 1M 窗口（id 带 `[1M]`）。
    pub one_m: bool,
    /// Claude 非 1M 模型的窗口：行里的 `CLAUDE_CODE_MAX_CONTEXT_TOKENS`，没有是 200K。
    /// Codex 的窗口写在目录条目里，这里是 0。
    pub window: u64,
}

/// Claude 行发布的模型：`ANTHROPIC_MODEL` 和各档 `ANTHROPIC_DEFAULT_*_MODEL`，按去掉 1M
/// 标记后的名字去重（任何一处带标记就按 1M）。显示名取对应档位的 `*_MODEL_NAME`。
pub fn claude_models(key: &str, provider: &Provider) -> Vec<PoolModel> {
    const ROLES: [(&str, Option<&str>); 5] = [
        ("ANTHROPIC_MODEL", None),
        (
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            Some("ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"),
        ),
        (
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            Some("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"),
        ),
        (
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            Some("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME"),
        ),
        (
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            Some("ANTHROPIC_DEFAULT_FABLE_MODEL_NAME"),
        ),
    ];
    let empty = Map::new();
    let env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let window = env
        .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
        .and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        })
        .filter(|window| *window > 0)
        .unwrap_or(CLAUDE_DEFAULT_WINDOW);

    // (去掉标记的模型名, 行里的原值, 显示名, 1M)
    let mut found: Vec<(String, String, Option<String>, bool)> = Vec::new();
    for (model_key, name_key) in ROLES {
        let Some(upstream) = env_string(env, model_key) else {
            continue;
        };
        let model = strip_one_m_suffix_for_upstream(upstream).trim().to_string();
        if model.is_empty() {
            continue;
        }
        let name = name_key.and_then(|name_key| env_string(env, name_key).map(str::to_string));
        let one_m = has_one_m_marker(upstream);
        match found.iter_mut().find(|entry| entry.0 == model) {
            Some(entry) => {
                // 同一个模型有一处带 1M 标记就按 1M，发往上游的也用带标记的那个写法。
                if one_m && !entry.3 {
                    entry.1 = upstream.to_string();
                    entry.3 = true;
                }
                if entry.2.is_none() {
                    entry.2 = name;
                }
            }
            None => found.push((model, upstream.to_string(), name, one_m)),
        }
    }

    found
        .into_iter()
        .map(|(model, upstream, name, one_m)| PoolModel {
            id: encode(&AppType::Claude, key, &model, one_m),
            display_name: display_name(name.as_deref().unwrap_or(&model), &provider.name),
            description: routed_description(&provider.name),
            upstream,
            one_m,
            window,
        })
        .collect()
}

/// Codex 行发布的模型：行里的模型目录，没有配置目录时只有行的 `model`。显示名和窗口
/// 由目录条目决定（`codex_config::plan_codex_pool_catalog`），这里只给 id 和上游模型名。
pub fn codex_models(key: &str, provider: &Provider) -> Vec<PoolModel> {
    let config = provider
        .settings_config
        .get("config")
        .and_then(Value::as_str)
        .unwrap_or("");
    crate::codex_config::codex_published_models(&provider.settings_config, config)
        .into_iter()
        .map(|model| PoolModel {
            id: encode(&AppType::Codex, key, &model, false),
            display_name: display_name(&model, &provider.name),
            description: routed_description(&provider.name),
            upstream: model,
            one_m: false,
            window: 0,
        })
        .collect()
}

/// 选择器里附加模型的显示名：`<模型显示名>（<供应商名>）`。
pub fn display_name(model: &str, provider_name: &str) -> String {
    format!("{model}（{provider_name}）")
}

/// 选择器里附加模型的说明。
pub fn routed_description(provider_name: &str) -> String {
    format!("经 CC Switch 路由到 {provider_name} (Routed by CC Switch to {provider_name})")
}

fn env_string<'a>(env: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    env.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// 一家附加供应商发布的模型。
pub fn models_of(app: &AppType, key: &str, provider: &Provider) -> Vec<PoolModel> {
    match app {
        AppType::Claude => claude_models(key, provider),
        AppType::Codex => codex_models(key, provider),
        _ => Vec::new(),
    }
}

/// 名单里的一家。
#[derive(Debug, Clone)]
pub struct Member {
    pub provider: Provider,
    pub key: String,
    pub models: Vec<PoolModel>,
}

/// 名单里还在库里的成员，按加入顺序。库里已经没有的跳过（删除供应商会先把它移出名单，
/// 删行前失败才会留下）。
pub fn members(db: &Database, app: &AppType, pool: &PoolState) -> Result<Vec<Member>, AppError> {
    let mut members = Vec::with_capacity(pool.members.len());
    for id in &pool.members {
        let Some(key) = pool.key_of(id) else {
            log::warn!("{} 的附加模型成员 {id} 没有登记 key，跳过", app.as_str());
            continue;
        };
        let Some(provider) = db.get_provider_by_id(id, app.as_str())? else {
            continue;
        };
        let models = models_of(app, key, &provider);
        members.push(Member {
            key: key.to_string(),
            provider,
            models,
        });
    }
    Ok(members)
}

/// 发布给客户端的模型：各成员的模型按名单顺序排，路由那家跳过（它的模型已经通过默认
/// 路由出现，名单保留）。
pub fn published(members: &[Member], route: Option<&str>) -> Vec<PoolModel> {
    members
        .iter()
        .filter(|member| Some(member.provider.id.as_str()) != route)
        .flat_map(|member| member.models.iter().cloned())
        .collect()
}

/// 代理模式下 `provider_id` 在附加名单里：它的行变了，客户端契约可能跟着变（发布的模型、
/// 窗口）。不在代理模式时名单不进契约。
pub fn is_member_in_proxy(app: &AppType, provider_id: &str) -> bool {
    if !supports_pool(app) {
        return false;
    }
    let store = DeviceStore::for_device();
    let read = || -> Result<bool, AppError> {
        Ok(state::mode_state(&store, app.as_str())?.is_proxy()
            && state::pool(&store, app.as_str())?.is_member(provider_id))
    };
    read().unwrap_or_else(|error| {
        log::warn!("读取 {} 的附加模型失败: {error}", app.as_str());
        false
    })
}

/// 在附加名单里（不管什么模式）。
pub fn is_member(app: &AppType, provider_id: &str) -> Result<bool, AppError> {
    if !supports_pool(app) {
        return Ok(false);
    }
    Ok(state::pool(&DeviceStore::for_device(), app.as_str())?.is_member(provider_id))
}

/// 这个应用现在发布的附加模型：代理模式下按已落定的名单和路由算，不在代理模式时没有。
pub fn published_now(db: &Database, app: &AppType) -> Result<Vec<PoolModel>, AppError> {
    if !supports_pool(app) {
        return Ok(Vec::new());
    }
    let store = DeviceStore::for_device();
    let mode = state::mode_state(&store, app.as_str())?;
    if !mode.is_proxy() {
        return Ok(Vec::new());
    }
    let pool = state::pool(&store, app.as_str())?;
    if pool.members.is_empty() {
        return Ok(Vec::new());
    }
    Ok(published(
        &members(db, app, &pool)?,
        mode.proxy_route.as_deref(),
    ))
}

/// 选中附加模型的请求要发往的那一家。
#[derive(Debug, Clone)]
pub struct PoolTarget {
    pub provider: Provider,
    /// 发往上游的模型名。
    pub upstream_model: String,
    /// 客户端发来的带前缀 id（只用于日志展示）。
    pub original_model: String,
}

/// 带保留前缀的 id 为什么解不出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolMiss {
    /// key 没登记，或 id 切不出 key 和模型。
    Unknown,
    /// 这家已经从附加名单移除。
    Removed,
    /// 这家已经从 CC Switch 删除。
    Deleted,
}

impl PoolMiss {
    /// 返回给客户端的错误文案。
    pub fn message(&self, model: &str) -> String {
        match self {
            Self::Unknown => format!(
                "附加模型 {model} 在 CC Switch 里不存在，请在模型列表里重新选择 (Attached model {model} is unknown to CC Switch; pick a model from the model list again)"
            ),
            Self::Removed => format!(
                "附加模型 {model} 已从 CC Switch 移除，请在模型列表里重新选择 (Attached model {model} was removed from CC Switch; pick a model from the model list again)"
            ),
            Self::Deleted => format!(
                "附加模型 {model} 对应的供应商已删除，请在模型列表里重新选择 (The provider of attached model {model} was deleted; pick a model from the model list again)"
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Resolved {
    /// 普通模型名，照旧走代理路由。
    Plain,
    Hit(Box<PoolTarget>),
    /// 带保留前缀但解不出来：报错，不回落到默认路由。名单为空时也一样。
    Miss(PoolMiss),
}

/// 解析请求里的模型 id。不带保留前缀时不读任何状态，路由请求的路径不变。
pub fn resolve(
    db: &Database,
    store: &DeviceStore,
    app: &AppType,
    model: &str,
) -> Result<Resolved, AppError> {
    let (key, model_part, one_m) = match decode(app, model) {
        Decoded::Plain => return Ok(Resolved::Plain),
        Decoded::Malformed => return Ok(Resolved::Miss(PoolMiss::Unknown)),
        Decoded::Pool { key, model, one_m } => (key, model, one_m),
    };
    let pool = state::pool(store, app.as_str())?;
    let Some(provider_id) = pool.keys.get(key) else {
        return Ok(Resolved::Miss(PoolMiss::Unknown));
    };
    let provider = db.get_provider_by_id(provider_id, app.as_str())?;
    let Some(provider) = provider else {
        return Ok(Resolved::Miss(PoolMiss::Deleted));
    };
    if !pool.is_member(provider_id) {
        return Ok(Resolved::Miss(PoolMiss::Removed));
    }
    // 行里配置的原值（可能带 1M 标记）和路由请求映射出来的一样；行里已经没有这个模型时
    // 照原样发（上游自己决定认不认）。
    let upstream_model = models_of(app, key, &provider)
        .into_iter()
        .find(|published| strip_one_m_suffix_for_upstream(&published.upstream).trim() == model_part)
        .map(|published| published.upstream)
        .unwrap_or_else(|| {
            if one_m {
                format!("{model_part}{ONE_M_MARKER_FOR_CLIENT}")
            } else {
                model_part.to_string()
            }
        });
    Ok(Resolved::Hit(Box::new(PoolTarget {
        provider,
        upstream_model,
        original_model: model.to_string(),
    })))
}

/// 给前端：名单里的一家。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolMemberView {
    pub provider_id: String,
    pub key: String,
    /// 发布给客户端的模型 id。
    pub model_ids: Vec<String>,
}

/// 给前端：附加模型的名单和提示。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolView {
    pub members: Vec<PoolMemberView>,
    /// Codex 官方做路由时官方模型列表暂未取到：`officialModelsBundled` 暂用 Codex 自带的
    /// 列表（可能缺账号专属的模型），`officialModelsUnavailable` 附加模型暂不可用。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<&'static str>,
}

pub fn member_views(members: &[Member]) -> Vec<PoolMemberView> {
    members
        .iter()
        .map(|member| PoolMemberView {
            provider_id: member.provider.id.clone(),
            key: member.key.clone(),
            model_ids: member.models.iter().map(|model| model.id.clone()).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(id: &str, name: &str, icon: Option<&str>, env: Value) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            name.to_string(),
            json!({ "env": env }),
            None,
        );
        provider.icon = icon.map(str::to_string);
        provider
    }

    #[test]
    fn keys_come_from_the_icon_then_the_name() {
        let mut pool = PoolState::default();
        let kimi = provider("a", "Kimi For Coding", Some("kimi"), json!({}));
        let named = provider("b", "My Relay 2.0!", None, json!({}));
        let chinese = provider("c1b2c3d4-e5f6", "智谱", None, json!({}));
        let blank_icon = provider("d", "DeepSeek", Some("  "), json!({}));

        assert_eq!(allocate_key(&mut pool, &kimi), "kimi");
        assert_eq!(allocate_key(&mut pool, &named), "my-relay-2-0");
        assert_eq!(allocate_key(&mut pool, &chinese), "pc1b2c3");
        assert_eq!(allocate_key(&mut pool, &blank_icon), "deepseek");
    }

    #[test]
    fn keys_never_contain_the_separators_and_stay_short() {
        for text in ["a--b", "a/b", "--edge--", "UPPER__lower", &"x".repeat(80)] {
            let key = slug(text);
            assert!(!key.contains("--") && !key.contains('/'), "{text} → {key}");
            assert!(
                !key.starts_with('-') && !key.ends_with('-'),
                "{text} → {key}"
            );
            assert!(key.len() <= KEY_MAX_LEN, "{text} → {key}");
        }
        assert_eq!(slug("UPPER__lower"), "upper-lower");
    }

    #[test]
    fn a_key_stays_with_its_provider_forever() {
        let mut pool = PoolState::default();
        let a = provider("a", "Kimi", Some("kimi"), json!({}));
        let b = provider("b", "Kimi Coding Plan", Some("kimi"), json!({}));

        assert_eq!(allocate_key(&mut pool, &a), "kimi");
        pool.members.push("a".to_string());
        // A 移除（登记簿保留）后，同图标的 B 拿不到 A 的 key。
        pool.members.clear();
        assert_eq!(allocate_key(&mut pool, &b), "kimi-2");
        // A 重新加入，拿回原来的 key。
        assert_eq!(allocate_key(&mut pool, &a), "kimi");
        assert_eq!(pool.keys.len(), 2);
    }

    #[test]
    fn claude_ids_round_trip() {
        let claude = AppType::Claude;
        for (model, one_m) in [
            ("kimi-k3", false),
            ("glm-5.2", true),
            ("vendor/model", false),
            ("model--with--dashes", true),
        ] {
            let id = encode(&claude, "kimi", model, one_m);
            assert_eq!(
                decode(&claude, &id),
                Decoded::Pool {
                    key: "kimi",
                    model,
                    one_m
                },
                "{id}"
            );
        }
        // 标记大小写不敏感。
        assert_eq!(
            decode(&claude, "ccs-claude-k--m[1m]"),
            Decoded::Pool {
                key: "k",
                model: "m",
                one_m: true
            }
        );
    }

    #[test]
    fn codex_ids_round_trip() {
        let codex = AppType::Codex;
        let id = encode(&codex, "deepseek", "deepseek/deepseek-v4-pro", false);
        assert_eq!(id, "ccs-deepseek/deepseek/deepseek-v4-pro");
        assert_eq!(
            decode(&codex, &id),
            Decoded::Pool {
                key: "deepseek",
                model: "deepseek/deepseek-v4-pro",
                one_m: false
            }
        );
    }

    #[test]
    fn plain_ids_and_other_apps_are_not_decoded() {
        let (claude, codex) = (AppType::Claude, AppType::Codex);
        assert_eq!(decode(&claude, "claude-sonnet-5"), Decoded::Plain);
        assert_eq!(decode(&claude, "kimi-k3"), Decoded::Plain);
        // 路由那家自己的 `deepseek/…` 不被同名的附加 key 截走。
        assert_eq!(decode(&codex, "deepseek/deepseek-v4-pro"), Decoded::Plain);
        assert_eq!(decode(&codex, "ccs-without-separator"), Decoded::Plain);
        assert_eq!(
            decode(&AppType::ClaudeDesktop, "ccs-claude-k--m"),
            Decoded::Plain
        );
        assert_eq!(decode(&AppType::GrokBuild, "ccs-k/m"), Decoded::Plain);
    }

    #[test]
    fn reserved_ids_that_do_not_split_are_malformed() {
        let (claude, codex) = (AppType::Claude, AppType::Codex);
        assert_eq!(decode(&claude, "ccs-claude-kimi"), Decoded::Malformed);
        assert_eq!(decode(&claude, "ccs-claude---m"), Decoded::Malformed);
        assert_eq!(decode(&claude, "ccs-claude-k--"), Decoded::Malformed);
        assert_eq!(decode(&codex, "ccs-/m"), Decoded::Malformed);
        assert_eq!(decode(&codex, "ccs-k/"), Decoded::Malformed);
    }

    #[test]
    fn claude_rows_publish_each_model_once() {
        let row = provider(
            "p",
            "Zhipu",
            None,
            json!({
                "ANTHROPIC_MODEL": "glm-5.2",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5.2[1M]",
                "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "GLM 5.2",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.7-air",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "128000"
            }),
        );
        let models = claude_models("zhipu", &row);
        assert_eq!(
            models,
            vec![
                PoolModel {
                    id: "ccs-claude-zhipu--glm-5.2[1M]".to_string(),
                    upstream: "glm-5.2[1M]".to_string(),
                    display_name: "GLM 5.2（Zhipu）".to_string(),
                    description: "经 CC Switch 路由到 Zhipu (Routed by CC Switch to Zhipu)"
                        .to_string(),
                    one_m: true,
                    window: 128_000,
                },
                PoolModel {
                    id: "ccs-claude-zhipu--glm-4.7-air".to_string(),
                    upstream: "glm-4.7-air".to_string(),
                    display_name: "glm-4.7-air（Zhipu）".to_string(),
                    description: "经 CC Switch 路由到 Zhipu (Routed by CC Switch to Zhipu)"
                        .to_string(),
                    one_m: false,
                    window: 128_000,
                },
            ]
        );
        let bare = provider("q", "Bare", None, json!({ "ANTHROPIC_AUTH_TOKEN": "sk" }));
        assert!(claude_models("bare", &bare).is_empty());
        let default_window = provider("r", "R", None, json!({ "ANTHROPIC_MODEL": "m" }));
        assert_eq!(
            claude_models("r", &default_window)[0].window,
            CLAUDE_DEFAULT_WINDOW
        );
    }

    #[test]
    fn the_route_is_not_published_twice() {
        let a = provider("a", "A", None, json!({ "ANTHROPIC_MODEL": "a-1" }));
        let b = provider("b", "B", None, json!({ "ANTHROPIC_MODEL": "b-1" }));
        let members: Vec<Member> = [(a, "a"), (b, "b")]
            .into_iter()
            .map(|(provider, key)| Member {
                models: claude_models(key, &provider),
                key: key.to_string(),
                provider,
            })
            .collect();
        let ids = |route| {
            published(&members, route)
                .into_iter()
                .map(|model| model.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(None), vec!["ccs-claude-a--a-1", "ccs-claude-b--b-1"]);
        assert_eq!(ids(Some("a")), vec!["ccs-claude-b--b-1"]);
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: DeviceStore,
        db: Database,
    }

    /// kimi、zhipu 在名单里；gone 登记过但已移除；deleted 在名单里但行已经删了。
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store = DeviceStore::at(dir.path());
        let db = Database::memory().unwrap();
        for row in [
            provider(
                "kimi",
                "Kimi",
                Some("kimi"),
                json!({ "ANTHROPIC_MODEL": "kimi-k3" }),
            ),
            provider(
                "zhipu",
                "Zhipu",
                None,
                json!({ "ANTHROPIC_MODEL": "glm-5.2[1M]" }),
            ),
            provider("gone", "Gone", None, json!({ "ANTHROPIC_MODEL": "g-1" })),
        ] {
            db.save_provider("claude", &row).unwrap();
        }
        state::update(&store, |live| {
            let pool = &mut live.apps.entry("claude".to_string()).or_default().pool;
            pool.members = ["kimi", "zhipu", "deleted"].map(str::to_string).to_vec();
            for id in ["kimi", "zhipu", "gone", "deleted"] {
                pool.keys.insert(id.to_string(), id.to_string());
            }
        })
        .unwrap();
        Fixture {
            _dir: dir,
            store,
            db,
        }
    }

    fn resolve_in(fx: &Fixture, app: AppType, model: &str) -> Resolved {
        resolve(&fx.db, &fx.store, &app, model).unwrap()
    }

    fn hit(resolved: Resolved) -> (String, String, String) {
        match resolved {
            Resolved::Hit(target) => (
                target.provider.id,
                target.upstream_model,
                target.original_model,
            ),
            other => panic!("expected a hit, got {other:?}"),
        }
    }

    fn miss(resolved: Resolved) -> PoolMiss {
        match resolved {
            Resolved::Miss(miss) => miss,
            other => panic!("expected a miss, got {other:?}"),
        }
    }

    #[test]
    fn attached_ids_resolve_to_their_provider_and_upstream_model() {
        let fx = fixture();
        assert_eq!(
            hit(resolve_in(&fx, AppType::Claude, "ccs-claude-kimi--kimi-k3")),
            (
                "kimi".to_string(),
                "kimi-k3".to_string(),
                "ccs-claude-kimi--kimi-k3".to_string()
            )
        );
        // 行里带 1M 标记的模型：不管客户端发来的 id 带不带标记，上游都用行里的原值。
        for id in ["ccs-claude-zhipu--glm-5.2", "ccs-claude-zhipu--glm-5.2[1m]"] {
            assert_eq!(
                hit(resolve_in(&fx, AppType::Claude, id)).1,
                "glm-5.2[1M]",
                "{id}"
            );
        }
        // 行里已经没有的模型照原样发。
        assert_eq!(
            hit(resolve_in(&fx, AppType::Claude, "ccs-claude-kimi--kimi-k9")).1,
            "kimi-k9"
        );
    }

    #[test]
    fn attached_ids_that_cannot_be_resolved_never_fall_back_to_the_route() {
        let fx = fixture();
        let claude = |id| miss(resolve_in(&fx, AppType::Claude, id));
        assert_eq!(claude("ccs-claude-gone--g-1"), PoolMiss::Removed);
        assert_eq!(claude("ccs-claude-deleted--d-1"), PoolMiss::Deleted);
        assert_eq!(claude("ccs-claude-nobody--m"), PoolMiss::Unknown);
        assert_eq!(claude("ccs-claude-kimi"), PoolMiss::Unknown);
        // 名单为空（这个应用从没加过附加模型）也一样报错。
        assert_eq!(
            miss(resolve_in(&fx, AppType::Codex, "ccs-kimi/kimi-k3")),
            PoolMiss::Unknown
        );
        let message = PoolMiss::Removed.message("ccs-claude-gone--g-1");
        assert!(message.contains("ccs-claude-gone--g-1"), "{message}");
    }

    #[test]
    fn codex_ids_resolve_to_the_catalog_model_and_leave_the_routes_names_alone() {
        let fx = fixture();
        let mut deepseek = provider("ds", "DeepSeek", Some("deepseek"), json!({}));
        deepseek.settings_config = json!({
            "auth": {},
            "config": "model = \"deepseek-v4-flash\"\n",
            "modelCatalog": { "models": [{ "model": "deepseek-v4-pro" }] },
        });
        fx.db.save_provider("codex", &deepseek).unwrap();
        state::update(&fx.store, |live| {
            let pool = &mut live.apps.entry("codex".to_string()).or_default().pool;
            pool.members = vec!["ds".to_string()];
            pool.keys.insert("deepseek".to_string(), "ds".to_string());
        })
        .unwrap();

        assert_eq!(
            codex_models("deepseek", &deepseek)
                .into_iter()
                .map(|model| model.id)
                .collect::<Vec<_>>(),
            vec!["ccs-deepseek/deepseek-v4-pro"]
        );
        assert_eq!(
            hit(resolve_in(
                &fx,
                AppType::Codex,
                "ccs-deepseek/deepseek-v4-pro"
            )),
            (
                "ds".to_string(),
                "deepseek-v4-pro".to_string(),
                "ccs-deepseek/deepseek-v4-pro".to_string()
            )
        );
        // 路由那家自己的 `deepseek/...`（OpenRouter 写法）不带保留前缀，照旧走默认路由。
        assert!(matches!(
            resolve_in(&fx, AppType::Codex, "deepseek/deepseek-v4-pro"),
            Resolved::Plain
        ));
    }

    #[test]
    fn plain_models_do_not_read_any_state() {
        let fx = fixture();
        // 状态文件坏了也不影响普通请求：不带保留前缀时根本不读。
        std::fs::write(fx.store.state_path(), "{ not json").unwrap();
        for (app, model) in [
            (AppType::Claude, "claude-sonnet-5"),
            (AppType::Claude, "kimi-k3"),
            (AppType::Codex, "deepseek/deepseek-v4-pro"),
            (AppType::ClaudeDesktop, "ccs-claude-kimi--kimi-k3"),
        ] {
            assert!(
                matches!(resolve_in(&fx, app, model), Resolved::Plain),
                "{model}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(fx.store.state_path()).unwrap(),
            "{ not json"
        );
    }
}
