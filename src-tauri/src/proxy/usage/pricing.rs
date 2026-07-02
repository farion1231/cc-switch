//! LiteLLM-backed pricing catalog for proxy requests.

use super::calculator::ModelPricing;
use once_cell::sync::Lazy;
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

const LITELLM_PRICING_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const EMBEDDED_PRICING: &str = include_str!("litellm-pricing.json");

static CATALOG: Lazy<RwLock<HashMap<String, ModelPricing>>> = Lazy::new(|| {
    RwLock::new(parse_catalog(EMBEDDED_PRICING).unwrap_or_else(|error| {
        log::error!("Failed to load embedded LiteLLM pricing: {error}");
        HashMap::new()
    }))
});
static REFRESH_STARTED: AtomicBool = AtomicBool::new(false);

pub fn find(model: &str) -> Option<ModelPricing> {
    refresh_in_background();
    let catalog = CATALOG.read().ok()?;
    find_in_catalog(&catalog, model).cloned()
}

fn refresh_in_background() {
    if REFRESH_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async {
        match reqwest::get(LITELLM_PRICING_URL).await {
            Ok(response) if response.status().is_success() => match response.text().await {
                Ok(body) => match parse_catalog(&body) {
                    Ok(catalog) if !catalog.is_empty() => match CATALOG.write() {
                        Ok(mut active) => {
                            *active = catalog;
                            log::info!("Refreshed LiteLLM pricing catalog");
                        }
                        Err(error) => {
                            log::warn!("Failed to replace LiteLLM pricing catalog: {error}")
                        }
                    },
                    Ok(_) => log::warn!("Ignored empty LiteLLM pricing refresh"),
                    Err(error) => log::warn!("Ignored invalid LiteLLM pricing refresh: {error}"),
                },
                Err(error) => log::warn!("Failed to read LiteLLM pricing refresh: {error}"),
            },
            Ok(response) => log::warn!(
                "LiteLLM pricing refresh returned HTTP {}",
                response.status()
            ),
            Err(error) => log::warn!("LiteLLM pricing refresh failed: {error}"),
        }
    });
}

fn parse_catalog(json: &str) -> Result<HashMap<String, ModelPricing>, String> {
    let root: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let entries = root.as_object().ok_or("pricing root must be an object")?;
    let mut catalog = HashMap::new();
    for (model, entry) in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some(input) = rate(entry, "input_cost_per_token") else {
            continue;
        };
        let Some(output) = rate(entry, "output_cost_per_token") else {
            continue;
        };
        let pricing = ModelPricing {
            input_cost_per_million: input,
            output_cost_per_million: output,
            cache_read_cost_per_million: rate(entry, "cache_read_input_token_cost")
                .unwrap_or(Decimal::ZERO),
            cache_creation_cost_per_million: rate(entry, "cache_creation_input_token_cost")
                .unwrap_or(Decimal::ZERO),
            input_cost_above_200k_per_million: rate(
                entry,
                "input_cost_per_token_above_200k_tokens",
            ),
            output_cost_above_200k_per_million: rate(
                entry,
                "output_cost_per_token_above_200k_tokens",
            ),
            cache_read_cost_above_200k_per_million: rate(
                entry,
                "cache_read_input_token_cost_above_200k_tokens",
            ),
            cache_creation_cost_above_200k_per_million: rate(
                entry,
                "cache_creation_input_token_cost_above_200k_tokens",
            ),
        };
        catalog.insert(normalize(model), pricing.clone());
        if let Some(short_name) = model.rsplit(['/', '.']).next() {
            catalog.entry(normalize(short_name)).or_insert(pricing);
        }
    }
    Ok(catalog)
}

fn rate(entry: &serde_json::Map<String, Value>, key: &str) -> Option<Decimal> {
    let raw = entry.get(key)?.to_string();
    let per_token = Decimal::from_scientific(&raw)
        .or_else(|_| Decimal::from_str(&raw))
        .ok()?;
    Some(per_token * Decimal::from(1_000_000))
}

fn find_in_catalog<'a>(
    catalog: &'a HashMap<String, ModelPricing>,
    model: &str,
) -> Option<&'a ModelPricing> {
    let normalized = normalize(model);
    if let Some(pricing) = catalog.get(&normalized) {
        return Some(pricing);
    }
    if let Some(path_tail) = model.rsplit('/').next().filter(|tail| *tail != model) {
        let normalized_tail = normalize(path_tail);
        if let Some(pricing) = catalog.get(&normalized_tail) {
            return Some(pricing);
        }
        if let Some(base_tail) = path_tail.split(':').next() {
            if let Some(pricing) = catalog.get(&normalize(base_tail)) {
                return Some(pricing);
            }
        }
    }
    let mut matching = catalog
        .iter()
        .filter(|(key, _)| normalized.starts_with(key.as_str()) || key.starts_with(&normalized))
        .filter(|(key, _)| key.len() >= 4)
        .max_by_key(|(key, _)| key.len());
    matching.take().map(|(_, pricing)| pricing)
}

fn normalize(model: &str) -> String {
    model
        .trim()
        .replace(['.', '@', '_'], "-")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_matches_litellm_pricing() {
        let catalog = parse_catalog(
            r#"{"gpt-5":{"input_cost_per_token":0.000001,"output_cost_per_token":0.00001}}"#,
        )
        .unwrap();
        assert!(find_in_catalog(&catalog, "openai/gpt-5:latest").is_some());
    }

    #[test]
    fn preserves_200k_tiers() {
        let catalog = parse_catalog(r#"{"claude":{"input_cost_per_token":0.000003,"output_cost_per_token":0.000015,"input_cost_per_token_above_200k_tokens":0.000006}}"#).unwrap();
        assert_eq!(
            catalog["claude"].input_cost_above_200k_per_million,
            Some(Decimal::from(6))
        );
    }

    #[test]
    fn preserves_provider_qualified_pricing_keys() {
        let catalog = parse_catalog(
            r#"{
                "anthropic.claude-haiku-4-5-20251001-v1:0":{
                    "input_cost_per_token":0.000001,
                    "output_cost_per_token":0.000002
                },
                "bedrock/us-gov-west-1/anthropic.claude-haiku-4-5-20251001-v1:0":{
                    "input_cost_per_token":0.000003,
                    "output_cost_per_token":0.000004
                }
            }"#,
        )
        .unwrap();

        let standard = find_in_catalog(&catalog, "anthropic.claude-haiku-4-5-20251001-v1:0")
            .expect("standard Anthropic price");
        let gov = find_in_catalog(
            &catalog,
            "bedrock/us-gov-west-1/anthropic.claude-haiku-4-5-20251001-v1:0",
        )
        .expect("gov Bedrock price");

        assert_eq!(standard.input_cost_per_million, Decimal::from(1));
        assert_eq!(gov.input_cost_per_million, Decimal::from(3));
    }

    #[test]
    fn preserves_fine_tune_model_suffixes() {
        assert_eq!(
            normalize("ft:gpt-4o-mini:org:job"),
            "ft:gpt-4o-mini:org:job"
        );
    }
}
