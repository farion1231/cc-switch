//! GitHub Copilot CLI 的卡片只用来打开终端，没有「启用」：官方卡按会话传 `--model`，自带
//! 模型的 Key 不进任何文件，只在 CC Switch 打开的终端里经环境变量传给 copilot（见
//! `copilot_config` 模块说明）。

use serde_json::Value;

use crate::copilot_config::{self, CopilotLaunch, CopilotProjection};
use crate::error::AppError;
use crate::provider::Provider;

/// 授权中心里 GitHub 账号的托管标识，和 Copilot 代理供应商共用。
pub(crate) const ACCOUNT_AUTH_PROVIDER: &str = "github_copilot";

/// 官方登录卡：分类标为官方，或者根本没填自带模型地址。供应商名称可编辑，不参与判断。
pub(crate) fn is_official(provider: &Provider) -> bool {
    provider.category.as_deref() == Some("official")
        || !CopilotProjection::of(&provider.settings_config).is_byok()
}

/// 官方卡去掉所有自带模型字段，只留模型。
fn launch_settings(provider: &Provider) -> Value {
    let mut settings = provider.settings_config.clone();
    if provider.category.as_deref() == Some("official") {
        if let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) {
            for key in copilot_config::BYOK_ENV_KEYS {
                env.remove(*key);
            }
        }
    }
    settings
}

/// 在终端里启动这张卡要用的参数。
pub(crate) fn terminal_launch(provider: &Provider) -> Result<CopilotLaunch, AppError> {
    copilot_config::prepare_launch(&launch_settings(provider))
}

/// 官方卡绑定的授权中心 GitHub 账号。
pub(crate) fn bound_account_id(provider: &Provider) -> Option<String> {
    if !is_official(provider) {
        return None;
    }
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for(ACCOUNT_AUTH_PROVIDER))
        .filter(|id| !id.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(settings: Value) -> Provider {
        Provider::with_id("test".into(), "GitHub API key".into(), settings, None)
    }

    #[test]
    fn provider_names_do_not_decide_the_login_mode() {
        let row = provider(json!({"env":{
            "COPILOT_PROVIDER_BASE_URL":"https://example.test/v1",
            "COPILOT_PROVIDER_API_KEY":"fixture",
            "COPILOT_MODEL":"gpt-5.4"
        }}));
        assert!(!is_official(&row));
        let launch = terminal_launch(&row).unwrap();
        assert!(launch.byok);

        let mut official = row;
        official.category = Some("official".into());
        assert!(is_official(&official));
        let launch = terminal_launch(&official).unwrap();
        assert!(!launch.byok);
        assert_eq!(launch.model.as_deref(), Some("gpt-5.4"));
        assert!(launch.env.iter().all(|(_, value)| value.is_none()));
    }

    #[test]
    fn model_only_cards_use_the_github_login() {
        let row = provider(json!({"env":{"COPILOT_MODEL":"claude-sonnet-4.6"}}));
        assert!(is_official(&row));
        assert_eq!(bound_account_id(&row), None);
    }
}
