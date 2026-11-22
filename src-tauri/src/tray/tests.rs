use super::*;
use crate::provider::{UsageData, UsageResult};
use crate::services::subscription::{
    CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_GEMINI_FLASH,
    TIER_GEMINI_FLASH_LITE, TIER_GEMINI_PRO, TIER_MONTHLY, TIER_SEVEN_DAY, TIER_SEVEN_DAY_FABLE,
    TIER_SEVEN_DAY_OPUS, TIER_SEVEN_DAY_SONNET, TIER_THIRTY_DAY, TIER_WEEKLY_LIMIT,
};

fn en() -> TrayTexts {
    TrayTexts::from_language("en")
}

fn zh() -> TrayTexts {
    TrayTexts::from_language("zh")
}

fn now() -> chrono::DateTime<chrono::Local> {
    chrono::Local::now()
}

#[test]
fn an_unreadable_provider_is_left_out_of_the_tray_instead_of_breaking_it() {
    use std::sync::Arc;
    let state = AppState::new(Arc::new(crate::database::Database::memory().unwrap()));
    for id in ["good", "broken"] {
        let provider = crate::provider::Provider::with_id(
            id.into(),
            id.into(),
            serde_json::json!({"env": {}}),
            None,
        );
        state.db.save_provider(&AppType::Codex, &provider).unwrap();
    }
    state
        .db
        .conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE providers SET meta = '{' WHERE app_type = 'codex' AND id = 'broken'",
            [],
        )
        .unwrap();

    let snapshot = collect_app_snapshot(
        &state,
        &en(),
        &crate::settings::AppSettings::default(),
        &AppType::Codex,
        None,
        &[],
    )
    .expect("the tray still builds");
    let ids: Vec<&str> = snapshot.providers.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["good"]);
}

#[test]
#[serial_test::serial]
fn the_tray_still_builds_when_the_provider_in_use_is_unreadable() {
    use std::sync::Arc;
    let home = tempfile::tempdir().unwrap();
    let _env = crate::test_support::EnvVars::new()
        .set("CC_SWITCH_TEST_HOME", home.path())
        .set("HOME", home.path());
    crate::settings::reload_settings().unwrap();
    let state = AppState::new(Arc::new(crate::database::Database::memory().unwrap()));
    for id in ["good", "broken"] {
        let provider = crate::provider::Provider::with_id(
            id.into(),
            id.into(),
            serde_json::json!({"env": {}}),
            None,
        );
        state.db.save_provider(&AppType::Codex, &provider).unwrap();
    }
    state
        .db
        .conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE providers SET meta = '{' WHERE app_type = 'codex' AND id = 'broken'",
            [],
        )
        .unwrap();
    let snapshot = || {
        collect_app_snapshot(
            &state,
            &en(),
            &crate::settings::AppSettings::default(),
            &AppType::Codex,
            None,
            &[],
        )
        .expect("the tray still builds")
    };

    // 直连指针指着读不出来的那行：按没有当前供应商显示，其余照常列出。
    crate::settings::set_current_provider(&AppType::Codex, Some("broken")).unwrap();
    let direct = snapshot();
    assert_eq!(direct.current_id, None);
    let ids: Vec<&str> = direct.providers.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["good"]);

    // 代理路由指着它也一样。
    crate::mode::state::update(&crate::live::engine::DeviceStore::for_device(), |live| {
        let codex = live.apps.entry("codex".to_string()).or_default();
        codex.mode = Some(crate::mode::state::Mode::Proxy);
        codex.proxy_route = Some("broken".to_string());
    })
    .unwrap();
    crate::settings::set_current_provider(&AppType::Codex, Some("good")).unwrap();
    assert_eq!(snapshot().current_id, None);

    crate::settings::set_current_provider(&AppType::Codex, None).unwrap();
}

// ─── 基础 ───

#[test]
fn tray_id_is_unique_to_app() {
    assert_eq!(TRAY_ID, "cc-switch");
    assert_ne!(TRAY_ID, "main");
}

#[test]
fn tray_lists_switch_apps_in_sidebar_order_without_additive_apps() {
    let names: Vec<&str> = TRAY_APPS.iter().map(app_display_name).collect();
    assert_eq!(
        names,
        [
            "Claude Code",
            "Claude Desktop",
            "Codex",
            "Gemini CLI",
            "Grok Build"
        ]
    );
    for app in TRAY_APPS {
        assert!(!app.spec().additive, "{app:?} 是累加式应用，不进托盘");
    }
}

/// 前端 `src/i18n/languages.ts` 的语言表和系统区域识别对照
/// `tests/config/languages.json`，这份文件由这里生成。改了 `settings::LANGUAGES`
/// 或 [`map_locale_to_tray_language`] 后用
/// `CC_SWITCH_UPDATE_GOLDEN=1 cargo test languages_match_shared_fixture` 重新生成。
#[test]
fn languages_match_shared_fixture() {
    const LOCALES: &[&str] = &[
        "",
        "zh",
        "zh-CN",
        "zh-SG",
        "zh-Hans",
        "zh-Hans-CN",
        "zh_CN",
        "zh-TW",
        "zh-HK",
        "zh-MO",
        "zh-Hant",
        "zh-Hant-TW",
        "zh-hant-hk",
        "zh-tw-x",
        "ja",
        "ja-JP",
        "ja_JP",
        "ko",
        "ko-KR",
        "kok",
        "tr",
        "tr-TR",
        "ru",
        "ru-RU",
        "de",
        "de-AT",
        "en",
        "en-US",
        "fr-FR",
        "es",
        "pt-BR",
    ];
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/config/languages.json");
    let locale_to_language: serde_json::Map<String, serde_json::Value> = LOCALES
        .iter()
        .map(|locale| {
            (
                locale.to_string(),
                map_locale_to_tray_language(locale).into(),
            )
        })
        .collect();
    let expected = serde_json::to_string_pretty(&serde_json::json!({
        "languages": crate::settings::LANGUAGES,
        "localeToLanguage": locale_to_language,
    }))
    .unwrap()
        + "\n";
    if std::env::var_os("CC_SWITCH_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        actual, expected,
        "tests/config/languages.json 和后端语言表不一致；确认改动后用 CC_SWITCH_UPDATE_GOLDEN=1 重新生成，再同步 src/i18n/languages.ts"
    );
    for &language in crate::settings::LANGUAGES {
        assert_eq!(map_locale_to_tray_language(language), language);
    }
}

#[test]
fn locale_maps_traditional_chinese_variants_to_zh_tw() {
    for locale in [
        "zh-TW",
        "zh-HK",
        "zh-MO",
        "zh-Hant",
        "zh-Hant-TW",
        "zh-hant-hk",
    ] {
        assert_eq!(
            map_locale_to_tray_language(locale),
            "zh-TW",
            "expected {locale} -> zh-TW"
        );
    }
}

#[test]
fn locale_maps_simplified_chinese_variants_to_zh() {
    for locale in ["zh", "zh-CN", "zh-SG", "zh-Hans", "zh-Hans-CN"] {
        assert_eq!(
            map_locale_to_tray_language(locale),
            "zh",
            "expected {locale} -> zh"
        );
    }
}

#[test]
fn locale_matches_language_and_chinese_region_boundaries() {
    for locale in ["zha", "zhb-CN", "jam", "trv-TW"] {
        assert_eq!(map_locale_to_tray_language(locale), "en", "{locale}");
    }
    for locale in ["zh-TW-u-nu-hanidec", "zh-Hant-TW", "zh-HK"] {
        assert_eq!(map_locale_to_tray_language(locale), "zh-TW", "{locale}");
    }
    for locale in ["zh-hantx", "zh-hkx"] {
        assert_eq!(map_locale_to_tray_language(locale), "zh", "{locale}");
    }
}

#[test]
fn locale_maps_korean_without_matching_other_language_subtags() {
    for locale in ["ko", "ko-KR", "ko-KP", "KO-kr"] {
        assert_eq!(map_locale_to_tray_language(locale), "ko");
    }
    for locale in ["kok", "kok-IN", "kos"] {
        assert_eq!(map_locale_to_tray_language(locale), "en");
    }
}

#[test]
fn locale_maps_russian_without_matching_other_language_subtags() {
    for locale in ["ru", "ru-RU", "ru-UA", "RU-ru"] {
        assert_eq!(map_locale_to_tray_language(locale), "ru");
    }
    // 卢森尼亚语 rue、Rupuni 等以 ru 开头但不是俄语，按不支持的语言回落英文
    for locale in ["rue", "rue-UA", "rup"] {
        assert_eq!(map_locale_to_tray_language(locale), "en");
    }
}

#[test]
fn locale_maps_german_without_matching_other_language_subtags() {
    for locale in ["de", "de-DE", "de-AT", "de-CH", "DE-de"] {
        assert_eq!(map_locale_to_tray_language(locale), "de");
    }
    // 下索布语 dsb、Delaware 语 del 等不是德语，按不支持的语言回落英文
    for locale in ["dsb", "del", "den-CA"] {
        assert_eq!(map_locale_to_tray_language(locale), "en");
    }
}

#[test]
fn locale_maps_japanese_and_english() {
    assert_eq!(map_locale_to_tray_language("ko-KR"), "ko");
    assert_eq!(map_locale_to_tray_language("ko"), "ko");
    assert_eq!(map_locale_to_tray_language("tr-TR"), "tr");
    assert_eq!(map_locale_to_tray_language("tr"), "tr");
    assert_eq!(map_locale_to_tray_language("ja-JP"), "ja");
    assert_eq!(map_locale_to_tray_language("ja"), "ja");
    assert_eq!(map_locale_to_tray_language("en-US"), "en");
    assert_eq!(map_locale_to_tray_language("en"), "en");
}

#[test]
fn locale_unknown_falls_back_to_en() {
    // 与前端 getInitialLanguage 保持一致：认不出的语言用英文，区域为空才用默认的 zh。
    for locale in ["it-IT", "fr", "es-ES"] {
        assert_eq!(
            map_locale_to_tray_language(locale),
            "en",
            "expected {locale} -> en"
        );
    }
    assert_eq!(map_locale_to_tray_language(""), "zh");
}

#[test]
fn mode_names_follow_the_v7_terms_in_every_language() {
    let zh = zh();
    assert_eq!(
        (zh.header_direct, zh.mode_route, zh.mode_stack),
        ("直连", "路由", "聚合")
    );
    assert_eq!(zh.header_failover, "路由 · 故障转移开启中");
    let tw = TrayTexts::from_language("zh-TW");
    assert_eq!((tw.header_direct, tw.mode_stack), ("直連", "聚合"));
    let en = en();
    assert_eq!(
        (en.header_direct, en.mode_route, en.mode_stack),
        ("Direct", "Routing", "Aggregation")
    );
    let ja = TrayTexts::from_language("ja");
    assert_eq!(ja.mode_route, "ルーティング");
    for texts in [zh, tw, en, ja] {
        // 托盘不用 emoji 表达状态（Windows 菜单里彩色 emoji 会变成单色轮廓）。
        for text in [
            texts.almost_out,
            texts.needs_attention,
            texts.official_blocked_suffix,
        ] {
            assert!(text.chars().all(|c| (c as u32) < 0x1F000), "{text}");
        }
    }
}

// ─── 额度文字 ───

fn make_quota(tool: &str, success: bool, tiers: Vec<QuotaTier>) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: tool.to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success,
        tiers,
        extra_usage: None,
        reset_credits: None,
        credits_balance: None,
        plan_name: None,
        error: None,
        queried_at: Some(0),
    }
}

fn tier(name: &str, utilization: f64) -> QuotaTier {
    QuotaTier {
        name: name.to_string(),
        utilization,
        resets_at: None,
        used_value_usd: None,
        max_value_usd: None,
    }
}

fn usage_data(plan_name: Option<&str>, utilization: f64) -> UsageData {
    UsageData {
        plan_name: plan_name.map(String::from),
        extra: None,
        is_valid: Some(true),
        invalid_message: None,
        total: Some(100.0),
        used: Some(utilization),
        remaining: Some(100.0 - utilization),
        unit: Some("%".to_string()),
    }
}

fn usage_result(success: bool, data: Vec<UsageData>) -> UsageResult {
    UsageResult {
        success,
        data: if data.is_empty() { None } else { Some(data) },
        error: None,
    }
}

fn title(view: Option<QuotaView>) -> Option<(String, bool)> {
    view.as_ref().and_then(quota_title)
}

fn sub_title(texts: &TrayTexts, quota: &SubscriptionQuota) -> Option<String> {
    title(format_subscription_quota(texts, quota)).map(|(text, _)| text)
}

#[test]
fn subscription_quota_is_written_as_what_is_left() {
    let quota = make_quota(
        "claude",
        true,
        vec![tier(TIER_FIVE_HOUR, 31.0), tier(TIER_SEVEN_DAY, 95.0)],
    );
    assert_eq!(
        title(format_subscription_quota(&zh(), &quota)),
        Some(("5 小时剩余 69% · 每周剩余 5%".to_string(), true))
    );
    assert_eq!(
        title(format_subscription_quota(&en(), &quota)),
        Some(("5-hour 69% left · Weekly 5% left".to_string(), true))
    );
    let tw = TrayTexts::from_language("zh-TW");
    assert_eq!(
        sub_title(&tw, &quota).as_deref(),
        Some("5 小時剩餘 69% · 每週剩餘 5%")
    );
}

#[test]
fn subscription_quota_is_written_as_what_is_used_when_chosen() {
    let used = |texts: TrayTexts| TrayTexts {
        quota_used: true,
        ..texts
    };
    let quota = make_quota(
        "claude",
        true,
        vec![tier(TIER_FIVE_HOUR, 31.0), tier(TIER_SEVEN_DAY, 95.0)],
    );
    // 「快用完」仍按剩余判断
    assert_eq!(
        title(format_subscription_quota(&used(zh()), &quota)),
        Some(("5 小时已用 31% · 每周已用 95%".to_string(), true))
    );
    // 用完照旧写「已用完」
    assert_eq!(
        sub_title(
            &used(zh()),
            &make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, 100.0)])
        )
        .as_deref(),
        Some("5 小时已用完")
    );
    assert_eq!(
        sub_title(
            &used(en()),
            &make_quota("gemini", true, vec![tier(TIER_GEMINI_PRO, 15.4)])
        )
        .as_deref(),
        Some("Pro 15% used")
    );
}

#[test]
fn almost_out_only_below_twenty_percent_left_and_not_when_used_up() {
    let at = |used: f64| {
        title(format_subscription_quota(
            &zh(),
            &make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, used)]),
        ))
        .unwrap()
    };
    assert_eq!(at(80.0), ("5 小时剩余 20%".to_string(), false));
    assert_eq!(at(85.0), ("5 小时剩余 15%".to_string(), true));
    // 用完时额度本身写「已用完」，不再加「快用完」。
    assert_eq!(at(100.0), ("5 小时已用完".to_string(), false));
}

#[test]
fn latin_labels_get_a_space_before_the_chinese_word() {
    let quota = make_quota(
        "gemini",
        true,
        vec![tier(TIER_GEMINI_PRO, 15.0), tier(TIER_GEMINI_FLASH, 42.0)],
    );
    assert_eq!(
        sub_title(&zh(), &quota).as_deref(),
        Some("Pro 剩余 85% · Flash 剩余 58%")
    );
}

#[test]
fn title_writes_every_tier_in_original_order() {
    let quota = make_quota(
        "gemini",
        true,
        vec![
            tier(TIER_GEMINI_PRO, 5.0),
            tier(TIER_GEMINI_FLASH, 42.0),
            tier(TIER_GEMINI_FLASH_LITE, 80.0),
        ],
    );
    assert_eq!(
        sub_title(&en(), &quota).as_deref(),
        Some("Pro 95% left · Flash 58% left · Flash Lite 20% left")
    );
}

#[test]
fn fable_stays_separate_and_weekly_aliases_use_the_highest() {
    let quota = make_quota(
        "claude",
        true,
        vec![
            tier(TIER_FIVE_HOUR, 12.0),
            tier(TIER_SEVEN_DAY_OPUS, 20.0),
            tier(TIER_SEVEN_DAY_SONNET, 95.0),
            tier(TIER_SEVEN_DAY_FABLE, 30.0),
        ],
    );
    let Some(QuotaView::Lines(lines)) = format_subscription_quota(&en(), &quota) else {
        panic!("expected lines");
    };
    let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(
        texts,
        ["5-hour 88% left", "Weekly 5% left", "Fable 70% left"]
    );
}

#[test]
fn monthly_and_thirty_day_windows_keep_their_own_words() {
    let codex_free = make_quota("codex", true, vec![tier(TIER_THIRTY_DAY, 85.0)]);
    assert_eq!(
        sub_title(&zh(), &codex_free).as_deref(),
        Some("30 天剩余 15%")
    );
    let volcengine = usage_result(
        true,
        vec![
            usage_data(Some(TIER_FIVE_HOUR), 25.0),
            usage_data(Some(TIER_WEEKLY_LIMIT), 30.0),
            usage_data(Some(TIER_MONTHLY), 42.0),
        ],
    );
    let Some(QuotaView::Lines(lines)) = format_script_result(&zh(), &volcengine) else {
        panic!("expected lines");
    };
    assert_eq!(lines[2].text, "每月剩余 58%");
    assert!(!lines.iter().any(|line| line.text.contains("monthly")));
}

#[test]
fn flattened_subscription_tiers_match_the_subscription_text() {
    let quota = make_quota(
        "claude",
        true,
        vec![tier(TIER_FIVE_HOUR, 12.0), tier(TIER_SEVEN_DAY, 25.0)],
    );
    let result = usage_result(
        true,
        vec![
            usage_data(Some(TIER_FIVE_HOUR), 12.0),
            usage_data(Some(TIER_SEVEN_DAY), 25.0),
        ],
    );
    assert_eq!(
        format_script_result(&en(), &result),
        format_subscription_quota(&en(), &quota)
    );
}

#[test]
fn balance_is_shown_and_never_warns_before_running_out() {
    let balance = |remaining: f64, total: Option<f64>| UsageData {
        plan_name: Some("CNY".to_string()),
        extra: None,
        is_valid: Some(true),
        invalid_message: None,
        total,
        used: None,
        remaining: Some(remaining),
        unit: Some("CNY".to_string()),
    };
    let one = |data| title(format_script_result(&zh(), &usage_result(true, vec![data])));
    assert_eq!(
        one(balance(82.1, None)),
        Some(("余额 82.10 CNY".to_string(), false))
    );
    assert_eq!(
        one(balance(5.0, Some(100.0))),
        Some(("余额 5.00 CNY".to_string(), false))
    );
    assert_eq!(
        one(balance(0.0, Some(100.0))),
        Some(("余额 0.00 CNY".to_string(), false))
    );
    // 欠费照写负数；-0.0 和舍入到 0 的小负数不写成 -0.00（#8812）
    assert_eq!(
        one(balance(-24.96, None)),
        Some(("余额 -24.96 CNY".to_string(), false))
    );
    assert_eq!(
        one(balance(-0.0, None)),
        Some(("余额 0.00 CNY".to_string(), false))
    );
    assert_eq!(
        one(balance(-0.004, None)),
        Some(("余额 0.00 CNY".to_string(), false))
    );

    // 面板只给还有的余额金额上绿色；用完的那行整行变色，金额不另上色
    let emphasis = |data| match format_script_result(&zh(), &usage_result(true, vec![data])) {
        Some(QuotaView::Lines(lines)) => lines[0].emphasis.clone(),
        _ => panic!("expected quota lines"),
    };
    assert_eq!(emphasis(balance(82.1, None)), Some("82.10 CNY".to_string()));
    assert_eq!(emphasis(balance(0.0, Some(100.0))), None);
}

#[test]
fn copilot_premium_requests_use_the_card_word() {
    let copilot = |total: f64, remaining: f64| UsageData {
        plan_name: Some("copilot_pro".to_string()),
        extra: Some("Reset: 2026-11-01".to_string()),
        is_valid: Some(true),
        invalid_message: None,
        total: Some(total),
        used: Some(total - remaining),
        remaining: Some(remaining),
        unit: Some(COPILOT_UNIT_PREMIUM.to_string()),
    };
    assert_eq!(
        title(format_script_result(
            &zh(),
            &usage_result(true, vec![copilot(300.0, 180.0)])
        )),
        Some(("Credits 剩余 60%".to_string(), false))
    );
    // Free 计划不含 Credits（总额 0）：不显示，不能写成「余额 0」
    assert_eq!(
        format_script_result(&zh(), &usage_result(true, vec![copilot(0.0, 0.0)])),
        None
    );
}

#[test]
fn expired_plan_and_failed_queries() {
    let mut expired = usage_data(None, 10.0);
    expired.is_valid = Some(false);
    assert_eq!(
        title(format_script_result(
            &zh(),
            &usage_result(true, vec![expired])
        )),
        Some(("套餐已过期".to_string(), false))
    );

    let failed = format_script_result(&zh(), &usage_result(false, vec![]));
    assert_eq!(failed, Some(QuotaView::Failed(QuotaFailure::Other)));
    // 查询失败时标题不写额度，原因写在子菜单里。
    assert_eq!(title(failed.clone()), None);
    assert_eq!(
        quota_note(&zh(), &failed.unwrap(), now()).as_deref(),
        Some("额度查询失败")
    );

    let login =
        SubscriptionQuota::error("claude", CredentialStatus::Expired, "expired".to_string());
    let view = format_subscription_quota(&zh(), &login).unwrap();
    assert_eq!(
        quota_note(&zh(), &view, now()).as_deref(),
        Some("额度查询失败：登录已过期")
    );
    let pending = SubscriptionQuota::error(
        "claude",
        CredentialStatus::RefreshPending,
        "pending".to_string(),
    );
    let view = format_subscription_quota(&zh(), &pending).unwrap();
    assert_eq!(
        quota_note(&zh(), &view, now()).as_deref(),
        Some("额度查询失败：令牌待刷新")
    );
    // 没有凭据时不说话。
    let missing =
        SubscriptionQuota::error("claude", CredentialStatus::NotFound, "missing".to_string());
    assert_eq!(format_subscription_quota(&zh(), &missing), None);
}

#[test]
fn unknown_tiers_alone_show_nothing() {
    let quota = make_quota("claude", true, vec![tier("one_hour", 80.0)]);
    assert_eq!(format_subscription_quota(&en(), &quota), None);
}

#[test]
fn reset_note_appears_only_when_running_out() {
    let later = (now() + chrono::Duration::days(3)).to_rfc3339();
    let mut weekly = tier(TIER_SEVEN_DAY, 95.0);
    weekly.resets_at = Some(later.clone());
    let quota = make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, 10.0), weekly]);
    let view = format_subscription_quota(&zh(), &quota).unwrap();
    let note = quota_note(&zh(), &view, now()).expect("note");
    assert!(note.starts_with("每周额度 "), "{note}");
    assert!(note.ends_with("日重置"), "{note}");
    let en_view = format_subscription_quota(&en(), &quota).unwrap();
    let en_note = quota_note(&en(), &en_view, now()).unwrap();
    assert!(en_note.starts_with("Weekly quota resets "), "{en_note}");

    let mut calm = tier(TIER_SEVEN_DAY, 50.0);
    calm.resets_at = Some(later);
    let calm = format_subscription_quota(&zh(), &make_quota("claude", true, vec![calm]));
    assert_eq!(quota_note(&zh(), &calm.unwrap(), now()), None);
}

// ─── 🔴 #7267：托管 Codex 账号卡按账号取额度 ───

fn codex_provider(account_id: Option<&str>, enabled: Option<bool>) -> Provider {
    serde_json::from_value(serde_json::json!({
        "id": "managed-codex",
        "name": "Managed Codex",
        "settingsConfig": {"auth": {}, "config": ""},
        "category": "official",
        "meta": {
            "authBinding": account_id.map(|id| serde_json::json!({
                "source": "managed_account",
                "authProvider": "codex_oauth",
                "accountId": id
            })),
            "usage_script": enabled.map(|enabled| serde_json::json!({
                "enabled": enabled,
                "language": "javascript",
                "code": "",
                "templateType": "official_subscription"
            }))
        }
    }))
    .unwrap()
}

#[test]
fn managed_codex_quota_stays_out_of_the_app_wide_tray_cache() {
    let provider = |id| codex_provider(id, Some(true));
    assert!(!provider_uses_official_subscription(&provider(Some(
        "account-1"
    ))));
    assert!(provider_uses_official_subscription(&provider(None)));
}

#[test]
fn managed_codex_tray_refresh_respects_binding_and_usage_toggle() {
    for enabled in [Some(true), None] {
        let provider = codex_provider(Some("account-1"), enabled);
        assert_eq!(
            tray_usage_source(&AppType::Codex, &provider),
            Some(TrayUsageSource::ManagedCodex("account-1".to_string()))
        );
    }
    let disabled = codex_provider(Some("account-1"), Some(false));
    assert_eq!(tray_usage_source(&AppType::Codex, &disabled), None);

    let mut fixed = codex_provider(Some("account-1"), Some(true));
    fixed.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
    assert_eq!(
        tray_usage_source(&AppType::Codex, &fixed),
        Some(TrayUsageSource::ManagedCodex("account-1".to_string()))
    );
    assert_eq!(
        tray_usage_source(&AppType::Codex, &codex_provider(None, Some(true))),
        Some(TrayUsageSource::Subscription)
    );
    let mut legacy = codex_provider(Some(" account-1 "), None);
    legacy.category = None;
    assert_eq!(
        tray_usage_source(&AppType::Codex, &legacy),
        Some(TrayUsageSource::ManagedCodex("account-1".to_string()))
    );
}

fn claude_bound_provider(account_id: Option<&str>, enabled: Option<bool>) -> Provider {
    serde_json::from_value(serde_json::json!({
        "id": "claude-bound",
        "name": "Claude Official",
        "settingsConfig": {"env": {}},
        "category": "official",
        "meta": {
            "authBinding": account_id.map(|id| serde_json::json!({
                "source": "managed_account",
                "authProvider": "claude_official",
                "accountId": id
            })),
            "usage_script": enabled.map(|enabled| serde_json::json!({
                "enabled": enabled,
                "language": "javascript",
                "code": "",
                "templateType": "official_subscription"
            }))
        }
    }))
    .unwrap()
}

#[test]
fn claude_bound_card_hint_shows_account_unless_noted() {
    let accounts = HashMap::from([("acct-1".to_string(), "me@example.com".to_string())]);
    let mut provider = claude_bound_provider(Some("acct-1"), None);
    provider.website_url = Some("https://www.anthropic.com".to_string());
    assert_eq!(
        provider_hint(&AppType::Claude, &provider, &accounts).as_deref(),
        Some("me@example.com")
    );

    provider.notes = Some("Work".to_string());
    assert_eq!(
        provider_hint(&AppType::Claude, &provider, &accounts).as_deref(),
        Some("Work")
    );

    // 账号不在这台电脑上：退回官网域名
    let mut missing = claude_bound_provider(Some("acct-2"), None);
    missing.website_url = Some("https://www.anthropic.com".to_string());
    assert_eq!(
        provider_hint(&AppType::Claude, &missing, &accounts).as_deref(),
        Some("www.anthropic.com")
    );
}

#[test]
fn claude_bound_card_tray_usage_defaults_on_and_reads_its_account() {
    // 卡片在没保存开关时默认查额度：托盘也要显示，不然两边各说各的
    for enabled in [Some(true), None] {
        assert_eq!(
            tray_usage_source(
                &AppType::Claude,
                &claude_bound_provider(Some(" acct-1 "), enabled)
            ),
            Some(TrayUsageSource::ClaudeAccount("acct-1".to_string()))
        );
    }
    assert_eq!(
        tray_usage_source(
            &AppType::Claude,
            &claude_bound_provider(Some("acct-1"), Some(false))
        ),
        None
    );
    // 没绑账号的官方卡照旧：开了官方订阅模板才查，读默认登录
    assert_eq!(
        tray_usage_source(&AppType::Claude, &claude_bound_provider(None, None)),
        None
    );
    assert_eq!(
        tray_usage_source(&AppType::Claude, &claude_bound_provider(None, Some(true))),
        Some(TrayUsageSource::Subscription)
    );

    // 只读这个号的快照，不回落到默认登录的应用级缓存
    let cache = UsageCache::new();
    let texts = en();
    let provider = claude_bound_provider(Some("acct-1"), None);
    let label = || {
        title(usage_view(
            &cache,
            &texts,
            &AppType::Claude,
            &provider,
            &provider.id,
        ))
        .map(|(text, _)| text)
    };
    cache.put_subscription(
        AppType::Claude,
        make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, 99.0)]),
    );
    assert_eq!(label(), None);
    cache.put_claude_account(
        "acct-1".to_string(),
        make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, 25.0)]),
    );
    assert_eq!(label().as_deref(), Some("5-hour 75% left"));
}

#[test]
fn managed_codex_tray_uses_bound_account_after_switch_or_rebind() {
    let cache = UsageCache::new();
    let texts = en();
    let first = codex_provider(Some("account-1"), Some(true));
    let mut second = codex_provider(Some("account-2"), Some(true));
    second.id = "second-provider".to_string();
    let label = |provider: &Provider| {
        title(usage_view(
            &cache,
            &texts,
            &AppType::Codex,
            provider,
            &provider.id,
        ))
        .map(|(text, _)| text)
    };
    cache.put_subscription(
        AppType::Codex,
        make_quota("codex", true, vec![tier(TIER_FIVE_HOUR, 99.0)]),
    );
    cache.put_script(
        AppType::Codex,
        first.id.clone(),
        usage_result(true, vec![usage_data(Some(TIER_FIVE_HOUR), 99.0)]),
    );
    // Neither CLI nor stale provider-scoped data may fill an account miss.
    assert_eq!(label(&first), None);
    cache.put_codex_oauth(
        "account-1".to_string(),
        make_quota("codex_oauth", true, vec![tier(TIER_FIVE_HOUR, 12.0)]),
    );
    assert_eq!(label(&first).as_deref(), Some("5-hour 88% left"));
    assert_eq!(label(&second), None);
    cache.put_codex_oauth(
        "account-2".to_string(),
        make_quota("codex_oauth", true, vec![tier(TIER_FIVE_HOUR, 25.0)]),
    );
    assert_eq!(label(&second).as_deref(), Some("5-hour 75% left"));
    // Rebinding the same provider must select the new account's snapshot.
    second.id = first.id.clone();
    assert_eq!(label(&second).as_deref(), Some("5-hour 75% left"));
    // A late response for the previous account cannot overwrite this label.
    cache.put_codex_oauth(
        "account-1".to_string(),
        make_quota("codex_oauth", true, vec![tier(TIER_FIVE_HOUR, 40.0)]),
    );
    assert_eq!(label(&second).as_deref(), Some("5-hour 75% left"));
    assert_eq!(label(&first).as_deref(), Some("5-hour 60% left"));

    let disabled = codex_provider(Some("account-2"), Some(false));
    assert_eq!(label(&disabled), None);
    assert_eq!(label(&second).as_deref(), Some("5-hour 75% left"));
    cache.put_codex_oauth(
        "account-2".to_string(),
        SubscriptionQuota::error(
            "codex_oauth",
            CredentialStatus::Expired,
            "expired".to_string(),
        ),
    );
    assert_eq!(label(&second), None);
    assert_eq!(
        usage_view(&cache, &texts, &AppType::Codex, &second, &second.id),
        Some(QuotaView::Failed(QuotaFailure::LoginExpired))
    );
    assert_eq!(label(&first).as_deref(), Some("5-hour 60% left"));
}

#[test]
fn native_codex_tray_still_uses_cli_subscription() {
    let cache = UsageCache::new();
    let mut native = codex_provider(None, Some(true));
    native.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
    cache.put_codex_oauth(
        "account-1".to_string(),
        make_quota("codex_oauth", true, vec![tier(TIER_FIVE_HOUR, 12.0)]),
    );
    cache.put_subscription(
        AppType::Codex,
        make_quota("codex", true, vec![tier(TIER_FIVE_HOUR, 25.0)]),
    );
    assert_eq!(
        title(usage_view(
            &cache,
            &en(),
            &AppType::Codex,
            &native,
            &native.id
        ))
        .map(|(text, _)| text)
        .as_deref(),
        Some("5-hour 75% left")
    );
}

#[test]
fn cli_subscription_tray_reads_the_cache_the_card_writes() {
    let cache = UsageCache::new();
    let native = codex_provider(None, Some(true));
    assert_eq!(
        tray_usage_source(&AppType::Claude, &native),
        Some(TrayUsageSource::Subscription)
    );
    let view = || usage_view(&cache, &en(), &AppType::Claude, &native, &native.id);

    // 脚本缓存里的同供应商结果不能盖住订阅缓存。
    cache.put_script(
        AppType::Claude,
        native.id.clone(),
        usage_result(true, vec![usage_data(Some(TIER_FIVE_HOUR), 99.0)]),
    );
    assert_eq!(view(), None);
    cache.put_subscription(
        AppType::Claude,
        SubscriptionQuota::error(
            "claude",
            CredentialStatus::RefreshPending,
            "pending".to_string(),
        ),
    );
    assert_eq!(
        view(),
        Some(QuotaView::Failed(QuotaFailure::TokenRefreshPending))
    );
    cache.put_subscription(
        AppType::Claude,
        make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, 25.0)]),
    );
    assert_eq!(
        title(view()).map(|(text, _)| text).as_deref(),
        Some("5-hour 75% left")
    );

    // xAI OAuth 的额度属于绑定的 SuperGrok 账号：仍走脚本路径，也不沿用应用级订阅缓存。
    let mut xai = codex_provider(None, Some(true));
    xai.meta.as_mut().unwrap().provider_type = Some("xai_oauth".to_string());
    assert_eq!(
        tray_usage_source(&AppType::Claude, &xai),
        Some(TrayUsageSource::Script)
    );
    assert_eq!(
        title(usage_view(&cache, &en(), &AppType::Claude, &xai, "xai")),
        None
    );
}

#[test]
fn codex_wire_api_falls_back_to_a_single_assignment_when_toml_is_broken() {
    assert_eq!(
        codex_wire_api("wire_api = \"chat\"\nbroken = [").as_deref(),
        Some("chat")
    );
    assert_eq!(
        codex_wire_api("wire_api = \"chat\"\nwire_api = \"responses\"\nx = ["),
        None
    );
}

// ─── 菜单模型 ───

fn entry(id: &str, name: &str) -> ProviderEntry {
    ProviderEntry {
        id: id.to_string(),
        name: name.to_string(),
        hint: None,
        needs_routing: false,
        official: false,
        blocked_from_routing: false,
    }
}

fn snapshot(app: AppType, mode: TrayMode, providers: Vec<ProviderEntry>) -> AppSnapshot {
    AppSnapshot {
        app,
        mode,
        current_id: providers.first().map(|p| p.id.clone()),
        providers,
        queue: Vec::new(),
        stack_members: Vec::new(),
        quota: None,
        service_down: false,
        needs_attention: false,
        profiles: None,
    }
}

fn model(problems: &[TrayProblem], apps: &[AppSnapshot]) -> Vec<TrayEntry> {
    let status = MenuStatus {
        problems: problems.to_vec(),
        ..Default::default()
    };
    build_menu_model(&zh(), &status, apps, false, now())
}

fn text_of(entry: &TrayEntry) -> String {
    match entry {
        TrayEntry::Item { text, .. }
        | TrayEntry::Check { text, .. }
        | TrayEntry::Submenu { text, .. } => text.clone(),
        TrayEntry::Separator => "---".to_string(),
    }
}

fn texts_of(entries: &[TrayEntry]) -> Vec<String> {
    entries.iter().map(text_of).collect()
}

#[test]
fn downloaded_update_adds_install_item_right_below_open_main() {
    let status = MenuStatus {
        update_ready: Some("4.1.0".to_string()),
        ..Default::default()
    };
    let entries = build_menu_model(&zh(), &status, &[], false, now());
    let texts = texts_of(&entries);
    assert_eq!(texts[0], "打开 CC Switch");
    assert_eq!(texts[1], "重启并更新到 v4.1.0");
    assert!(matches!(
        &entries[1],
        TrayEntry::Item { id, .. } if id == INSTALL_UPDATE_ID
    ));

    let without = build_menu_model(&zh(), &MenuStatus::default(), &[], false, now());
    assert!(!without
        .iter()
        .any(|entry| matches!(entry, TrayEntry::Item { id, .. } if id == INSTALL_UPDATE_ID)));
}

fn children_of(menu: &[TrayEntry], app: &AppType) -> Vec<TrayEntry> {
    menu.iter()
        .find_map(|entry| match entry {
            TrayEntry::Submenu {
                app: Some(a),
                children,
                ..
            } if a == app => Some(children.clone()),
            _ => None,
        })
        .expect("submenu")
}

#[test]
fn top_level_order_is_open_apps_lightweight_website_quit() {
    let apps = [
        snapshot(
            AppType::Claude,
            TrayMode::Direct,
            vec![entry("kimi", "Kimi For Coding")],
        ),
        snapshot(AppType::GrokBuild, TrayMode::Direct, vec![]),
    ];
    let menu = model(&[], &apps);
    assert_eq!(
        texts_of(&menu),
        [
            "打开 CC Switch",
            "---",
            "Claude Code · Kimi For Coding",
            "Grok Build · 添加供应商…",
            "---",
            "轻量模式",
            "---",
            "打开官方网站",
            "退出 CC Switch",
        ]
    );
    assert!(matches!(&menu[3], TrayEntry::Item { id, .. } if id == "nav:add:grokbuild"));
    assert!(matches!(&menu[8], TrayEntry::Item { id, .. } if id == "quit"));
}

#[test]
fn direct_submenu_sends_needs_routing_providers_to_the_app_page() {
    let mut copilot = entry("copilot", "GitHub Copilot");
    copilot.needs_routing = true;
    let app = snapshot(
        AppType::Claude,
        TrayMode::Direct,
        vec![entry("kimi", "Kimi For Coding"), copilot],
    );
    let children = children_of(&model(&[], &[app]), &AppType::Claude);
    assert_eq!(
        texts_of(&children),
        [
            "直连",
            "Kimi For Coding",
            "GitHub Copilot（需要路由）…",
            "---",
            "打开 Claude Code 页面"
        ]
    );
    assert!(matches!(
        &children[0],
        TrayEntry::Item { enabled: false, .. }
    ));
    assert!(matches!(
        &children[1],
        TrayEntry::Check { id, checked: true, enabled: true, .. } if id == "prov:claude:kimi"
    ));
    assert!(matches!(
        &children[2],
        TrayEntry::Item { id, enabled: true, .. } if id == "nav:needs:claude:copilot"
    ));
    assert!(matches!(&children[4], TrayEntry::Item { id, .. } if id == "nav:app:claude"));
}

#[test]
fn route_submenu_disables_official_plans_with_the_reason() {
    let mut official = entry("xai", "xAI Official");
    official.official = true;
    official.blocked_from_routing = true;
    let app = snapshot(
        AppType::GrokBuild,
        TrayMode::Route,
        vec![entry("or", "OpenRouter"), official],
    );
    let menu = model(&[], &[app]);
    assert_eq!(text_of(&menu[2]), "Grok Build · 路由 · OpenRouter");
    let children = children_of(&menu, &AppType::GrokBuild);
    assert_eq!(text_of(&children[0]), "路由");
    assert!(
        matches!(&children[2], TrayEntry::Item { enabled: false, text, .. }
        if text == "xAI Official（官方订阅不经过路由）")
    );
}

#[test]
fn failover_submenu_is_read_only_and_lists_only_the_queue() {
    let mut app = snapshot(
        AppType::Claude,
        TrayMode::Failover,
        vec![
            entry("a", "DeepSeek"),
            entry("b", "智谱 GLM Coding Plan"),
            entry("c", "OpenRouter"),
        ],
    );
    app.queue = vec!["a".to_string(), "b".to_string()];
    let children = children_of(&model(&[], &[app]), &AppType::Claude);
    assert_eq!(
        texts_of(&children),
        [
            "路由 · 故障转移开启中",
            "按队列自动选择，要调整请到应用页",
            "1. DeepSeek",
            "2. 智谱 GLM Coding Plan",
            "---",
            "打开 Claude Code 页面"
        ]
    );
    for row in &children[1..4] {
        assert!(
            matches!(
                row,
                TrayEntry::Item { enabled: false, .. } | TrayEntry::Check { enabled: false, .. }
            ),
            "{row:?}"
        );
    }
    assert!(matches!(
        &children[2],
        TrayEntry::Check { checked: true, .. }
    ));
}

#[test]
fn stack_submenu_lists_members_and_the_codex_official_card() {
    let mut official = entry("official", "OpenAI Official");
    official.official = true;
    let mut app = snapshot(
        AppType::Codex,
        TrayMode::Stack,
        vec![
            entry("deepseek", "DeepSeek"),
            entry("kimi", "Kimi For Coding"),
            entry("other", "Not In Stack"),
            official,
        ],
    );
    app.stack_members = vec!["deepseek".to_string(), "kimi".to_string()];
    let menu = model(&[], &[app]);
    assert_eq!(text_of(&menu[2]), "Codex · 聚合 · DeepSeek");
    let children = children_of(&menu, &AppType::Codex);
    assert_eq!(
        texts_of(&children),
        [
            "聚合 · 默认 DeepSeek",
            "DeepSeek",
            "Kimi For Coding",
            "OpenAI Official",
            "---",
            "打开 Codex 页面"
        ]
    );
}

#[test]
fn desktop_uses_the_same_mode_rows_as_the_routing_apps() {
    let mut mapping = entry("kimi", "Kimi For Coding");
    mapping.needs_routing = true;
    let mut official = entry("official", "Claude Desktop Official");
    official.official = true;
    official.blocked_from_routing = true;
    let direct = snapshot(
        AppType::ClaudeDesktop,
        TrayMode::Direct,
        vec![mapping.clone(), official.clone()],
    );
    let children = children_of(&model(&[], &[direct]), &AppType::ClaudeDesktop);
    // 直连下要经路由的卡点了去应用页弹「需要路由」，不直接切。
    assert!(
        matches!(&children[1], TrayEntry::Item { id, .. } if id == "nav:needs:claude-desktop:kimi")
    );

    let mut routed = snapshot(
        AppType::ClaudeDesktop,
        TrayMode::Route,
        vec![mapping, official],
    );
    routed.current_id = Some("kimi".to_string());
    let menu = model(&[], &[routed]);
    assert_eq!(text_of(&menu[2]), "Claude Desktop · 路由 · Kimi For Coding");
    let children = children_of(&menu, &AppType::ClaudeDesktop);
    assert!(
        matches!(&children[1], TrayEntry::Check { id, checked: true, .. } if id == "prov:claude-desktop:kimi")
    );
}

#[test]
fn service_down_shows_the_problem_row_and_needs_attention_instead_of_quota() {
    let mut mapping = entry("kimi", "Kimi For Coding");
    mapping.needs_routing = true;
    let mut desktop = snapshot(AppType::ClaudeDesktop, TrayMode::Route, vec![mapping]);
    desktop.service_down = true;
    desktop.quota = Some(QuotaView::Lines(vec![tier_line(
        &zh(),
        zh().tier_weekly,
        20.0,
        None,
    )]));
    let mut apps = [desktop];
    let problems = [TrayProblem::ServiceDown { port: 15721 }];
    mark_attention(&mut apps, &problems);
    let menu = model(&problems, &apps);
    assert_eq!(text_of(&menu[0]), "路由服务没在运行（端口 15721）");
    assert!(
        matches!(&menu[0], TrayEntry::Item { id, enabled: true, .. } if id == "nav:settings:routing")
    );
    assert_eq!(text_of(&menu[1]), "---");
    assert_eq!(text_of(&menu[2]), "打开 CC Switch");
    assert_eq!(
        text_of(&menu[4]),
        "Claude Desktop · 路由 · Kimi For Coding · 需要处理"
    );
    // 子菜单和其他路由应用一样以模式标题开头，服务没在跑由顶部问题区说。
    let children = children_of(&menu, &AppType::ClaudeDesktop);
    assert_eq!(text_of(&children[0]), "路由");
}

#[test]
fn problem_area_keeps_two_rows_then_says_how_many_more() {
    let problems = [
        TrayProblem::ServiceDown { port: 15721 },
        TrayProblem::AttachFailed {
            app: AppType::Claude,
            stack: false,
        },
        TrayProblem::SwitchFailed {
            app: AppType::Codex,
            reason: "config.toml 格式有误".to_string(),
        },
    ];
    let menu = model(&problems, &[]);
    assert_eq!(
        texts_of(&menu[..4]),
        [
            "路由服务没在运行（端口 15721）",
            "Claude Code：上次的路由没能接上，已回到直连",
            "还有 1 个问题，打开 CC Switch 查看",
            "---"
        ]
    );
    let switch = model(&problems[2..], &[]);
    assert_eq!(
        text_of(&switch[0]),
        "Codex 没切换成功：config.toml 格式有误"
    );
    assert!(matches!(&switch[0], TrayEntry::Item { id, .. } if id == "nav:app:codex:failed"));
}

#[test]
fn quota_title_and_almost_out_suffix_on_the_app_row() {
    let mut app = snapshot(
        AppType::Claude,
        TrayMode::Direct,
        vec![entry("official", "Claude Official")],
    );
    let quota = make_quota(
        "claude",
        true,
        vec![tier(TIER_FIVE_HOUR, 31.0), tier(TIER_SEVEN_DAY, 95.0)],
    );
    app.quota = format_subscription_quota(&zh(), &quota);
    assert_eq!(
        app_row_title(&zh(), &app),
        "Claude Code · Claude Official · 5 小时剩余 69% · 每周剩余 5% · 快用完"
    );
}

#[test]
fn app_row_title_writes_every_tier_including_the_roomy_five_hour_one() {
    // #8011：火山三档里 5 小时剩得最多，只留剩余最少两档时它总被挤掉。
    let mut app = snapshot(
        AppType::Claude,
        TrayMode::Direct,
        vec![entry("volc", "火山方舟")],
    );
    let volcengine = usage_result(
        true,
        vec![
            usage_data(Some(TIER_FIVE_HOUR), 10.0),
            usage_data(Some(TIER_WEEKLY_LIMIT), 60.0),
            usage_data(Some(TIER_MONTHLY), 70.0),
        ],
    );
    app.quota = format_script_result(&zh(), &volcengine);
    assert_eq!(
        app_row_title(&zh(), &app),
        "Claude Code · 火山方舟 · 5 小时剩余 90% · 每周剩余 40% · 每月剩余 30%"
    );
}

#[test]
fn projects_live_inside_the_app_submenu() {
    let mut app = snapshot(
        AppType::Codex,
        TrayMode::Direct,
        vec![entry("deepseek", "DeepSeek")],
    );
    app.profiles = Some(ProfileSection {
        scope: "codex",
        items: vec![
            ("p1".to_string(), "个人项目".to_string()),
            ("p2".to_string(), "公司项目".to_string()),
        ],
        current: Some("p2".to_string()),
    });
    let children = children_of(&model(&[], &[app]), &AppType::Codex);
    assert_eq!(
        texts_of(&children),
        [
            "直连",
            "DeepSeek",
            "---",
            "项目",
            "不使用项目",
            "个人项目",
            "公司项目",
            "---",
            "打开 Codex 页面"
        ]
    );
    assert!(
        matches!(&children[4], TrayEntry::Check { id, checked: false, .. } if id == "profile_none_codex")
    );
    assert!(
        matches!(&children[6], TrayEntry::Check { id, checked: true, .. } if id == "profile_codex_p2")
    );
}

#[test]
fn long_and_duplicate_names_stay_readable() {
    let long = "An Extremely Long Provider Name That Keeps Going On";
    let mut first = entry("a", "OpenAI Official");
    first.hint = Some("me@example.com".to_string());
    let second = entry("b", "OpenAI Official");
    let third = entry("c", long);
    let names = display_names(&[&first, &second, &third]);
    assert_eq!(names[0], "OpenAI Official · me@example.com");
    assert_eq!(names[1], "OpenAI Official #2");
    assert_eq!(names[2].chars().count(), MAX_NAME_CHARS);
    assert!(names[2].ends_with('…'));
}

#[test]
fn lightweight_mode_is_a_check_item() {
    let menu = build_menu_model(&en(), &MenuStatus::default(), &[], true, now());
    assert!(menu.iter().any(|entry| matches!(entry,
        TrayEntry::Check { id, checked: true, text, .. }
            if id == "lightweight_mode" && text == "Lightweight mode")));
}

#[test]
fn stack_header_falls_back_to_the_generic_word_without_a_default() {
    let mut app = snapshot(
        AppType::Claude,
        TrayMode::Stack,
        vec![entry("deepseek", "DeepSeek")],
    );
    app.current_id = None;
    app.stack_members = vec!["deepseek".to_string()];
    let children = children_of(&model(&[], &[app.clone()]), &AppType::Claude);
    assert_eq!(text_of(&children[0]), "聚合 · 默认供应商");
    app.current_id = Some("deepseek".to_string());
    let menu = build_menu_model(&en(), &MenuStatus::default(), &[app], false, now());
    let children = children_of(&menu, &AppType::Claude);
    assert_eq!(text_of(&children[0]), "Aggregation · Default: DeepSeek");
}

#[test]
fn quit_says_the_routing_service_stops_only_while_it_is_in_use() {
    let quit_text = |texts: &TrayTexts, routing: bool| {
        let status = MenuStatus {
            routing_in_use: routing,
            ..Default::default()
        };
        let menu = build_menu_model(texts, &status, &[], false, now());
        match menu.last() {
            Some(TrayEntry::Item { id, text, .. }) if id == "quit" => text.clone(),
            other => panic!("last entry is not quit: {other:?}"),
        }
    };
    assert_eq!(quit_text(&zh(), false), "退出 CC Switch");
    assert_eq!(quit_text(&zh(), true), "退出 CC Switch（路由服务会停止）");
    assert_eq!(
        quit_text(&en(), true),
        "Quit CC Switch (stops the routing service)"
    );
    for &language in crate::settings::LANGUAGES {
        let texts = TrayTexts::from_language(language);
        assert_ne!(texts.quit, texts.quit_stops_routing, "{language}");
        assert!(
            texts.quit_stops_routing.starts_with(texts.quit),
            "{language}"
        );
        assert!(texts.header_stack_default.contains("{name}"), "{language}");
        assert!(texts.tooltip_problem.contains("{problem}"), "{language}");
        for template in [
            texts.feedback_switched,
            texts.feedback_stack_default,
            texts.feedback_profile,
        ] {
            assert!(
                template.contains("{app}") && template.contains("{name}"),
                "{language}: {template}"
            );
        }
    }
}

#[test]
fn tooltip_names_the_first_problem_and_is_plain_otherwise() {
    assert_eq!(tray_tooltip(&zh(), &[]), "CC Switch");
    let problems = [
        TrayProblem::ServiceDown { port: 15721 },
        TrayProblem::SwitchFailed {
            app: AppType::Codex,
            reason: "boom".to_string(),
        },
    ];
    assert_eq!(
        tray_tooltip(&zh(), &problems),
        "CC Switch · 需要处理：路由服务没在运行（端口 15721）"
    );
    let long = [TrayProblem::SwitchFailed {
        app: AppType::Codex,
        reason: "x".repeat(500),
    }];
    assert!(tray_tooltip(&en(), &long).chars().count() <= MAX_TOOLTIP_CHARS);
}

#[test]
fn problem_dot_sits_bottom_right_with_a_clear_ring() {
    let size = 72u32;
    let pixel = |image: &Image<'_>, x: u32, y: u32| {
        let i = ((y * size + x) * 4) as usize;
        image.rgba()[i..i + 4].to_vec()
    };
    // 整张图都是不透明白色，方便看哪里被挖掉、哪里画了点。
    let base = Image::new_owned(vec![255; (size * size * 4) as usize], size, size);

    let colored = icon_with_problem_dot(&base, false);
    let dot_center = size - (size as f32 * DOT_RING_RADIUS) as u32;
    assert_eq!(
        pixel(&colored, dot_center, dot_center),
        vec![0xF7, 0x63, 0x0C, 255]
    );
    // 圆点和圈之间是透明的。
    let ring = dot_center - (size as f32 * (DOT_RADIUS + DOT_RING_RADIUS) / 2.0) as u32;
    assert_eq!(pixel(&colored, ring, dot_center)[3], 0);
    // 左上角不动。
    assert_eq!(pixel(&colored, 2, 2), vec![255, 255, 255, 255]);

    let template = icon_with_problem_dot(&base, true);
    assert_eq!(pixel(&template, dot_center, dot_center), vec![0, 0, 0, 255]);
    assert_eq!(pixel(&template, 2, 2), vec![255, 255, 255, 255]);

    // 尺寸对不上的图原样返回，不越界。
    let broken = Image::new_owned(vec![1, 2, 3], 4, 4);
    assert_eq!(icon_with_problem_dot(&broken, false).rgba(), &[1, 2, 3]);
}

#[test]
fn feedback_only_for_switches_that_need_a_client_restart() {
    let name = || "DeepSeek".to_string();
    for app in [AppType::Codex, AppType::Gemini, AppType::GrokBuild] {
        assert_eq!(
            switch_feedback(&app, TrayMode::Direct, name()),
            Some(TrayFeedback::Switched {
                app: app.clone(),
                name: name()
            })
        );
        // 路由 / 故障转移换一家立即生效。
        assert_eq!(switch_feedback(&app, TrayMode::Route, name()), None);
        assert_eq!(switch_feedback(&app, TrayMode::Failover, name()), None);
    }
    assert_eq!(
        switch_feedback(&AppType::Claude, TrayMode::Direct, name()),
        None
    );
    // Desktop 只在启动时读配置：直连、路由换一家都要重启。
    for mode in [TrayMode::Direct, TrayMode::Route] {
        assert!(matches!(
            switch_feedback(&AppType::ClaudeDesktop, mode, name()),
            Some(TrayFeedback::Switched { .. })
        ));
    }
    for app in [AppType::Claude, AppType::Codex] {
        assert!(matches!(
            switch_feedback(&app, TrayMode::Stack, name()),
            Some(TrayFeedback::StackDefault { .. })
        ));
    }
}

#[test]
fn feedback_row_is_a_grey_line_after_the_problems() {
    let status = MenuStatus {
        problems: vec![TrayProblem::ServiceDown { port: 15721 }],
        feedback: Some(TrayFeedback::Switched {
            app: AppType::Codex,
            name: "DeepSeek".to_string(),
        }),
        routing_in_use: false,
        update_ready: None,
    };
    let menu = build_menu_model(&zh(), &status, &[], false, now());
    assert_eq!(
        texts_of(&menu[..4]),
        [
            "路由服务没在运行（端口 15721）",
            "Codex 已切换到 DeepSeek，重启 Codex 后生效",
            "---",
            "打开 CC Switch",
        ]
    );
    assert!(matches!(&menu[1], TrayEntry::Item { enabled: false, .. }));

    let only_feedback = MenuStatus {
        feedback: Some(TrayFeedback::StackDefault {
            app: AppType::Codex,
            name: "Kimi For Coding".to_string(),
        }),
        ..Default::default()
    };
    let menu = build_menu_model(&zh(), &only_feedback, &[], false, now());
    assert_eq!(
        texts_of(&menu[..3]),
        [
            "Codex 的默认供应商改为 Kimi For Coding，重启 Codex 后生效",
            "---",
            "打开 CC Switch",
        ]
    );
    assert_eq!(
        feedback_text(
            &zh(),
            &TrayFeedback::ProfileApplied {
                app: AppType::Claude,
                name: "公司项目".to_string()
            }
        ),
        "已把项目「公司项目」用到 Claude Code"
    );
    assert_eq!(
        feedback_text(
            &en(),
            &TrayFeedback::Switched {
                app: AppType::ClaudeDesktop,
                name: "DeepSeek".to_string()
            }
        ),
        "Claude Desktop switched to DeepSeek. Restart Claude Desktop to apply"
    );
}

#[test]
fn feedback_goes_away_after_the_panel_showed_it_or_two_minutes() {
    let record = |at: std::time::Instant| {
        *FEEDBACK.lock_recover() = Some(FeedbackRecord {
            feedback: TrayFeedback::Switched {
                app: AppType::Codex,
                name: "DeepSeek".to_string(),
            },
            at,
            seen: false,
        });
    };
    record(std::time::Instant::now());
    assert!(current_feedback().is_some());
    // 面板收起 = 这次面板里已经显示过。
    mark_feedback_seen();
    assert!(current_feedback().is_none());
    assert!(FEEDBACK.lock_recover().is_none());

    if let Some(at) = std::time::Instant::now().checked_sub(FEEDBACK_TTL) {
        record(at);
        assert!(current_feedback().is_none());
    }
}

// ─── 托盘 → 主界面 ───

#[test]
fn nav_ids_parse_into_navigation_requests() {
    assert_eq!(
        parse_nav_id("nav:needs:claude:pkg:with:colons"),
        Some(TrayNavigation {
            app: Some("claude".to_string()),
            intent: Some("needsRoute".to_string()),
            provider_id: Some("pkg:with:colons".to_string()),
            ..Default::default()
        })
    );
    assert_eq!(
        parse_nav_id("nav:app:claude-desktop:quota"),
        Some(TrayNavigation {
            app: Some("claude-desktop".to_string()),
            ..Default::default()
        })
    );
    assert_eq!(
        parse_nav_id("nav:add:grokbuild"),
        Some(TrayNavigation {
            app: Some("grokbuild".to_string()),
            intent: Some("add".to_string()),
            ..Default::default()
        })
    );
    assert_eq!(
        parse_nav_id("nav:settings:routing"),
        Some(TrayNavigation {
            section: Some("routing".to_string()),
            ..Default::default()
        })
    );
    assert_eq!(parse_nav_id("nav:main"), Some(TrayNavigation::default()));
    assert_eq!(parse_nav_id("nav:needs:claude"), None);
    assert_eq!(parse_nav_id("nav:app:nope"), None);
    assert_eq!(parse_nav_id("prov:claude:x"), None);
    let json = serde_json::to_value(parse_nav_id("nav:needs:codex:p1").unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"app": "codex", "intent": "needsRoute", "providerId": "p1"})
    );
}

#[test]
fn problems_drop_attach_failures_once_the_app_routes_again() {
    ATTACH_FAILURES.lock_recover().clear();
    SWITCH_FAILURES.lock_recover().clear();
    ATTACH_FAILURES
        .lock_recover()
        .push((AppType::Claude, false));
    record_switch_failure(&AppType::Codex, "boom".to_string());
    let visible = [(AppType::Claude, true), (AppType::Codex, true)];
    assert_eq!(collect_problems(None, &visible).len(), 2);
    // Claude Code 重新进入路由：「退回直连」的问题行消失。
    let rerouted = [(AppType::Claude, false), (AppType::Codex, true)];
    assert_eq!(
        collect_problems(Some(15721), &rerouted),
        [
            TrayProblem::ServiceDown { port: 15721 },
            TrayProblem::SwitchFailed {
                app: AppType::Codex,
                reason: "boom".to_string()
            }
        ]
    );
    assert!(clear_app_problems(&AppType::Codex));
    assert!(!clear_app_problems(&AppType::Codex));
    assert!(collect_problems(None, &rerouted).is_empty());
}
