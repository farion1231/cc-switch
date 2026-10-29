use super::*;

#[test]
fn official_gpt_reasoning_overrides_apply_to_exact_and_alias_models() {
    with_official_models(official_gpt_rows(), || {
        for profile in [
            CodexCatalogToolProfile::NativeResponses,
            CodexCatalogToolProfile::ProxyChat,
        ] {
            for (model, levels, default, expected_default) in [
                ("gpt-6-sol", json!(["max", "ultra"]), "max", "max"),
                ("gpt-6-sol-relay", json!(["ultra"]), "low", "ultra"),
                ("openai/gpt-6-sol", json!(["xhigh", "low"]), "", "low"),
                ("gpt-6-luna", json!(["ultra"]), "", "ultra"),
            ] {
                let entries = catalog_for(
                    json!([{
                        "model": model,
                        "reasoningLevels": levels,
                        "defaultReasoningLevel": default,
                    }]),
                    "",
                    profile,
                );
                let entry = &entries[0];
                let actual: Vec<&str> = entry["supported_reasoning_levels"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|level| level["effort"].as_str().unwrap())
                    .collect();
                let expected = if model == "openai/gpt-6-sol" {
                    vec!["low", "xhigh"]
                } else if model == "gpt-6-sol" {
                    vec!["max", "ultra"]
                } else {
                    vec!["ultra"]
                };
                assert_eq!(actual, expected, "{model}: {profile:?}");
                assert_eq!(entry["default_reasoning_level"], expected_default);
                assert_eq!(entry["slug"], model);
                assert_eq!(entry["apply_patch_tool_type"], "freeform");
                assert_eq!(entry["context_window"], 272_000);
                assert_eq!(entry["max_context_window"], 872_000);
                assert_eq!(entry["tool_mode"], "code_mode_only");
                assert_eq!(
                    entry["model_messages"]["instructions_template"],
                    if model == "gpt-6-luna" {
                        "GPT-6 base prompt"
                    } else {
                        "GPT-6 Sol prompt"
                    }
                );
            }
        }
    });
}

#[test]
fn absent_or_invalid_reasoning_overrides_keep_official_defaults() {
    with_official_models(official_gpt_rows(), || {
        for levels in [Value::Null, json!([]), json!(["unsupported"])] {
            let entries = catalog_for(
                json!([{
                    "model": "gpt-6-sol",
                    "reasoningLevels": levels,
                    "defaultReasoningLevel": "ultra",
                }]),
                "",
                CodexCatalogToolProfile::NativeResponses,
            );
            assert_eq!(entries[0]["default_reasoning_level"], "low");
            assert_eq!(
                entries[0]["supported_reasoning_levels"],
                json!([
                    { "effort": "low", "description": "l" },
                    { "effort": "xhigh", "description": "x" },
                ])
            );
        }
    });
}

#[test]
fn stack_keeps_third_party_overrides_and_native_official_levels() {
    with_official_models(official_gpt_rows(), || {
        let settings = json!({ "modelCatalog": { "models": [{
            "model": "gpt-6-sol", "reasoningLevels": ["max", "ultra"],
        }] } });
        for official_route in [false, true] {
            let route = if official_route {
                CodexStackRoute::Official {
                    native: official_gpt_rows(),
                    config_text: "",
                }
            } else {
                CodexStackRoute::ThirdParty(CodexCatalogRow {
                    settings: &settings,
                    config_text: "",
                    profile: CodexCatalogToolProfile::NativeResponses,
                })
            };
            let catalog = plan_codex_stack_catalog(
                route,
                &[CodexStackCatalogMember {
                    key: "relay",
                    provider_name: "Relay",
                    row: CodexCatalogRow {
                        settings: &settings,
                        config_text: "",
                        profile: CodexCatalogToolProfile::NativeResponses,
                    },
                }],
                false,
            )
            .unwrap();
            let models = catalog["models"].as_array().unwrap();
            let member = models
                .iter()
                .find(|m| m["slug"] == "ccs-relay/gpt-6-sol")
                .unwrap();
            let efforts: Vec<&str> = member["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|level| level["effort"].as_str().unwrap())
                .collect();
            assert_eq!(efforts, ["max", "ultra"]);
            assert_eq!(member["default_reasoning_level"], "ultra");
            let route_model = models.iter().find(|m| m["slug"] == "gpt-6-sol").unwrap();
            assert_eq!(
                route_model["default_reasoning_level"],
                if official_route { "low" } else { "ultra" }
            );
        }
    });
}

#[test]
fn official_gpt_reasoning_overrides_survive_catalog_recovery() {
    with_official_models(official_gpt_rows(), || {
        for (levels, default) in [
            (json!(["max", "ultra"]), "max"),
            (json!(["low", "xhigh"]), "xhigh"),
        ] {
            let mut entry = official_gpt_row("gpt-6-sol", "GPT-6 Sol prompt");
            entry["supported_reasoning_levels"] = json!(levels
                .as_array()
                .unwrap()
                .iter()
                .map(|effort| json!({ "effort": effort, "description": "declared" }))
                .collect::<Vec<Value>>());
            entry["default_reasoning_level"] = json!(default);
            let recovered =
                build_simplified_catalog_from_texts("", &json!({ "models": [entry] }).to_string())
                    .unwrap();
            let row = &recovered["models"][0];
            assert_eq!(row["reasoningLevels"], levels);
            assert_eq!(row["defaultReasoningLevel"], default);
            assert_eq!(row["model"], "gpt-6-sol");
            assert!(row.get("displayName").is_none());
            assert!(row.get("inputModalities").is_none());
            let generated = codex_model_catalog_from_settings(
                &json!({ "modelCatalog": recovered }),
                "",
                CodexCatalogToolProfile::NativeResponses,
            )
            .unwrap()
            .unwrap();
            let actual: Vec<&str> = generated["models"][0]["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|level| level["effort"].as_str().unwrap())
                .collect();
            let expected: Vec<&str> = levels
                .as_array()
                .unwrap()
                .iter()
                .map(|level| level.as_str().unwrap())
                .collect();
            assert_eq!(actual, expected);
            assert_eq!(generated["models"][0]["default_reasoning_level"], default);
        }
    });
}

fn legacy_official_rows() -> Vec<Value> {
    let mut rows = official_gpt_rows();
    for row in &mut rows {
        row.as_object_mut().unwrap().remove("model_messages");
    }
    normalize_codex_native_rows(rows).unwrap()
}

#[test]
fn legacy_official_reasoning_overrides_survive_catalog_recovery() {
    with_official_models(legacy_official_rows(), || {
        for profile in [
            CodexCatalogToolProfile::NativeResponses,
            CodexCatalogToolProfile::ProxyChat,
        ] {
            for model in ["gpt-6-sol", "openai/gpt-6-sol"] {
                for spec in [
                    json!({
                        "model": model, "reasoningLevels": ["max", "ultra"],
                        "defaultReasoningLevel": "max",
                    }),
                    json!({
                        "model": model, "reasoningLevels": ["low", "xhigh"],
                        "defaultReasoningLevel": "xhigh",
                    }),
                    json!({ "model": model }),
                ] {
                    let models = json!([spec]);
                    let catalog = json!({ "models": catalog_for(models.clone(), "", profile) });
                    assert!(catalog["models"][0].get("model_messages").is_none());
                    assert_eq!(
                        catalog["models"][0]["base_instructions"],
                        "GPT-6 Sol prompt"
                    );
                    let recovered =
                        build_simplified_catalog_from_texts("", &catalog.to_string()).unwrap();
                    assert_eq!(
                        recovered["models"][0]["reasoningLevels"],
                        models[0]["reasoningLevels"]
                    );
                    assert_eq!(recovered["models"], models, "{model}: {profile:?}");
                    let regenerated = codex_model_catalog_from_settings(
                        &json!({ "modelCatalog": recovered }),
                        "",
                        profile,
                    )
                    .unwrap()
                    .unwrap();
                    assert_eq!(regenerated, catalog);
                }
            }
        }
    });
}

#[test]
fn legacy_official_match_keeps_template_rows_user_values() {
    let rows = legacy_official_rows();
    let settings = json!({ "modelCatalog": { "models": [{
        "model": "gpt-6-sol", "displayName": "GPT-6-SOL",
        "baseInstructions": "GPT-6 Sol prompt",
        "inputModalities": rows[1]["input_modalities"],
        "supportsParallelToolCalls": false,
    }] } });
    with_official_models(rows.clone(), || {
        for profile in [
            CodexCatalogToolProfile::Anthropic,
            CodexCatalogToolProfile::ProxyChat,
        ] {
            // Anthropic has the same prompt but different tools; the legacy Chat
            // template has the same tools but a different model's prompt.
            let catalog = codex_model_catalog_from_specs(
                &codex_catalog_model_specs(&settings),
                &rows[2],
                profile,
                128_000,
            );
            let recovered = build_simplified_catalog_from_texts("", &catalog.to_string()).unwrap();
            assert_eq!(recovered["models"][0]["displayName"], "GPT-6-SOL");
            assert_eq!(recovered["models"][0]["supportsParallelToolCalls"], false);
        }
    });
}
