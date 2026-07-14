use std::path::Path;

use anyhow::Context;

use crate::relay_config::{
    backfill_relay_profile_from_home_with_common, relay_config_status_from_home,
};
use crate::settings::{BackendSettings, RelayMode, SettingsStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelaySwitchResult {
    pub settings: BackendSettings,
    pub configured: bool,
    pub backup_path: Option<String>,
}

pub fn switch_relay_profile_in_home(
    store: &SettingsStore,
    home: &Path,
    next_settings: BackendSettings,
    previous_active_relay_id: &str,
) -> anyhow::Result<RelaySwitchResult> {
    let mut selected_settings = next_settings;
    if !selected_settings.relay_profiles_enabled {
        anyhow::bail!("供应商配置总开关已关闭，未写入 config.toml / auth.json。");
    }

    let original_settings = store.load().unwrap_or_default();
    if !previous_active_relay_id.trim().is_empty()
        && previous_active_relay_id != selected_settings.active_relay_id
    {
        backfill_profile_before_switch(home, &mut selected_settings, previous_active_relay_id)?;
    }

    store
        .save(&selected_settings)
        .context("保存供应商设置失败")?;
    let selected_settings = store.load().context("读取供应商设置失败")?;

    match apply_selected_relay_profile(home, &selected_settings) {
        Ok(result) => Ok(result),
        Err(error) => {
            let _ = store.save(&original_settings);
            Err(error)
        }
    }
}

fn backfill_profile_before_switch(
    home: &Path,
    settings: &mut BackendSettings,
    previous_active_relay_id: &str,
) -> anyhow::Result<()> {
    let profile = settings
        .relay_profiles
        .iter_mut()
        .find(|profile| profile.id == previous_active_relay_id)
        .with_context(|| "当前供应商已不在配置列表中，已停止切换以避免覆盖用户改动。")?;
    backfill_relay_profile_from_home_with_common(
        home,
        profile,
        &mut settings.relay_context_config_contents,
    )
    .with_context(|| "回填当前供应商配置失败")
}

fn apply_selected_relay_profile(
    home: &Path,
    settings: &BackendSettings,
) -> anyhow::Result<RelaySwitchResult> {
    let relay = settings.active_relay_profile();
    let common_config = relay_combined_common_config(settings);
    let result = if relay.relay_mode == RelayMode::Official && !relay.official_mix_api_key {
        let auth_contents =
            (!relay.auth_contents.trim().is_empty()).then_some(relay.auth_contents.as_str());
        crate::relay_config::clear_relay_config_to_home_with_auth_and_computer_use_guard(
            home,
            auth_contents,
            settings.computer_use_guard_enabled,
        )?
    } else {
        validate_switch_profile_files(&relay)?;
        crate::relay_config::apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
            home,
            &relay,
            &common_config,
            settings.computer_use_guard_enabled,
        )?
    };
    let status = relay_config_status_from_home(home);
    if relay.relay_mode == RelayMode::PureApi && !status.configured {
        anyhow::bail!(
            "纯 API 配置写入后未检测到完整 custom provider，请检查 config.toml 和供应商 API Key。"
        );
    }
    Ok(RelaySwitchResult {
        settings: settings.clone(),
        configured: status.configured,
        backup_path: result.backup_path,
    })
}

fn validate_switch_profile_files(profile: &crate::settings::RelayProfile) -> anyhow::Result<()> {
    if profile.relay_mode != RelayMode::Aggregate && profile.config_contents.trim().is_empty() {
        anyhow::bail!(
            "供应商「{}」缺少独立 config.toml，已停止切换，避免继续显示上一套配置文件。",
            if profile.name.trim().is_empty() {
                profile.id.as_str()
            } else {
                profile.name.as_str()
            }
        );
    }
    if profile.relay_mode == RelayMode::Official
        && serde_json::from_str::<serde_json::Value>(&profile.auth_contents)
            .ok()
            .and_then(|value| {
                value
                    .get("OPENAI_API_KEY")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .map(str::is_empty)
            })
            == Some(false)
    {
        anyhow::bail!(
            "官方混合 API 不应在 auth.json 中保存 OPENAI_API_KEY。请清理此供应商的 auth.json 后再切换。"
        );
    }
    Ok(())
}

fn relay_combined_common_config(settings: &BackendSettings) -> String {
    let sections = [
        settings.relay_common_config_contents.trim(),
        settings.relay_context_config_contents.trim(),
    ]
    .into_iter()
    .filter(|section| !section.is_empty())
    .collect::<Vec<_>>();
    if sections.is_empty() {
        String::new()
    } else {
        crate::relay_config::normalize_config_text(&format!("{}\n", sections.join("\n\n")))
    }
}
