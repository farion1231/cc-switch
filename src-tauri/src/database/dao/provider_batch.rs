//! Append-only provider batches. Preview and commit use the same plan under the DB lock.

use crate::{
    database::{lock_conn, Database},
    error::AppError,
    provider::Provider,
};
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) struct ProviderBatchPlan {
    pub duplicates: Vec<bool>,
    pub same_names: Vec<bool>,
    pub token: String,
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, canonical(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        other => other,
    }
}

fn fingerprint(
    app: &str,
    id: &str,
    name: &str,
    mut config: Value,
    mut meta: Value,
    category: Option<&str>,
    endpoints: &[String],
) -> Option<[u8; 32]> {
    if matches!(app, "codex" | "grokbuild") {
        let text = config.get("config")?.as_str()?;
        let parsed: toml::Value = toml::from_str(text).ok()?;
        config["config"] = serde_json::to_value(parsed).ok()?;
    }
    let primary = normalize_config_endpoint(app, &mut config);
    // The regular Add form records the primary URL in the endpoint table too.
    // Only a genuinely different URL makes this a multi-endpoint configuration.
    if endpoints
        .iter()
        .any(|endpoint| normalize_url(endpoint).is_none_or(|endpoint| Some(endpoint) != primary))
    {
        return None;
    }
    if app == "kimi" {
        let models = config.get_mut("models")?.as_object_mut()?;
        let owner_prefix = format!("{id}/");
        let mut normalized = serde_json::Map::new();
        for (alias, mut model) in std::mem::take(models) {
            if model
                .get("provider")
                .is_some_and(|owner| owner.as_str() != Some(id))
            {
                return None;
            }
            model.as_object_mut()?.remove("provider");
            // Tag relative and literal aliases, so a real '<owner>/x' alias cannot
            // collide with the normalized identity of an automatically generated one.
            let alias = match alias.strip_prefix(&owner_prefix) {
                Some(tail) => serde_json::json!([true, tail]).to_string(),
                None => serde_json::json!([false, alias]).to_string(),
            };
            // Do not merge distinct aliases after normalization.
            if normalized.insert(alias, model).is_some() {
                return None;
            }
        }
        *models = normalized;
    }
    if let Some(meta) = meta.as_object_mut() {
        // Activation is device state, never part of a connection's identity.
        meta.remove("liveConfigManaged");
        let native = match app {
            "claude" | "claude-desktop" => "anthropic",
            "codex" | "grokbuild" => "openai_responses",
            _ => "",
        };
        if meta.get("apiFormat").and_then(Value::as_str) == Some(native) {
            meta.remove("apiFormat");
        }
        // Auto-selection cannot change a single-endpoint connection.
        meta.remove("endpointAutoSelect");
    }
    let value = serde_json::json!([app, name, config, meta, category.unwrap_or("custom")]);
    Some(Sha256::digest(serde_json::to_vec(&canonical(value)).ok()?).into())
}

fn normalize_url(url: &str) -> Option<String> {
    url::Url::parse(url.trim())
        .ok()
        .map(|url| url.to_string().trim_end_matches('/').to_string())
}

fn normalize_config_endpoint(app: &str, config: &mut Value) -> Option<String> {
    let value = match app {
        "claude" | "claude-desktop" => config.pointer_mut("/env/ANTHROPIC_BASE_URL"),
        "gemini" | "agy" => config.pointer_mut("/env/GOOGLE_GEMINI_BASE_URL"),
        "opencode" | "mcode" => config.pointer_mut("/options/baseURL"),
        "kimi" => config.pointer_mut("/provider/base_url"),
        "hermes" => config.get_mut("base_url"),
        "codex" => {
            let config = config.get_mut("config")?;
            let key = config.get("model_provider")?.as_str()?.to_owned();
            config
                .get_mut("model_providers")?
                .get_mut(key)?
                .get_mut("base_url")
        }
        "grokbuild" => {
            let config = config.get_mut("config")?;
            let key = config.get("models")?.get("default")?.as_str()?.to_owned();
            config.get_mut("model")?.get_mut(key)?.get_mut("base_url")
        }
        _ => config.get_mut("baseUrl"),
    }?;
    let normalized = normalize_url(value.as_str()?)?;
    *value = Value::String(normalized.clone());
    Some(normalized)
}

fn plan(
    conn: &Connection,
    app: &str,
    providers: &[Provider],
) -> Result<ProviderBatchPlan, AppError> {
    let mut known = HashSet::new();
    let mut names = HashSet::new();
    let mut endpoints: HashMap<String, Vec<String>> = HashMap::new();
    let mut stmt =
        conn.prepare("SELECT provider_id,url FROM provider_endpoints WHERE app_type=?1")?;
    for row in stmt.query_map([app], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (id, url) = row?;
        endpoints.entry(id).or_default().push(url);
    }
    let mut stmt = conn.prepare(
        "SELECT name, settings_config, meta, category, id FROM providers WHERE app_type=?1",
    )?;
    let existing = stmt.query_map([app], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in existing {
        let (name, config, meta, category, id) = row?;
        if let (Ok(config), Ok(meta)) = (serde_json::from_str(&config), serde_json::from_str(&meta))
        {
            if let Some(hash) = fingerprint(
                app,
                &id,
                &name,
                config,
                meta,
                category.as_deref(),
                endpoints.get(&id).map(Vec::as_slice).unwrap_or_default(),
            ) {
                known.insert(hash);
            }
        }
        names.insert(name);
    }
    let mut duplicates = Vec::with_capacity(providers.len());
    let mut same_names = Vec::with_capacity(providers.len());
    let mut digest = Sha256::new();
    digest.update(app.as_bytes());
    for provider in providers {
        let meta = serde_json::to_value(provider.meta.clone().unwrap_or_default())
            .map_err(|_| AppError::Message("provider.bulkImport.errors.invalidConfig".into()))?;
        let hash = fingerprint(
            app,
            &provider.id,
            &provider.name,
            provider.settings_config.clone(),
            meta,
            provider.category.as_deref(),
            &[],
        )
        .ok_or_else(|| AppError::Message("provider.bulkImport.errors.invalidConfig".into()))?;
        let duplicate = !known.insert(hash);
        let same_name = !names.insert(provider.name.clone());
        digest.update(hash);
        digest.update([u8::from(duplicate), u8::from(same_name)]);
        duplicates.push(duplicate);
        same_names.push(same_name);
    }
    Ok(ProviderBatchPlan {
        duplicates,
        same_names,
        token: format!("{:x}", digest.finalize()),
    })
}

impl Database {
    pub(crate) fn preview_provider_batch(
        &self,
        app: &str,
        providers: &[Provider],
    ) -> Result<ProviderBatchPlan, AppError> {
        let conn = lock_conn!(self.conn);
        plan(&conn, app, providers)
    }

    pub(crate) fn append_provider_batch(
        &self,
        app: &str,
        providers: &[Provider],
        expected_token: &str,
    ) -> Result<(Vec<String>, usize), AppError> {
        let mut conn = lock_conn!(self.conn);
        // Reserve the writer before planning: other connections cannot both plan inserts.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = plan(&tx, app, providers)?;
        if current.token != expected_token {
            return Err(AppError::Message(
                "provider.bulkImport.errors.previewChanged".into(),
            ));
        }
        let (max_order, has_unordered, latest): (i64, bool, i64) = tx.query_row(
            "SELECT COALESCE(MAX(sort_index),-1), COUNT(*) > COUNT(sort_index), COALESCE(MAX(created_at),0)
             FROM providers WHERE app_type=?1",
            [app],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let mut order = max_order.max(-1);
        let mut created_at = chrono::Utc::now().timestamp_millis().max(latest);
        let mut added = Vec::new();
        let mut skipped = 0;
        for (provider, duplicate) in providers.iter().zip(current.duplicates) {
            if duplicate {
                skipped += 1;
                continue;
            }
            order = order.checked_add(1).ok_or_else(|| {
                AppError::Message("provider.bulkImport.errors.storageFailed".into())
            })?;
            created_at = created_at.checked_add(1).ok_or_else(|| {
                AppError::Message("provider.bulkImport.errors.storageFailed".into())
            })?;
            // The renderer places NULL sort indexes last. Keep that bucket when it exists,
            // then append by creation time without rewriting existing providers' order.
            let sort_index = (!has_unordered).then_some(order);
            let config = serde_json::to_string(&provider.settings_config).map_err(|_| {
                AppError::Message("provider.bulkImport.errors.invalidConfig".into())
            })?;
            let meta = serde_json::to_string(&provider.meta.clone().unwrap_or_default()).map_err(
                |_| AppError::Message("provider.bulkImport.errors.invalidConfig".into()),
            )?;
            // No UPSERT and no current/failover mutation: an ID collision aborts the whole batch.
            let inserted = tx.execute("INSERT INTO providers(id,app_type,name,settings_config,website_url,category,created_at,sort_index,notes,icon,icon_color,meta,is_current,in_failover_queue) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0,0)",
                params![provider.id,app,provider.name,config,provider.website_url,provider.category,created_at,sort_index,provider.notes,provider.icon,provider.icon_color,meta])?;
            if inserted != 1 {
                return Err(AppError::Message(
                    "provider.bulkImport.errors.storageFailed".into(),
                ));
            }
            added.push(provider.id.clone());
        }
        tx.commit()?;
        Ok((added, skipped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn conflicting_id_rolls_back_prior_inserts_without_overwriting() {
        let db = Database::memory().unwrap();
        let existing = Provider::with_id(
            "collision".into(),
            "Existing".into(),
            json!({"env":{}}),
            None,
        );
        db.save_provider("claude", &existing).unwrap();
        let candidates = vec![
            Provider::with_id("new".into(), "First".into(), json!({"env":{}}), None),
            Provider::with_id(
                "collision".into(),
                "Replacement".into(),
                json!({"env":{}}),
                None,
            ),
        ];
        let token = db
            .preview_provider_batch("claude", &candidates)
            .unwrap()
            .token;
        assert!(db
            .append_provider_batch("claude", &candidates, &token)
            .is_err());
        let all = db.get_all_providers("claude").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all["collision"].name, "Existing");
    }

    #[test]
    fn appends_after_unordered_rows_without_changing_them() {
        let db = Database::memory().unwrap();
        let mut existing = Provider::with_id("old".into(), "Old".into(), json!({}), None);
        existing.created_at = Some(chrono::Utc::now().timestamp_millis() + 10000);
        db.save_provider("claude", &existing).unwrap();
        let candidates = vec![
            Provider::with_id("first".into(), "Z first".into(), json!({}), None),
            Provider::with_id("second".into(), "A second".into(), json!({}), None),
        ];
        let token = db
            .preview_provider_batch("claude", &candidates)
            .unwrap()
            .token;
        db.append_provider_batch("claude", &candidates, &token)
            .unwrap();
        let all = db.get_all_providers("claude").unwrap();
        assert!(all.values().all(|p| p.sort_index.is_none()));
        assert_eq!(all["old"].created_at, existing.created_at);
        assert!(all["old"].created_at < all["first"].created_at);
        assert!(all["first"].created_at < all["second"].created_at);
        assert_eq!(
            all.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["old", "first", "second"]
        );
    }
}
