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
