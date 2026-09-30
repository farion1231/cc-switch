//! 切换时 Codex 的 `auth.json` 怎么办。
//!
//! `auth.json` 只留给官方登录：第三方的 Key 写在 `config.toml` 的路由表里。切换时它有
//! 四种去向（不动、整份写、删掉、换成托管账号的登录），由 [`plan`] 按目标供应商、live
//! 里现在的登录和保留登录开关算出来，是个纯函数。
//!
//! **登录暂存。**保留登录开关关闭时，切到第三方要删掉 `auth.json`（兼容期维持 v3.20.1
//! 的行为），切回官方卡时再还回去。以前靠切走时把 live 回填进官方卡的行，现在不回填：
//! 回填会让 token 随云同步走，而且行里的快照不会跟着 Codex CLI 轮换，过几天就作废。
//! 改为把 CC Switch 删掉或覆盖掉的那份原生登录存在这台设备上
//! （`~/.cc-switch/codex-login-stash.json`，0600，不同步），切回官方卡时原样还回去。
//! 托管账号的登录在账号管理器里，不进暂存。
//!
//! 官方卡的行里存着的 OAuth 登录（旧版回填进去的）只在暂存文件第一次建立时读一次，
//! 作为暂存的初始内容；之后以暂存为准，行里的快照不再使用。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codex_config::{
    codex_auth_has_credential_login_material, codex_auth_has_login_material,
    codex_auth_has_openai_account_material, codex_live_auth_is_stale_third_party_residue,
    extract_codex_auth_api_key, extract_codex_auth_user_identity,
};

pub(crate) const STASH_FILENAME: &str = "codex-login-stash.json";

/// 这台设备上暂存的原生登录，按身份存。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct LoginStash {
    #[serde(default)]
    pub logins: BTreeMap<String, Value>,
    /// 最近一次暂存的是谁：切回没有自己登录的官方卡（行里 `auth` 为空）时还它。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// 暂存文件已经存在。为 `false` 时内容是刚从官方行里的快照初始化出来的，要写一次。
    #[serde(skip)]
    pub initialized: bool,
}

impl LoginStash {
    /// 暂存文件还不存在：用官方卡行里存着的 OAuth 登录初始化（旧版回填进去的）。
    pub fn seeded_from_rows<'a>(row_auths: impl IntoIterator<Item = &'a Value>) -> Self {
        let mut stash = Self::default();
        for auth in row_auths {
            if let Some(id) = oauth_identity(auth) {
                stash.logins.entry(id).or_insert_with(|| auth.clone());
            }
        }
        stash
    }

    fn put(&mut self, auth: &Value) {
        let id = identity(auth);
        self.logins.insert(id.clone(), auth.clone());
        self.last = Some(id);
    }

    fn take(&mut self, id: &str) -> Option<Value> {
        let auth = self.logins.remove(id)?;
        if self.last.as_deref() == Some(id) {
            self.last = None;
        }
        Some(auth)
    }
}

/// 登录的身份：id_token 里的用户，其次 ChatGPT workspace；API Key 登录共用一个位置。
fn identity(auth: &Value) -> String {
    if let Some(user) = extract_codex_auth_user_identity(auth) {
        return user;
    }
    if let Some(account) = auth
        .pointer("/tokens/account_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return format!("account:{account}");
    }
    if extract_codex_auth_api_key(auth).is_some() {
        return "api-key".to_string();
    }
    "unknown".to_string()
}

/// 行里存的是 OAuth 登录（不是 API Key）时，它的身份。
fn oauth_identity(auth: &Value) -> Option<String> {
    codex_auth_has_credential_login_material(auth).then(|| identity(auth))
}

/// 官方卡要 `auth.json` 里是谁：行里 OAuth 登录的身份，或 API Key 的摘要；行里没存登录
/// （跟随 Codex 当前的登录）时为空。算进代理契约：两张官方卡要的账号不同，契约就不同，
/// 代理模式下换路由才会像直连一样换登录。
pub(crate) fn official_login_requirement(row_auth: &Value) -> Option<String> {
    if is_api_key_credential(row_auth) {
        return extract_codex_auth_api_key(row_auth)
            .and_then(|key| crate::live::engine::digest(Some(key.as_bytes())))
            .map(|key| format!("api-key:{key}"));
    }
    oauth_identity(row_auth)
}

/// 官方卡的行里存的是 API Key（直连 OpenAI API）：静态凭据，不会过期，照写。
fn is_api_key_credential(auth: &Value) -> bool {
    extract_codex_auth_api_key(auth).is_some() && !codex_auth_has_credential_login_material(auth)
}

/// 旧版切到第三方时写进 `auth.json` 的 Key（只有 `OPENAI_API_KEY`，而且就是某个第三方
/// 供应商的 Key）。能证明是 CC Switch 写的才算残留；用户自己 `codex login --api-key`
/// 登录的 Key 不算。
fn is_residue(auth: &Value, third_party_keys: &[String]) -> bool {
    codex_live_auth_is_stale_third_party_residue(auth)
        && extract_codex_auth_api_key(auth).is_some_and(|key| third_party_keys.contains(&key))
}

/// 目标供应商对 `auth.json` 的要求。
#[derive(Debug, Clone, Copy)]
pub(crate) enum AuthTarget<'a> {
    /// 直连的第三方：保留登录开关关闭时删掉 `auth.json`。
    ThirdParty { preserve: bool },
    /// 代理的第三方路由：不动用户的原生登录（请求凭据由代理注入）。
    ProxyThirdParty,
    /// 没绑托管账号的官方卡（直连，或代理的官方路由）。
    Official { row_auth: &'a Value },
    /// 托管账号：整份写它的登录。
    Managed { auth: &'a Value },
}

pub(crate) struct AuthInput<'a> {
    /// live 的 `auth.json`（文件不存在为 `None`，解析不了的按非对象处理）。
    pub live: Option<&'a Value>,
    /// live 里是某个托管账号的登录（切走时要清掉，不进暂存）。
    pub live_is_managed: bool,
    /// 第三方供应商行里的 Key，用来认出旧版留下的残留。
    pub third_party_keys: &'a [String],
    /// 切走的是没绑托管账号的官方卡（它的行 `auth`）：live 里没有登录，说明用户在
    /// Codex 里登出了，暂存里对应的登录也作废，免得切回来又登上。
    pub leaving_official: Option<&'a Value>,
    pub target: AuthTarget<'a>,
    pub stash: LoginStash,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AuthPlan {
    /// `None` 不动；`Some(None)` 删掉；`Some(Some(v))` 整份写 `v`。
    pub auth: Option<Option<Value>>,
    /// 暂存有变化时的新内容。
    pub stash: Option<LoginStash>,
    /// 写完之后 `auth.json` 里有没有登录（第三方路由表的 `requires_openai_auth` 跟着它；
    /// 登录存在系统钥匙串里时另算，见 `codex_direct`）。
    pub login_on_disk: bool,
}

pub(crate) fn plan(input: AuthInput<'_>) -> AuthPlan {
    let original = input.stash.clone();
    let mut stash = input.stash;
    let managed = input.live_is_managed;
    let residue = input
        .live
        .is_some_and(|auth| !managed && is_residue(auth, input.third_party_keys));
    let native = input
        .live
        .filter(|auth| !managed && !residue && codex_auth_has_login_material(auth));

    if let Some(row_auth) = input.leaving_official {
        if native.is_none() && !managed {
            match oauth_identity(row_auth) {
                Some(id) => {
                    stash.take(&id);
                }
                None => {
                    if let Some(last) = stash.last.take() {
                        stash.logins.remove(&last);
                    }
                }
            }
            stash.last = None;
        }
    }

    let auth = match input.target {
        AuthTarget::ThirdParty { preserve } => {
            if let Some(live) = native.filter(|_| !preserve) {
                stash.put(live);
            }
            // 保留登录关闭时第三方路由旁边不留任何 auth.json（写 `{}` 不等于登出）。
            (managed || residue || (!preserve && input.live.is_some())).then_some(None)
        }
        AuthTarget::ProxyThirdParty => managed.then_some(None),
        AuthTarget::Managed { auth } => {
            if let Some(live) = native {
                stash.put(live);
            }
            Some(Some(auth.clone()))
        }
        AuthTarget::Official { row_auth } => {
            official(row_auth, native, managed || residue, &mut stash)
        }
    };

    // 只算 Codex 会拿来登录的东西（OAuth token、API Key 等），Bedrock 凭据和元数据不算。
    let login_on_disk = match &auth {
        None => native.is_some_and(codex_auth_has_openai_account_material),
        Some(None) => false,
        Some(Some(auth)) => codex_auth_has_openai_account_material(auth),
    };
    let changed = stash != original || !original.initialized;
    AuthPlan {
        auth,
        stash: changed.then_some(LoginStash {
            initialized: true,
            ..stash
        }),
        login_on_disk,
    }
}

/// 切到没绑托管账号的官方卡。`clearable` 表示 live 里是要清掉的东西（托管账号的登录、
/// 旧版的第三方 Key 残留）。
fn official(
    row_auth: &Value,
    native: Option<&Value>,
    clearable: bool,
    stash: &mut LoginStash,
) -> Option<Option<Value>> {
    if is_api_key_credential(row_auth) {
        if let Some(live) = native.filter(|live| *live != row_auth) {
            stash.put(live);
        }
        return Some(Some(row_auth.clone()));
    }

    let wanted = oauth_identity(row_auth);
    if let Some(live) = native {
        // 行里没存登录：跟随 Codex 当前的登录。存的就是当前这个人：不动。
        let id = wanted.filter(|id| identity(live) != *id)?;
        // 行里存的是另一个人：暂存里有他才换，否则不动（行里的快照可能早已作废）。
        let saved = stash.take(&id)?;
        stash.put(live);
        return Some(Some(saved));
    }

    let restored = match wanted {
        Some(id) => stash.take(&id),
        None => stash.last.clone().and_then(|id| stash.take(&id)),
    };
    match restored {
        Some(saved) => Some(Some(saved)),
        None => clearable.then_some(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn login(account: &str) -> Value {
        json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "id",
                "access_token": format!("access-{account}"),
                "refresh_token": format!("refresh-{account}"),
                "account_id": account,
            },
        })
    }

    fn input<'a>(
        live: Option<&'a Value>,
        target: AuthTarget<'a>,
        stash: LoginStash,
    ) -> AuthInput<'a> {
        AuthInput {
            live,
            live_is_managed: false,
            third_party_keys: &[],
            leaving_official: None,
            target,
            stash,
        }
    }

    fn ready() -> LoginStash {
        LoginStash {
            initialized: true,
            ..LoginStash::default()
        }
    }

    #[test]
    fn preservation_off_stashes_the_login_and_gives_it_back_on_the_way_to_official() {
        let alice = login("alice");
        let empty = json!({});

        let away = plan(input(
            Some(&alice),
            AuthTarget::ThirdParty { preserve: false },
            ready(),
        ));
        assert_eq!(away.auth, Some(None), "auth.json is deleted");
        assert!(!away.login_on_disk);
        let stash = away.stash.expect("login stashed");

        let back = plan(input(
            None,
            AuthTarget::Official { row_auth: &empty },
            stash,
        ));
        assert_eq!(back.auth, Some(Some(alice)), "the same login comes back");
        assert!(back.stash.expect("stash emptied").logins.is_empty());
    }

    #[test]
    fn preservation_on_leaves_the_login_alone() {
        let alice = login("alice");
        let away = plan(input(
            Some(&alice),
            AuthTarget::ThirdParty { preserve: true },
            ready(),
        ));
        assert_eq!(away.auth, None);
        assert!(away.login_on_disk);
        assert_eq!(away.stash, None);
    }

    #[test]
    fn logging_out_on_the_official_card_sticks() {
        let alice = login("alice");
        let empty = json!({});
        let mut stash = ready();
        stash.put(&alice);

        // 在官方卡上登出后切走：暂存里的登录作废。
        let away = plan(AuthInput {
            leaving_official: Some(&empty),
            ..input(None, AuthTarget::ThirdParty { preserve: false }, stash)
        });
        let stash = away.stash.expect("stash updated");
        assert!(stash.logins.is_empty());

        let back = plan(input(
            None,
            AuthTarget::Official { row_auth: &empty },
            stash,
        ));
        assert_eq!(back.auth, None, "stays logged out");
    }

    #[test]
    fn stale_row_snapshots_are_only_a_one_time_seed() {
        let alice = login("alice");
        let stash = LoginStash::seeded_from_rows([&alice]);
        assert!(!stash.initialized);

        // 升级前切到第三方时旧版删了 auth.json、把登录回填进了官方行：第一次切回官方时还回去。
        let back = plan(input(
            None,
            AuthTarget::Official { row_auth: &alice },
            stash,
        ));
        assert_eq!(back.auth, Some(Some(alice.clone())));
        let stash = back.stash.expect("stash file created");
        assert!(stash.initialized && stash.logins.is_empty());

        // 之后在 Codex 里登出，再切回这张卡：行里的旧快照不再使用。
        let again = plan(input(
            None,
            AuthTarget::Official { row_auth: &alice },
            stash,
        ));
        assert_eq!(again.auth, None);
    }

    #[test]
    fn switching_between_official_cards_of_different_accounts_swaps_logins() {
        let alice = login("alice");
        let bob = login("bob");
        let stash = LoginStash::seeded_from_rows([&alice, &bob]);

        let to_bob = plan(input(
            Some(&alice),
            AuthTarget::Official { row_auth: &bob },
            stash,
        ));
        assert_eq!(to_bob.auth, Some(Some(bob.clone())));
        let stash = to_bob.stash.unwrap();
        assert!(stash.logins.contains_key("account:alice"), "alice stashed");

        // bob 的登录在 live 里被 CLI 轮换过：切回 alice 时存下的是轮换后的那份。
        let mut rotated_bob = bob.clone();
        rotated_bob["tokens"]["refresh_token"] = json!("refresh-bob-2");
        let to_alice = plan(input(
            Some(&rotated_bob),
            AuthTarget::Official { row_auth: &alice },
            stash,
        ));
        assert_eq!(to_alice.auth, Some(Some(alice)));
        assert_eq!(
            to_alice.stash.unwrap().logins["account:bob"],
            rotated_bob,
            "the rotated login is what gets stashed"
        );
    }

    #[test]
    fn a_card_without_its_own_login_follows_the_live_one() {
        let alice = login("alice");
        let empty = json!({});
        let plan = plan(input(
            Some(&alice),
            AuthTarget::Official { row_auth: &empty },
            ready(),
        ));
        assert_eq!(plan.auth, None);
    }

    #[test]
    fn an_api_key_official_card_always_writes_its_key_and_stashes_the_login() {
        let alice = login("alice");
        let api = json!({ "OPENAI_API_KEY": "sk-openai" });
        let plan = plan(input(
            Some(&alice),
            AuthTarget::Official { row_auth: &api },
            ready(),
        ));
        assert_eq!(plan.auth, Some(Some(api)));
        assert!(plan.stash.unwrap().logins.contains_key("account:alice"));
    }

    #[test]
    fn only_provable_residue_is_deleted() {
        let empty = json!({});
        let keys = vec!["sk-relay".to_string()];
        let residue = json!({ "OPENAI_API_KEY": "sk-relay" });
        let own = json!({ "OPENAI_API_KEY": "sk-my-own-openai-key" });

        let cleared = plan(AuthInput {
            third_party_keys: &keys,
            ..input(
                Some(&residue),
                AuthTarget::Official { row_auth: &empty },
                ready(),
            )
        });
        assert_eq!(cleared.auth, Some(None), "old third-party key is removed");

        let kept = plan(AuthInput {
            third_party_keys: &keys,
            ..input(
                Some(&own),
                AuthTarget::Official { row_auth: &empty },
                ready(),
            )
        });
        assert_eq!(kept.auth, None, "the user's own API key login stays");
    }

    #[test]
    fn managed_logins_are_cleared_and_never_stashed() {
        let managed = login("managed");
        let empty = json!({});
        for target in [
            AuthTarget::ThirdParty { preserve: true },
            AuthTarget::ProxyThirdParty,
            AuthTarget::Official { row_auth: &empty },
        ] {
            let plan = plan(AuthInput {
                live_is_managed: true,
                ..input(Some(&managed), target, ready())
            });
            assert_eq!(plan.auth, Some(None), "{target:?}");
            assert_eq!(plan.stash, None, "{target:?}");
        }
    }

    #[test]
    fn the_proxy_never_touches_the_native_login() {
        let alice = login("alice");
        let plan = plan(input(Some(&alice), AuthTarget::ProxyThirdParty, ready()));
        assert_eq!(plan.auth, None);
        assert!(plan.login_on_disk);
    }
}
