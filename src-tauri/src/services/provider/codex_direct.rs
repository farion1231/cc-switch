//! 写 Codex 的客户端文件：`config.toml` 只替换关键字段和独有字段，其余字节不碰；
//! `auth.json`、模型目录、托管账号的登录标记、登录暂存和它在同一个操作里提交，崩溃后按
//! pending 前滚或丢弃。
//!
//! 写 Codex live 的入口（切换、新增第一个供应商、编辑当前供应商、同步、统一供应商、
//! 进入 / 退出代理）都走这里。不回填、不合并通用配置片段、不补回 MCP：这些设置本来就
//! 留在 live 里。
//!
//! 分三步：
//! 1. [`prepare`]：拿写锁之前做要联网的事（取托管账号的 token、采纳 Codex CLI 轮换过的
//!    refresh token）；
//! 2. [`plan`]：在内存里算出 `config.toml` 的补丁、模型目录和代理契约，行有问题就在这里
//!    报错，什么都不写；
//! 3. [`run`]：拿写锁，读 live 的 `auth.json` 决定它的去向（见 `codex_login`），再按
//!    盘上有没有登录定下路由表的 `requires_openai_auth`，一起提交。

use serde_json::{Map, Value};
use toml_edit::{Item, Table, Value as TomlValue};

use crate::app_config::AppType;
use crate::codex_config::{
    codex_auth_has_credential_login_material, codex_config_auth_store_mode,
    codex_disables_web_search, codex_live_auth_is_managed_chatgpt_login,
    codex_managed_oauth_marker_bytes, extract_codex_auth_api_key, get_codex_auth_path,
    get_codex_config_path, get_codex_managed_oauth_live_auth_marker_path,
    get_codex_model_catalog_path, plan_codex_model_catalog, CodexAuthStoreMode,
};
use crate::config::sorted_json_bytes;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{digest, lock_app, read_current, DeviceStore, LiveFile};
use crate::live::patch::{Guarded, LivePatch, WholeFile};
use crate::live::project::codex::{
    official_mirror_table, proxy_route_table, requires_openai_auth, CodexConfigPatch,
    CodexProjection, KnownTable, Route, RouteAuth, RouteWrite, RowInput, ROUTE_ID,
    WEB_SEARCH_DISABLED,
};
use crate::mode::contract::CONTRACT_VERSION;
use crate::mode::operation::{self, FileChange, OperationReport};
use crate::mode::state::{Contract, PendingTarget};
use crate::provider::Provider;
use crate::proxy::providers::codex_oauth_auth::CodexLiveAuthSwitchGuard;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use std::sync::Arc;

use super::codex_login::{self, AuthInput, AuthTarget, LoginStash, STASH_FILENAME};
use super::ProviderService;

fn app() -> &'static str {
    AppType::Codex.as_str()
}

/// 官方卡：`category == "official"`，或按 `is_codex_official_provider` 认出来的（早期
/// 绑定托管账号时没存 category 的卡）。
pub(crate) fn is_official(provider: &Provider) -> bool {
    provider.category.as_deref() == Some("official")
        || crate::proxy::providers::is_codex_official_provider(provider)
}

fn managed_account(provider: &Provider) -> Option<String> {
    ProviderService::managed_codex_oauth_account_id(provider)
}

/// 本地代理给 Codex 的地址（带 `/v1`），不需要代理在运行：官方直连时写休眠表用。
pub(crate) fn configured_proxy_base_url(db: &Database) -> String {
    let (address, port) = db.get_proxy_listen_sync();
    let host = match address.as_str() {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" => "[::1]".to_string(),
        other if other.contains(':') && !other.starts_with('[') => format!("[{other}]"),
        other => other.to_string(),
    };
    let port = if port == 0 {
        crate::proxy::types::ProxyConfig::default().listen_port
    } else {
        port
    };
    format!("http://{host}:{port}/v1")
}

/// 写成什么样。
#[derive(Clone, Copy)]
pub(crate) enum Target<'a> {
    /// 直连：这个供应商（`None`：没有直连供应商，只清掉关键字段）。
    Direct(Option<&'a Provider>),
    /// 代理契约：路由供应商；`base_url` 是本地代理给 Codex 的地址（带 `/v1`）。
    Proxy {
        route: &'a Provider,
        base_url: &'a str,
    },
}

/// live 现在是谁写进去的：删它带进来的独有字段、认出要切走的托管账号、判断用户是不是
/// 登出了，都看它。
#[derive(Clone, Copy)]
pub(crate) enum Owner<'a> {
    Provider(&'a Provider),
    /// 代理契约，`route` 是契约对应的路由供应商（找得到时）。
    Contract {
        contract: &'a Contract,
        route: Option<&'a Provider>,
    },
    None,
}

impl<'a> Owner<'a> {
    fn provider(&self) -> Option<&'a Provider> {
        match self {
            Self::Provider(provider) => Some(provider),
            Self::Contract { route, .. } => *route,
            Self::None => None,
        }
    }
}

/// 拿写锁之前准备好的托管账号凭据。
#[derive(Default)]
pub(crate) struct Prepared {
    /// 目标托管账号和它的登录。
    target_login: Option<(String, Value)>,
    /// 要切走的托管账号，和采纳 CLI 轮换后记下的盘上 refresh token。
    outgoing: Option<(String, CodexLiveAuthSwitchGuard)>,
}

fn target_provider<'a>(target: &Target<'a>) -> Option<&'a Provider> {
    match target {
        Target::Direct(provider) => *provider,
        Target::Proxy { route, .. } => Some(route),
    }
}

fn target_account(target: &Target<'_>) -> Option<String> {
    target_provider(target)
        .filter(|provider| is_official(provider))
        .and_then(managed_account)
}

/// 取目标托管账号的登录（必要时刷新 token），并在切走托管账号前采纳 Codex CLI 轮换过的
/// refresh token。都可能联网，所以在拿写锁之前做。
pub(crate) fn prepare(
    manager: &Arc<CodexOAuthManager>,
    owner: &Owner<'_>,
    target: &Target<'_>,
) -> Result<Prepared, AppError> {
    let target_account = target_account(target);
    let target_login = match &target_account {
        Some(account) => Some((
            account.clone(),
            super::live::get_codex_managed_oauth_live_auth_value(manager.clone(), account.clone())?,
        )),
        None => None,
    };
    let outgoing = match owner
        .provider()
        .and_then(managed_account)
        .filter(|account| target_account.as_ref() != Some(account))
    {
        Some(account) => {
            let guard = super::live::prepare_codex_managed_oauth_live_auth_switch_away(
                manager.clone(),
                account.clone(),
            )?;
            Some((account, guard))
        }
        None => None,
    };
    Ok(Prepared {
        target_login,
        outgoing,
    })
}

fn project(provider: &Provider) -> Result<CodexProjection, AppError> {
    CodexProjection::of(&RowInput {
        settings: &provider.settings_config,
        official: is_official(provider),
        proxy_injected_oauth: provider.uses_proxy_injected_oauth(),
    })
}

/// 这个供应商的独有字段，含 `web_search`（需要时为 `"disabled"`）。
fn exclusive_of(provider: &Provider, projection: &CodexProjection) -> Vec<(String, TomlValue)> {
    let mut exclusive = projection.exclusive.clone();
    let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(provider);
    if codex_disables_web_search(
        &provider.settings_config,
        &projection.catalog_input_text(),
        profile,
    ) {
        exclusive.retain(|(key, _)| key != "web_search");
        exclusive.push((
            "web_search".to_string(),
            TomlValue::from(WEB_SEARCH_DISABLED),
        ));
    }
    exclusive
}

/// live 现在的独有字段：切走时值还相同就删。
fn outgoing_exclusive(owner: &Owner<'_>) -> Vec<(String, TomlValue)> {
    match owner {
        Owner::Provider(provider) => match project(provider) {
            Ok(projection) => exclusive_of(provider, &projection),
            Err(err) => {
                log::warn!(
                    "无法投影 Codex 供应商 {} 的独有字段，切走时不清理它们: {err}",
                    provider.id
                );
                Vec::new()
            }
        },
        Owner::Contract { contract, .. } => contract
            .exclusive
            .iter()
            .filter_map(|(key, value)| {
                let literal = value.as_str()?.parse::<TomlValue>().ok()?;
                Some((key.clone(), literal))
            })
            .collect(),
        Owner::None => Vec::new(),
    }
}

/// 数据库里所有 Codex 行能证明的事：它们的投影写过哪些表、第三方的 Key、官方卡里存着的
/// 登录（登录暂存第一次建立时用）。
struct RowFacts {
    retired: Vec<KnownTable>,
    third_party_keys: Vec<String>,
    official_logins: Vec<Value>,
}

fn row_facts(db: &Database) -> Result<RowFacts, AppError> {
    let mut facts = RowFacts {
        retired: Vec::new(),
        third_party_keys: Vec::new(),
        official_logins: Vec::new(),
    };
    for provider in db.get_all_providers(app())?.values() {
        let auth = provider.settings_config.get("auth");
        if is_official(provider) {
            if managed_account(provider).is_none() {
                if let Some(auth) =
                    auth.filter(|auth| codex_auth_has_credential_login_material(auth))
                {
                    facts.official_logins.push(auth.clone());
                }
            }
            continue;
        }
        if let Some(key) = auth.and_then(extract_codex_auth_api_key) {
            facts.third_party_keys.push(key);
        }
        let Some(doc) = provider
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        else {
            continue;
        };
        let providers = doc.get("model_providers").and_then(Item::as_table_like);
        let base_url_of = |id: &str| {
            providers
                .and_then(|table| table.get(id))
                .and_then(Item::as_table_like)
                .and_then(|table| table.get("base_url"))
                .and_then(Item::as_str)
                .map(|url| url.trim().to_string())
        };
        // 旧版整份写入时，路由表用的是行自己的 id（custom 是 CC Switch 现在写的，不算）。
        let selector = doc
            .get("model_provider")
            .and_then(Item::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty() && *id != ROUTE_ID);
        if let Some((id, base_url)) = selector.and_then(|id| Some((id, base_url_of(id)?))) {
            facts.retired.push(KnownTable {
                id: id.to_string(),
                base_url,
            });
        }
        if let Some(base_url) = doc
            .get("openai_base_url")
            .and_then(Item::as_str)
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty())
        {
            facts.retired.push(KnownTable {
                id: "cc-switch".to_string(),
                base_url,
            });
        }
    }
    Ok(facts)
}

/// `auth.json` 的去向（拥有所有权的版本，[`run`] 里转成 `codex_login::AuthTarget`）。
#[derive(Debug, Clone)]
enum AuthGoal {
    ThirdParty,
    ProxyThirdParty,
    Official(Value),
    Managed(Value),
    /// 没有直连供应商：只清托管账号的登录。
    Keep,
}

/// `config.toml` 的补丁：先应用编辑器里的全局改动（有的话），再换关键字段。
struct ConfigWithEdits<'a> {
    edits: Option<&'a super::codex_editor::CodexEdits>,
    key_fields: &'a CodexConfigPatch,
}

impl LivePatch for ConfigWithEdits<'_> {
    fn apply(
        &self,
        path: &std::path::Path,
        pre: Option<&[u8]>,
    ) -> Result<Vec<u8>, crate::live::patch::LiveWriteError> {
        let mut doc = crate::live::patch::toml::parse(path, pre)?;
        if let Some(edits) = self.edits {
            edits.apply_to(path, &mut doc)?;
        }
        self.key_fields.apply_to(path, &mut doc)?;
        Ok(doc.to_string().into_bytes())
    }
}

/// 在内存里算好的一次 Codex 写入。
pub(crate) struct Planned {
    config: CodexConfigPatch,
    /// 第三方路由表的凭据来源：写入时按盘上有没有登录定 `requires_openai_auth`。
    stamp: Option<RouteAuth>,
    catalog: Option<Vec<u8>>,
    auth: AuthGoal,
    /// 切走的是没绑托管账号的官方卡：它行里的 `auth`。
    leaving_official: Option<Value>,
    retired_keys: Vec<String>,
    official_logins: Vec<Value>,
    /// 代理契约（直连时也算，没有用处）。
    pub contract: Contract,
}

impl Planned {
    /// `config.toml` 的补丁（编辑器显示用：在内存里对 live 做一次切换投影）。
    pub(crate) fn config(&self) -> &CodexConfigPatch {
        &self.config
    }
}

/// 算出写入内容；行有问题（比如会把官方登录发给第三方）就在这里报错，什么都不写。
pub(crate) fn plan(
    db: &Database,
    owner: &Owner<'_>,
    target: &Target<'_>,
    prepared: &Prepared,
) -> Result<Planned, AppError> {
    let facts = row_facts(db)?;
    let provider = target_provider(target);
    let projection = provider.map(project).transpose()?;

    let (top, nested, exclusive) = match (&projection, provider) {
        (Some(projection), Some(provider)) => (
            projection.top.clone(),
            projection.nested.clone(),
            exclusive_of(provider, projection),
        ),
        _ => (Vec::new(), Vec::new(), Vec::new()),
    };

    let official = provider.is_some_and(is_official);
    let managed_login = prepared.target_login.as_ref().map(|(_, auth)| auth.clone());
    let (route, stamp, auth) = match (target, &projection) {
        (Target::Direct(None), _) | (_, None) => (RouteWrite::Default, None, AuthGoal::Keep),
        (Target::Direct(Some(provider)), Some(projection)) => {
            let auth = match &managed_login {
                Some(login) => AuthGoal::Managed(login.clone()),
                None if official => AuthGoal::Official(
                    provider
                        .settings_config
                        .get("auth")
                        .cloned()
                        .unwrap_or_else(|| Value::Object(Map::new())),
                ),
                None => AuthGoal::ThirdParty,
            };
            match &projection.route {
                Route::Official if crate::settings::unify_codex_session_history() => {
                    (RouteWrite::OfficialMirror, None, auth)
                }
                Route::Official => (
                    RouteWrite::Official {
                        dormant_base_url: configured_proxy_base_url(db),
                    },
                    None,
                    auth,
                ),
                Route::Custom { table, auth: kind } => {
                    (RouteWrite::Custom(table.clone()), Some(*kind), auth)
                }
                Route::BuiltIn { id, table } => (
                    RouteWrite::BuiltIn {
                        id: id.clone(),
                        table: table.clone(),
                    },
                    None,
                    auth,
                ),
                Route::Default => (RouteWrite::Default, None, auth),
            }
        }
        (Target::Proxy { route, base_url }, Some(_)) => {
            if official {
                let auth = match &managed_login {
                    Some(login) => AuthGoal::Managed(login.clone()),
                    None => AuthGoal::Official(
                        route
                            .settings_config
                            .get("auth")
                            .cloned()
                            .unwrap_or_else(|| Value::Object(Map::new())),
                    ),
                };
                (
                    RouteWrite::OfficialProxy(official_mirror_table(Some(base_url), false)),
                    None,
                    auth,
                )
            } else {
                (
                    RouteWrite::Custom(proxy_route_table(ROUTE_ID, base_url, false)),
                    Some(RouteAuth::Bearer),
                    AuthGoal::ProxyThirdParty,
                )
            }
        }
    };

    let catalog_plan = match (provider, &projection) {
        (Some(provider), Some(projection)) => Some(plan_codex_model_catalog(
            &provider.settings_config,
            &projection.catalog_input_text(),
            crate::proxy::providers::resolve_codex_catalog_tool_profile(provider),
        )?),
        _ => None,
    };
    let catalog = catalog_plan
        .and_then(|plan| plan.catalog)
        .map(|catalog| sorted_json_bytes(&catalog))
        .transpose()?;

    let leaving_official = owner
        .provider()
        .filter(|provider| is_official(provider) && managed_account(provider).is_none())
        .map(|provider| {
            provider
                .settings_config
                .get("auth")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()))
        });

    let config = CodexConfigPatch {
        top,
        nested,
        exclusive,
        outgoing: outgoing_exclusive(owner),
        route,
        catalog: catalog.is_some(),
        retired: facts.retired,
    };
    let contract = contract_of(target, &config, catalog.as_deref(), prepared);
    Ok(Planned {
        config,
        stamp,
        catalog,
        auth,
        leaving_official,
        retired_keys: facts.third_party_keys,
        official_logins: facts.official_logins,
        contract,
    })
}

fn value_literal(value: &TomlValue) -> String {
    let mut value = value.clone();
    value.decor_mut().clear();
    value.to_string()
}

fn table_text(table: &Table) -> String {
    let mut table = table.clone();
    table.remove("requires_openai_auth");
    let mut doc = toml_edit::DocumentMut::new();
    doc.insert("t", Item::Table(table));
    doc.to_string()
}

/// 代理契约：路由供应商在客户端那一侧的全部要求。摘要相同，换路由时客户端文件就不读
/// 也不写。`requires_openai_auth` 跟着盘上的登录走，不算进契约。
fn contract_of(
    target: &Target<'_>,
    config: &CodexConfigPatch,
    catalog: Option<&[u8]>,
    prepared: &Prepared,
) -> Contract {
    let base_url = match target {
        Target::Proxy { base_url, .. } => *base_url,
        Target::Direct(_) => "",
    };
    let (selector, table) = match &config.route {
        RouteWrite::Custom(table) => (ROUTE_ID, table_text(table)),
        RouteWrite::OfficialProxy(table) => (
            crate::live::project::codex::OFFICIAL_PROXY_ROUTE_ID,
            table_text(table),
        ),
        _ => ("", String::new()),
    };
    let pairs = |entries: &[(String, TomlValue)]| -> Vec<Value> {
        let mut pairs: Vec<Value> = entries
            .iter()
            .map(|(key, value)| serde_json::json!([key, value_literal(value)]))
            .collect();
        pairs.sort_by_key(|pair| pair[0].as_str().unwrap_or_default().to_string());
        pairs
    };
    let nested: Vec<Value> = config
        .nested
        .iter()
        .map(|(path, value)| serde_json::json!([path.join("."), value_literal(value)]))
        .collect();
    let parts = serde_json::json!({
        "app": "codex",
        "version": CONTRACT_VERSION,
        "url": base_url,
        "top": pairs(&config.top),
        "nested": nested,
        "exclusive": pairs(&config.exclusive),
        "selector": selector,
        "table": table,
        "catalog": digest(catalog),
        "managed": prepared.target_login.as_ref().map(|(account, _)| account),
    });
    let key = digest(Some(
        &serde_json::to_vec(&parts).expect("contract parts serialize"),
    ))
    .expect("digest");
    Contract {
        version: CONTRACT_VERSION,
        key,
        exclusive: config
            .exclusive
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value_literal(value))))
            .collect(),
    }
}

fn load_stash(store: &DeviceStore, official_logins: &[Value]) -> (LoginStash, Option<Vec<u8>>) {
    let path = store.file(STASH_FILENAME);
    let pre = read_current(&path).ok().flatten();
    let stash = match pre.as_deref().map(serde_json::from_slice::<LoginStash>) {
        Some(Ok(stash)) => LoginStash {
            initialized: true,
            ..stash
        },
        Some(Err(err)) => {
            log::warn!("Codex 登录暂存无法解析，重新开始: {err}");
            LoginStash::default()
        }
        None => LoginStash::seeded_from_rows(official_logins),
    };
    (stash, pre)
}

fn guarded(pre: Option<&[u8]>, then: WholeFile) -> Guarded {
    Guarded {
        expected_pre: digest(pre),
        then,
    }
}

/// 执行一次 Codex 写入：拿写锁，决定 `auth.json` 的去向，和 `config.toml`、模型目录、
/// 托管账号标记、登录暂存一起提交，最后落定 `pending`（指针、模式状态）。
pub(crate) fn run(
    db: &Database,
    op: &str,
    planned: Planned,
    prepared: &Prepared,
    pending: PendingTarget,
) -> Result<OperationReport, AppError> {
    run_with_edits(db, op, planned, prepared, pending, None)
}

/// 同 [`run`]，另把编辑器里对全局设置的改动在同一次 `config.toml` 写入里应用。
pub(crate) fn run_with_edits(
    db: &Database,
    op: &str,
    planned: Planned,
    prepared: &Prepared,
    pending: PendingTarget,
    edits: Option<&super::codex_editor::CodexEdits>,
) -> Result<OperationReport, AppError> {
    let guard = lock_app(app());
    let store = DeviceStore::for_device();

    // 切走的托管账号：采纳之后 CLI 又刷新了就停下，免得删掉新 token。
    if let Some((account, outgoing)) = &prepared.outgoing {
        outgoing.ensure_unchanged(account)?;
    }

    // auth.json 读不了（比如被换成了目录）：不碰它，其余照常写。
    let auth_path = get_codex_auth_path();
    let (auth_pre, auth_readable) = match read_current(&auth_path) {
        Ok(pre) => (pre, true),
        Err(err) => {
            log::warn!("读取 Codex auth.json 失败，这次不改动它: {err}");
            (None, false)
        }
    };
    let live_auth = auth_pre
        .as_deref()
        .map(|bytes| serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null));
    // 已被删除的托管账号放弃了所有权：它留在盘上的登录按用户自己的登录处理。
    let managed_accounts: Vec<&str> = prepared
        .outgoing
        .iter()
        .filter(|(_, guard)| !matches!(guard, CodexLiveAuthSwitchGuard::MissingAccount))
        .map(|(account, _)| account.as_str())
        .chain(
            prepared
                .target_login
                .iter()
                .map(|(account, _)| account.as_str()),
        )
        .collect();
    let live_is_managed = live_auth.as_ref().is_some_and(|auth| {
        managed_accounts
            .iter()
            .any(|account| codex_live_auth_is_managed_chatgpt_login(auth, account))
    });
    let outgoing_missing = prepared
        .outgoing
        .as_ref()
        .is_some_and(|(_, guard)| matches!(guard, CodexLiveAuthSwitchGuard::MissingAccount));

    let (stash, stash_pre) = load_stash(&store, &planned.official_logins);
    let preserve = crate::settings::preserve_codex_official_auth_on_switch();
    let target = match &planned.auth {
        AuthGoal::ThirdParty => AuthTarget::ThirdParty { preserve },
        AuthGoal::ProxyThirdParty | AuthGoal::Keep => AuthTarget::ProxyThirdParty,
        AuthGoal::Official(row_auth) => AuthTarget::Official { row_auth },
        AuthGoal::Managed(auth) => AuthTarget::Managed { auth },
    };
    let auth_plan = codex_login::plan(AuthInput {
        live: live_auth.as_ref(),
        live_is_managed,
        third_party_keys: &planned.retired_keys,
        leaving_official: planned.leaving_official.as_ref(),
        target,
        stash,
    });

    // Codex 把登录存在哪由 `cli_auth_credentials_store` 决定：只存 auth.json 时看它；
    // 存在系统钥匙串（keyring、auto）或认不出时看不到登录，直连按保留登录开关、代理按
    // 「登录不动」处理；ephemeral 从不落盘，当成没登录。
    let login = match codex_config_auth_store_mode(
        &read_current(&get_codex_config_path())
            .ok()
            .flatten()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default(),
    ) {
        CodexAuthStoreMode::File => auth_plan.login_on_disk,
        CodexAuthStoreMode::Ephemeral => false,
        CodexAuthStoreMode::Keyring | CodexAuthStoreMode::Auto | CodexAuthStoreMode::Unknown => {
            !matches!(planned.auth, AuthGoal::ThirdParty) || preserve
        }
    };
    let mut config = planned.config;
    if let (Some(kind), RouteWrite::Custom(table)) = (planned.stamp, &mut config.route) {
        if matches!(kind, RouteAuth::Bearer | RouteAuth::EnvKey) {
            table.insert(
                "requires_openai_auth",
                toml_edit::value(requires_openai_auth(kind, login)),
            );
        }
    }

    let auth_patch = auth_plan
        .auth
        .clone()
        .filter(|_| auth_readable)
        .map(|auth| {
            let then = match auth {
                Some(auth) => sorted_json_bytes(&auth).map(WholeFile::Write),
                None => Ok(WholeFile::Delete),
            };
            then.map(|then| guarded(auth_pre.as_deref(), then))
        });
    let auth_patch = auth_patch.transpose()?;

    let marker_path = get_codex_managed_oauth_live_auth_marker_path();
    let marker_pre = read_current(&marker_path)?;
    let marker_patch = match (&prepared.target_login, &auth_plan.auth) {
        (Some((account, login)), Some(Some(written))) if written == login => {
            codex_managed_oauth_marker_bytes(login, account)?
                .map(|bytes| guarded(marker_pre.as_deref(), WholeFile::Write(bytes)))
        }
        _ if marker_pre.is_some()
            && ((live_is_managed && auth_plan.auth == Some(None)) || outgoing_missing) =>
        {
            Some(guarded(marker_pre.as_deref(), WholeFile::Delete))
        }
        _ => None,
    };

    let stash_patch = auth_plan
        .stash
        .as_ref()
        .map(|stash| {
            serde_json::to_vec_pretty(stash)
                .map(|bytes| guarded(stash_pre.as_deref(), WholeFile::Write(bytes)))
                .map_err(|e| AppError::Message(format!("序列化 Codex 登录暂存失败: {e}")))
        })
        .transpose()?;
    let catalog_patch = planned.catalog.map(WholeFile::Write);
    let config_patch = ConfigWithEdits {
        edits,
        key_fields: &config,
    };

    let mut changes: Vec<FileChange<'_>> = Vec::new();
    // auth.json 放第一个：Codex CLI 恰好在这时刷新了登录，就在发布任何文件之前停下。
    if let Some(patch) = &auth_patch {
        changes.push(FileChange {
            file: LiveFile::private(&auth_path),
            patch: patch as &dyn LivePatch,
        });
    }
    changes.push(FileChange {
        file: LiveFile::private(get_codex_config_path()),
        patch: &config_patch,
    });
    if let Some(patch) = &catalog_patch {
        changes.push(FileChange {
            file: LiveFile::shared(get_codex_model_catalog_path()),
            patch,
        });
    }
    if let Some(patch) = &marker_patch {
        changes.push(FileChange {
            file: LiveFile::private(&marker_path),
            patch,
        });
    }
    if let Some(patch) = &stash_patch {
        changes.push(FileChange {
            file: LiveFile::private(store.file(STASH_FILENAME)),
            patch,
        });
    }

    operation::run(&store, &guard, op, &changes, pending, &|target| {
        operation::commit_target(db, &store, app(), target)
    })
}

/// 直连写入：`prepare` → `plan` → `run`。
pub(crate) fn write_direct(
    db: &Database,
    manager: &Arc<CodexOAuthManager>,
    op: &str,
    owner: Owner<'_>,
    target: Option<&Provider>,
    pending: PendingTarget,
) -> Result<OperationReport, AppError> {
    let target = Target::Direct(target);
    let prepared = prepare(manager, &owner, &target)?;
    let planned = plan(db, &owner, &target, &prepared)?;
    run(db, op, planned, &prepared, pending)
}

/// 只校验，不写：切换前用它挡住会被拒绝的目标（行有问题时指针不能先动）。
pub(crate) fn preflight(db: &Database, provider: &Provider) -> Result<(), AppError> {
    let target = Target::Direct(Some(provider));
    plan(db, &Owner::None, &target, &Prepared::default()).map(|_| ())
}

/// 上一次 Codex 写入还有没补完的部分（文件已经发布、状态还没落定）。
pub(crate) fn has_pending() -> bool {
    crate::mode::state::pending(&DeviceStore::for_device(), app())
        .ok()
        .flatten()
        .is_some()
}
