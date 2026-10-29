//! DeepSeek Harness' standard profiles. CC Switch keeps one provider and MCP set in every
//! `web` and `desktop` profile, plus initialized `headless` and `acp` profiles;
//! `sdk` / `sdk-minimal` are for embedding and stay native-owned. All
//! profiles share `$DSH_HOME`'s credentials, sessions, skills and `AGENTS.md`.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{json, Value};
use serde_yaml::Value as Yaml;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".dsh"))
}

/// Profiles CC Switch writes, in this order. Web and Desktop are prepared before first launch.
const TARGET_PROFILES: [&str; 4] = ["web", "desktop", "headless", "acp"];
/// Providers and MCP are shown from the first of these that exists.
const DISPLAY_PROFILES: [&str; 2] = ["web", "desktop"];

pub(crate) fn profile_dir(name: &str) -> PathBuf {
    data_dir().join("profiles").join(name)
}

pub(crate) fn profile_path(name: &str) -> PathBuf {
    profile_dir(name).join("cordis.patch.yml")
}

/// Always prepare Web and Desktop: DSH's initProfile preserves an existing patch and supplies
/// the remaining files on first launch. Optional runtime profiles are only written if present.
pub(crate) fn target_paths() -> Vec<PathBuf> {
    TARGET_PROFILES
        .iter()
        .filter_map(|name| {
            let path = profile_path(name);
            (DISPLAY_PROFILES.contains(name) || path.exists()).then_some(path)
        })
        .collect()
}

/// The authoritative profile providers and MCP are read from first: `web`, else `desktop`, else
/// the first target. Writes go to every target, so [`read_paths`] also lists the rest.
pub(crate) fn config_path() -> PathBuf {
    DISPLAY_PROFILES
        .iter()
        .map(|name| profile_path(name))
        .find(|path| path.exists())
        .or_else(|| target_paths().into_iter().find(|path| path.exists()))
        .unwrap_or_else(|| profile_path("web"))
}

/// Every profile whose providers and MCP are shown, authoritative profile first.
///
/// Providers and MCP are written to all initialized profiles, so a server or provider that only
/// exists in one of them (added natively in DSH, or written before another profile was
/// initialized) must still be listed: hiding it while [`provider_key_exists`] reports it as taken
/// leaves the user with no way to see or resolve the collision.
pub(crate) fn read_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let authoritative = config_path();
    for path in std::iter::once(authoritative.clone()).chain(target_paths()) {
        if (path == authoritative || path.exists()) && !paths.contains(&path) {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        paths.push(authoritative);
    }
    paths
}

/// Profile directory name of a `profiles/<name>/cordis.patch.yml` path, for error messages.
fn profile_name(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("profile")
        .to_owned()
}

/// `$DSH_HOME/cordis.patch.yml`, applied after every profile.
pub(crate) fn home_patch_path() -> PathBuf {
    data_dir().join("cordis.patch.yml")
}

pub(crate) fn credentials_path() -> PathBuf {
    data_dir().join(".credentials.yaml")
}

fn read_text(path: &Path) -> Result<String, AppError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(AppError::io(path, error)),
    }
}

pub(crate) fn entries(text: &str) -> Result<Vec<Yaml>, AppError> {
    let value: Yaml = serde_yaml::from_str(text)
        .map_err(|_| AppError::Config("Invalid DeepSeek Harness profile YAML".into()))?;
    match value {
        Yaml::Null => Ok(vec![]),
        Yaml::Sequence(entries) => Ok(entries),
        _ => Err(AppError::Config(
            "DeepSeek Harness profile must be a YAML sequence".into(),
        )),
    }
}

/// Replace one plugin override, retaining every other entry byte for byte (including !!js).
/// The replaced entry itself is reserialized and must not use tags, anchors or aliases.
fn replace_entry(text: &str, id: &str, entry: Option<Yaml>) -> Result<String, AppError> {
    let parsed = entries(text)?;
    let matching: Vec<_> = parsed
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry["id"].as_str() == Some(id))
        .map(|(index, _)| index)
        .collect();
    if matching.len() > 1 {
        return Err(AppError::Config(format!(
            "Duplicate DeepSeek Harness plugin id '{id}'"
        )));
    }
    replace_patch(text, &parsed, matching.first().copied(), entry)
}

/// Edit a top-level patch, including an insert list without its own plugin id.
fn replace_patch(
    text: &str,
    parsed: &[Yaml],
    index: Option<usize>,
    entry: Option<Yaml>,
) -> Result<String, AppError> {
    let next = splice_patch(text, parsed, index, entry)?;
    // Removing or rewriting an entry can strand an alias whose anchor lived in it; refuse rather
    // than write a profile DSH can no longer load.
    entries(&next).map_err(|_| {
        AppError::Config(
            "This change would leave a YAML alias without its anchor; edit cordis.patch.yml directly".into(),
        )
    })?;
    Ok(next)
}

fn splice_patch(
    text: &str,
    parsed: &[Yaml],
    index: Option<usize>,
    entry: Option<Yaml>,
) -> Result<String, AppError> {
    // DSH requires a sequence even when the last patch is removed.
    let empty = if parsed.is_empty() || (parsed.len() == 1 && index.is_some()) {
        "[]\n"
    } else {
        ""
    };
    let reserialized = entry.is_some();
    let replacement = entry
        .map(|entry| serde_yaml::to_string(&vec![entry]))
        .transpose()
        .map_err(|error| AppError::Config(error.to_string()))?
        .unwrap_or_else(|| empty.to_owned());
    if parsed.is_empty() {
        // Native profiles start with comments followed by `[]`. Appending a
        // block entry to that flow sequence would produce invalid YAML.
        let mut next: String = text
            .split_inclusive('\n')
            .filter(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
            .collect();
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(&replacement);
        return Ok(next);
    }
    let mut starts = vec![];
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("- ") || line.trim_end() == "-" {
            starts.push(offset);
        }
        offset += line.len();
    }
    if parsed.len() != starts.len() {
        return Err(AppError::Config(
            "Use block-style YAML for the DeepSeek Harness profile".into(),
        ));
    }
    if let Some(index) = index {
        let end = starts.get(index + 1).copied().unwrap_or(text.len());
        // serde_yaml drops `!!js` tags and expands anchors, so rewriting an entry
        // that uses them would silently change what DSH evaluates.
        if reserialized
            && text[starts[index]..end]
                .lines()
                .any(crate::omp_config::has_yaml_reference_tokens)
        {
            return Err(AppError::Config(
                "This DeepSeek Harness profile entry uses YAML tags (such as !!js), anchors or aliases; edit it in cordis.patch.yml directly".into(),
            ));
        }
        Ok(format!(
            "{}{}{}",
            &text[..starts[index]],
            replacement,
            &text[end..]
        ))
    } else if replacement.is_empty() {
        Ok(text.into())
    } else {
        Ok(format!(
            "{}{}{}",
            text,
            if text.is_empty() || text.ends_with('\n') {
                ""
            } else {
                "\n"
            },
            replacement
        ))
    }
}

/// Same `<file>.lock` exclusive-create protocol and PID record as dsh-atomic-write.
struct FileLock(PathBuf);
impl FileLock {
    fn acquire(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
        }
        let lock = PathBuf::from(format!("{}.lock", path.display()));
        match Self::create(&lock) {
            Ok(guard) => Ok(guard),
            Err(AppError::Conflict(_)) if clear_stale_lock(&lock) => {
                // 上一次写入的进程已经不在了，锁是残留：清掉后重试一次。
                Self::create(&lock)
            }
            Err(error) => Err(error),
        }
    }

    fn create(lock: &Path) -> Result<Self, AppError> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    AppError::Conflict(
                        "DeepSeek Harness is saving its configuration; retry shortly".into(),
                    )
                } else {
                    AppError::io(lock, error)
                }
            })?;
        let guard = Self(lock.to_path_buf());
        writeln!(file, "{}", std::process::id()).map_err(|error| AppError::io(&guard.0, error))?;
        Ok(guard)
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// 崩溃残留的 `<file>.lock` 会把之后的写入永久卡住（DSH 原生协议只做 create_new）。
/// 只有在能证明锁的主人已经不在时才删除：Unix 用 `kill(pid, 0)`、Windows 用 `OpenProcess` 判活，都用修改
/// 时间兜底（一次写入的锁不可能留 10 分钟）。
const STALE_LOCK_AGE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

fn clear_stale_lock(lock: &Path) -> bool {
    let owner_gone = fs::read_to_string(lock)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .is_some_and(|pid| !process_is_alive(pid));
    let too_old = fs::metadata(lock)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > STALE_LOCK_AGE);
    (owner_gone || too_old) && fs::remove_file(lock).is_ok()
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    // SAFETY: kill(2) 只读内核状态；pid 0 代表进程组，已排除。
    pid > 0 && unsafe { libc::kill(pid as libc::pid_t, 0) } == 0
}

/// Windows：按 pid 打开进程；只有系统明确回答“没有这个进程”（`ERROR_INVALID_PARAMETER`）
/// 或进程已退出才算不在，拒绝访问等其它失败一律按活着处理，交给 mtime 兜底。
#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return true;
    }
    // SAFETY: 只查询；句柄在下面关掉。
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // SAFETY: 紧跟在失败的 OpenProcess 之后读取本线程的错误码。
        return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
    }
    let mut exit_code = 0u32;
    // SAFETY: 句柄有效，输出参数指向本地变量。
    let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
    // SAFETY: OpenProcess 返回的句柄只关这一次。
    unsafe { CloseHandle(handle) };
    !queried || exit_code == STILL_ACTIVE as u32
}

#[cfg(not(any(unix, windows)))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

fn restore(path: &Path, previous: Option<Vec<u8>>) -> Result<(), AppError> {
    match previous {
        Some(bytes) => atomic_write_private(path, &bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io(path, error)),
        },
    }
}

/// Edit several native files as one change: each file holds its own DSH `<file>.lock` through
/// the database commit. `plan` maps every file's current text to its next text, so an invalid
/// edit writes nothing; a failed write or commit restores every file already written.
fn edit_files_and_commit<T>(
    paths: &[PathBuf],
    plan: impl FnOnce(&[String]) -> Result<Vec<String>, AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    // 固定加锁顺序（按路径排序），避免两个并发写入以不同顺序拿锁时互相等死；
    // 调用方传进来的顺序只决定 plan 的下标语义，这里按排序后的顺序加锁。
    let mut lock_order: Vec<usize> = (0..paths.len()).collect();
    lock_order.sort_by(|left, right| paths[*left].cmp(&paths[*right]));
    let mut _guards = Vec::with_capacity(paths.len());
    for index in &lock_order {
        _guards.push(FileLock::acquire(&paths[*index])?);
    }
    let mut previous = Vec::with_capacity(paths.len());
    let mut current = Vec::with_capacity(paths.len());
    for path in paths {
        previous.push(if path.exists() {
            Some(fs::read(path).map_err(|error| AppError::io(path, error))?)
        } else {
            None
        });
        current.push(read_text(path)?);
    }
    let next = plan(&current)?;
    // plan 少给或多给一个文件都意味着某一处改动会被静默丢掉：宁可整体失败。
    if next.len() != paths.len() {
        return Err(AppError::Message(format!(
            "DeepSeek Harness write plan returned {} file(s) for {} file(s); nothing was written",
            next.len(),
            paths.len()
        )));
    }
    let mut written = vec![];
    let result = paths
        .iter()
        .zip(&current)
        .zip(&next)
        .enumerate()
        .filter(|(_, ((_, current), next))| current != next)
        .try_for_each(|(index, ((path, _), next))| {
            atomic_write_private(path, next.as_bytes())?;
            written.push(index);
            Ok(())
        })
        .and_then(|()| commit());
    match result {
        Ok(result) => Ok(result),
        Err(error) => {
            let failures: Vec<_> = written
                .into_iter()
                .rev()
                .filter_map(|index| {
                    restore(&paths[index], previous[index].take())
                        .err()
                        .map(|rollback| rollback.to_string())
                })
                .collect();
            if failures.is_empty() {
                Err(error)
            } else {
                Err(AppError::Message(format!(
                    "{error}; restoring DeepSeek Harness configuration failed: {}",
                    failures.join("; ")
                )))
            }
        }
    }
}

#[cfg(test)]
fn edit_path_and_commit<T>(
    path: &Path,
    edit: impl FnOnce(&str) -> Result<String, AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    edit_files_and_commit(
        &[path.to_path_buf()],
        |texts| Ok(vec![edit(&texts[0])?]),
        commit,
    )
}

fn provider_entry(text: &str) -> Result<Yaml, AppError> {
    let found: Vec<_> = entries(text)?
        .into_iter()
        .filter(|entry| entry["id"].as_str() == Some("llm-pi-ai"))
        .collect();
    if found.len() > 1 {
        return Err(AppError::Config("Duplicate llm-pi-ai overrides".into()));
    }
    let mut entry = found.into_iter().next().unwrap_or(
        serde_yaml::to_value(json!({"id":"llm-pi-ai","config":{"providers":{}}})).unwrap(),
    );
    if entry.get("config").is_none() {
        entry["config"] = serde_yaml::to_value(json!({})).unwrap();
    }
    if !entry["config"].is_mapping() {
        return Err(AppError::Config(
            "Invalid DeepSeek Harness llm-pi-ai configuration".into(),
        ));
    }
    if entry["config"].get("providers").is_none() {
        entry["config"]["providers"] = serde_yaml::to_value(json!({})).unwrap();
    }
    if !entry["config"]["providers"].is_mapping() {
        return Err(AppError::Config(
            "Invalid DeepSeek Harness providers mapping".into(),
        ));
    }
    Ok(entry)
}

fn credential_document(path: &Path) -> Result<Yaml, AppError> {
    credential_document_from(&read_text(path)?)
}

fn credential_document_from(text: &str) -> Result<Yaml, AppError> {
    if text.trim().is_empty() {
        return Ok(serde_yaml::to_value(json!({"version":1,"refs":{},"records":{}})).unwrap());
    }
    let document: Yaml = serde_yaml::from_str(text)
        .map_err(|_| AppError::Config("Invalid DeepSeek Harness credentials YAML".into()))?;
    if document["version"].as_u64() != Some(1)
        || (!document["refs"].is_null() && !document["refs"].is_mapping())
    {
        return Err(AppError::Config("Unsupported DeepSeek Harness credential document; migrate it in DeepSeek Harness first".into()));
    }
    Ok(document)
}

/// Key of the DeepSeek account login written by DSH's `deepseek-account-platform` provider.
pub(crate) const ACCOUNT_RECORD: &str = "deepseek-account-platform/default";
const ACCOUNT_DEVICE_RECORD: &str = "deepseek-account-platform/device";

/// The shared DeepSeek account grant (`{version: 1, token, issuer}`), if signed in.
pub(crate) fn account_grant() -> Result<Option<(String, String)>, AppError> {
    let document = credential_document(&credentials_path())?;
    let record = &document["records"][ACCOUNT_RECORD];
    if record.is_null() {
        return Ok(None);
    }
    let payload = &record["payload"];
    match (
        record["kind"].as_str(),
        payload["token"].as_str(),
        payload["issuer"].as_str(),
    ) {
        (Some("grant"), Some(token), Some(issuer)) if !token.is_empty() => {
            Ok(Some((token.to_owned(), issuer.to_owned())))
        }
        _ => Err(AppError::Config(
            "Unrecognized DeepSeek account login in .credentials.yaml".into(),
        )),
    }
}

/// DSH's per-install device id (`deepseek-account-platform/device`), created on first login
/// exactly like DSH does so CC Switch and DSH report the same device to Platform.
pub(crate) fn account_device_id() -> Result<String, AppError> {
    let path = credentials_path();
    let _lock = FileLock::acquire(&path)?;
    let mut document = credential_document(&path)?;
    if let Some(id) = document["records"][ACCOUNT_DEVICE_RECORD]["payload"]["id"].as_str() {
        if uuid::Uuid::parse_str(id).is_ok() {
            return Ok(id.to_owned());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    set_record(
        &mut document,
        ACCOUNT_DEVICE_RECORD,
        json!({"kind":"grant","payload":{"id":id}}),
    )?;
    write_credentials(&path, &document)?;
    Ok(id)
}

/// Store a DeepSeek account login for both DSH Web and Desktop (DSH reloads the file).
pub(crate) fn store_account_grant(token: &str, issuer: &str) -> Result<(), AppError> {
    let path = credentials_path();
    let _lock = FileLock::acquire(&path)?;
    let mut document = credential_document(&path)?;
    set_record(
        &mut document,
        ACCOUNT_RECORD,
        json!({"kind":"grant","payload":{"version":1,"token":token,"issuer":issuer}}),
    )?;
    write_credentials(&path, &document)
}

fn set_record(document: &mut Yaml, key: &str, record: Value) -> Result<(), AppError> {
    if !document["records"].is_mapping() {
        document["records"] = Yaml::Mapping(Default::default());
    }
    document["records"][key] =
        serde_yaml::to_value(record).map_err(|error| AppError::Config(error.to_string()))?;
    Ok(())
}

fn write_credentials(path: &Path, document: &Yaml) -> Result<(), AppError> {
    let yaml =
        serde_yaml::to_string(document).map_err(|error| AppError::Config(error.to_string()))?;
    atomic_write_private(path, yaml.as_bytes())
}

/// Sign out locally for Web and Desktop alike, under DSH's credentials lock. Returns the
/// removed grant so the caller can revoke it remotely, as DSH does.
pub(crate) fn remove_account_grant() -> Result<Option<(String, String)>, AppError> {
    let path = credentials_path();
    let _lock = FileLock::acquire(&path)?;
    let grant = account_grant()?;
    if grant.is_none() {
        return Ok(None);
    }
    let mut document = credential_document(&path)?;
    if let Some(records) = document["records"].as_mapping_mut() {
        records.remove(Yaml::from(ACCOUNT_RECORD));
    }
    write_credentials(&path, &document)?;
    Ok(grant)
}

/// Providers as shown in CC Switch: every initialized profile ([`read_paths`]), authoritative
/// profile first, first occurrence of a key winning.
pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let credentials = credentials_path();
    let authoritative = config_path();
    let mut merged: IndexMap<String, Value> = IndexMap::new();
    for path in read_paths() {
        let providers = match get_providers_from(&path, &credentials) {
            Ok(providers) => providers,
            // 权威 profile 读不出来时照旧报错；其余 profile 只跳过并记日志，别让一个写坏的
            // profile 把整张供应商列表变成错误页。
            Err(error) if path != authoritative => {
                log::warn!(
                    "跳过无法解析的 DeepSeek Harness profile {}: {error}",
                    path.display()
                );
                continue;
            }
            Err(error) => return Err(error),
        };
        for (id, provider) in providers {
            merged.entry(id).or_insert(provider);
        }
    }
    Ok(merged)
}

/// An `llm-pi-ai` config in the home patch is applied after every profile, so providers CC Switch
/// writes into the profiles would never take effect.
fn ensure_providers_not_shadowed(home: &Path) -> Result<(), AppError> {
    let text = read_text(home)?;
    if text.trim().is_empty() {
        return Ok(());
    }
    let shadowed = entries(&text)?
        .iter()
        .any(|entry| entry["id"].as_str() == Some("llm-pi-ai") && !entry["config"].is_null());
    if shadowed {
        let path = home.display();
        return Err(AppError::localized(
            "dsh.providers_shadowed_by_home",
            format!("供应商被 home 级补丁覆盖，请先在该文件里移除 llm-pi-ai 的 config：{path}"),
            format!("Providers are overridden by the home-level patch; remove the llm-pi-ai config from that file first: {path}"),
        ));
    }
    Ok(())
}

fn get_providers_from(
    path: &Path,
    credentials_path: &Path,
) -> Result<IndexMap<String, Value>, AppError> {
    let entry = provider_entry(&read_text(path)?)?;
    let credentials = credential_document(credentials_path)?;
    let mut result = IndexMap::new();
    for (key, native) in entry["config"]["providers"].as_mapping().unwrap() {
        let id = key
            .as_str()
            .ok_or_else(|| AppError::Config("Invalid DeepSeek Harness provider id".into()))?;
        let mut config: Value = serde_json::to_value(native).map_err(|_| {
            AppError::Config(format!(
                "Provider '{id}' contains dynamic YAML; edit it in DeepSeek Harness"
            ))
        })?;
        if !config.is_object() {
            return Err(AppError::Config(format!(
                "Invalid DeepSeek Harness provider '{id}'"
            )));
        }
        let reference = config
            .get("apiKeyEnv")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let key = credentials["refs"][reference]
            .as_str()
            .map(str::to_string)
            .or_else(|| std::env::var(reference).ok())
            .unwrap_or_default();
        let mut models = serde_json::Map::new();
        for model in config
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = model.get("id").and_then(Value::as_str) {
                let mut model = model.clone();
                let mut limits = serde_json::Map::new();
                for (native, editor) in [("contextWindow", "context"), ("maxTokens", "output")] {
                    if let Some(value) = model.get(native) {
                        limits.insert(editor.into(), value.clone());
                    }
                }
                model["limit"] = Value::Object(limits);
                if let Some(efforts) = model.get("reasoningEfforts") {
                    model["reasoning"] = json!(efforts.is_object());
                }
                if let Some(input) = model.get("input").cloned() {
                    model["modalities"] = json!({"input":input,"output":["text"]});
                }
                models.insert(id.into(), model);
            }
        }
        config["name"] = config.get("displayName").cloned().unwrap_or(json!(id));
        config["options"] = json!({"baseURL": config.get("baseURL"),"apiKey":key,"headers":config.get("headers").cloned().unwrap_or(json!({}))});
        config["models"] = Value::Object(models);
        result.insert(id.into(), config);
    }
    Ok(result)
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.is_empty()
        || id == "deepseek-official"
        || id.starts_with('-')
        || id.ends_with('-')
        || id.contains("--")
        || id
            .chars()
            .any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-')
    {
        return Err(AppError::InvalidInput("DeepSeek Harness provider ids must be lowercase hyphenated identifiers; deepseek-official is reserved".into()));
    }
    if !matches!(
        config["api"].as_str(),
        Some("openai-completions" | "openai-responses" | "anthropic-messages")
    ) {
        return Err(AppError::InvalidInput(
            "Select a supported DeepSeek Harness API protocol".into(),
        ));
    }
    if !config
        .pointer("/options/baseURL")
        .and_then(Value::as_str)
        .and_then(|url| url::Url::parse(url).ok())
        .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    {
        return Err(AppError::InvalidInput(
            "Enter a valid DeepSeek Harness endpoint URL".into(),
        ));
    }
    if config
        .pointer("/options/apiKey")
        .and_then(Value::as_str)
        .is_none_or(|key| key.trim().is_empty())
        && config
            .get("apiKeyEnv")
            .and_then(Value::as_str)
            .is_none_or(|reference| reference.is_empty())
    {
        return Err(AppError::InvalidInput(
            "Enter an API key or retain a native credential reference".into(),
        ));
    }
    let Some(models) = config["models"]
        .as_object()
        .filter(|models| !models.is_empty())
    else {
        return Err(AppError::InvalidInput(
            "Add at least one DeepSeek Harness model".into(),
        ));
    };
    for (id, model) in models {
        if id.trim().is_empty() || !model.is_object() {
            return Err(AppError::InvalidInput(
                "Invalid DeepSeek Harness model".into(),
            ));
        }
        for field in ["context", "output"] {
            if model
                .pointer(&format!("/limit/{field}"))
                .is_some_and(|value| !value.is_null() && value.as_u64().is_none_or(|n| n == 0))
            {
                return Err(AppError::InvalidInput(
                    "Model capacities must be positive integers".into(),
                ));
            }
        }
    }
    Ok(())
}

fn update_provider(
    text: &str,
    id: &str,
    config: Option<Value>,
    create: bool,
) -> Result<String, AppError> {
    let mut entry = provider_entry(text)?;
    let providers = entry["config"]["providers"].as_mapping_mut().unwrap();
    if config.is_none() && !providers.contains_key(Yaml::from(id)) {
        return Ok(text.to_owned());
    }
    if create && providers.contains_key(Yaml::from(id)) {
        return Err(AppError::Conflict(format!(
            "DeepSeek Harness provider '{id}' already exists"
        )));
    }
    if let Some(config) = config {
        providers.insert(
            id.into(),
            serde_yaml::to_value(config).map_err(|error| AppError::Config(error.to_string()))?,
        );
    } else {
        providers.remove(Yaml::from(id));
    }
    replace_entry(text, "llm-pi-ai", Some(entry))
}

/// Add, edit or remove one provider in every target profile together with its credential, as
/// one change ([`edit_files_and_commit`]).
pub(crate) fn save_and_commit<T>(
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    if config.is_some() {
        ensure_providers_not_shadowed(&home_patch_path())?;
    }
    save_paths_and_commit(
        &target_paths(),
        &credentials_path(),
        id,
        config,
        create,
        commit,
    )
}

fn save_paths_and_commit<T>(
    paths: &[PathBuf],
    credentials_path: &Path,
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let Some(config) = config else {
        let existing: Vec<_> = paths.iter().filter(|path| path.exists()).cloned().collect();
        if existing.is_empty() {
            return commit();
        }
        // 删除也只和“真正存在的这些文件”比，索引必须按这批文件重算
        let authoritative = authoritative_index(&existing);
        return edit_files_and_commit(
            &existing,
            |texts| {
                ensure_provider_copies_agree(&existing, authoritative, texts, id)?;
                texts
                    .iter()
                    .map(|text| update_provider(text, id, None, false))
                    .collect()
            },
            commit,
        );
    };
    let mut files = vec![credentials_path.to_path_buf()];
    files.extend(paths.iter().cloned());
    let authoritative = authoritative_index(paths);
    edit_files_and_commit(
        &files,
        |texts| {
            let profile_texts = &texts[1..];
            if create {
                ensure_provider_key_is_free(paths, profile_texts, id)?;
            } else {
                ensure_provider_copies_agree(paths, authoritative, profile_texts, id)?;
            }
            let (credentials, native) = native_provider(id, &config, &texts[0])?;
            let mut next = vec![credentials];
            for text in profile_texts {
                next.push(update_provider(text, id, Some(native.clone()), create)?);
            }
            Ok(next)
        },
        commit,
    )
}

/// Seed a newly encountered profile once. Keep the receipt beside the native profiles, rather
/// than in the cloud-synced database: deleting every managed node in DSH must not cause a later
/// startup to restore it. Profiles, credentials and the receipt are committed together.
pub(crate) fn initialize_profiles(
    providers: &IndexMap<String, Value>,
    servers: &IndexMap<String, Value>,
) -> Result<(), AppError> {
    initialize_profiles_and_commit(providers, servers, || Ok(()))
}

fn initialize_profiles_and_commit<T>(
    providers: &IndexMap<String, Value>,
    servers: &IndexMap<String, Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    if providers.is_empty() && servers.is_empty() {
        return commit();
    }
    let profiles = target_paths();
    let receipt = data_dir().join(".cc-switch-profile-sync.json");
    let parse_receipt = |text: &str| -> Result<Vec<String>, AppError> {
        if text.trim().is_empty() {
            Ok(vec![])
        } else {
            serde_json::from_str(text).map_err(|_| {
                AppError::Config("Invalid DeepSeek Harness profile sync receipt".into())
            })
        }
    };
    let initialized = parse_receipt(&read_text(&receipt)?)?;
    if profiles
        .iter()
        .all(|path| initialized.contains(&profile_name(path)))
    {
        return commit();
    }
    let mut files = vec![credentials_path()];
    files.extend(profiles.iter().cloned());
    // Write the receipt last, after every native file has been written successfully.
    files.push(receipt);
    edit_files_and_commit(
        &files,
        |texts| {
            let mut initialized = parse_receipt(&texts[profiles.len() + 1])?;
            let mut next = texts.to_vec();
            for (index, path) in profiles.iter().enumerate() {
                let name = profile_name(path);
                if initialized.contains(&name) {
                    continue;
                }
                let text = &texts[index + 1];
                let has_providers = providers.keys().try_fold(false, |found, id| {
                    Ok::<_, AppError>(found || existing_provider_node(text, id)?.is_some())
                })?;
                let has_servers = servers.keys().try_fold(false, |found, id| {
                    Ok::<_, AppError>(found || existing_mcp_entry(text, id)?.is_some())
                })?;
                if !has_providers && !providers.is_empty() {
                    // Copying managed nodes into a new profile is not a user edit: a home-level
                    // llm-pi-ai config hides them until it is removed, but refusing here would
                    // fail every startup without ever writing the receipt.
                    for (id, config) in providers {
                        // Prefer the existing native node, including its credential reference.
                        // A stale database row must not overwrite a key edited in DSH.
                        let mut native = None;
                        for source in &texts[1..=profiles.len()] {
                            if let Some(node) = existing_provider_node(source, id)? {
                                let node = serde_json::to_value(node).map_err(|_| {
                                    AppError::Config(format!(
                                        "Provider '{id}' contains dynamic YAML; edit it in DeepSeek Harness"
                                    ))
                                })?;
                                // Validate the original text too: serde_yaml can drop !!js tags.
                                update_provider(source, id, Some(node.clone()), false)?;
                                native = Some(node);
                                break;
                            }
                        }
                        let native = match native {
                            Some(native) => native,
                            None => {
                                let (credentials, native) = native_provider(id, config, &next[0])?;
                                next[0] = credentials;
                                native
                            }
                        };
                        next[index + 1] =
                            update_provider(&next[index + 1], id, Some(native), true)?;
                    }
                }
                if !has_servers {
                    for (id, spec) in servers {
                        validate_mcp_spec(id, spec)?;
                        let mut source = None;
                        for text in &texts[1..=profiles.len()] {
                            if let Some(entry) = existing_mcp_entry(text, id)? {
                                // Apply the same tag/anchor checks as an explicit native edit.
                                mcp_edit(text, id, Some(spec))?;
                                source = Some(entry);
                                break;
                            }
                        }
                        next[index + 1] = match source {
                            Some(source) => insert_mcp_entry(&next[index + 1], &source)?,
                            None => mcp_edit(&next[index + 1], id, Some(spec))?,
                        };
                    }
                }
                initialized.push(name);
            }
            next[profiles.len() + 1] = serde_json::to_string(&initialized)
                .map_err(|error| AppError::Config(error.to_string()))?;
            Ok(next)
        },
        commit,
    )
}

/// Position of the authoritative profile ([`config_path`]) among `paths`.
///
/// Every target profile is written, but only the authoritative one defines what CC Switch shows,
/// so its copy is the reference the others are compared against. Tests that pass a single
/// arbitrary path fall back to it.
fn authoritative_index(paths: &[PathBuf]) -> usize {
    let authoritative = config_path();
    paths
        .iter()
        .position(|path| *path == authoritative)
        .unwrap_or(0)
}

/// The `llm-pi-ai` provider node stored under `id` in one profile, if any.
fn existing_provider_node(text: &str, id: &str) -> Result<Option<Yaml>, AppError> {
    Ok(provider_entry(text)?["config"]["providers"]
        .get(id)
        .cloned())
}

/// Refuse to write when another profile holds a *different* version of the same provider.
///
/// CC Switch only knows the authoritative copy, so rewriting the others would silently discard
/// whatever the user changed natively in DSH (a different endpoint, models or credential
/// reference). Refuse instead, and name the profile that has to be reconciled.
fn ensure_provider_copies_agree(
    paths: &[PathBuf],
    authoritative: usize,
    texts: &[String],
    id: &str,
) -> Result<(), AppError> {
    let Some(source) = texts
        .get(authoritative)
        .map(|text| existing_provider_node(text, id))
        .transpose()?
        .flatten()
    else {
        return Ok(());
    };
    for (index, text) in texts.iter().enumerate() {
        if index == authoritative {
            continue;
        }
        if existing_provider_node(text, id)?.is_some_and(|copy| copy != source) {
            return Err(diverged_provider_error(
                id,
                paths.get(index),
                paths.get(authoritative),
            ));
        }
    }
    Ok(())
}

fn diverged_provider_error(
    id: &str,
    other: Option<&PathBuf>,
    source: Option<&PathBuf>,
) -> AppError {
    let other = other.map(|path| profile_name(path)).unwrap_or_default();
    let source = source.map(|path| profile_name(path)).unwrap_or_default();
    AppError::localized(
        "dsh.provider_diverged",
        format!(
            "供应商 '{id}' 在 profile '{other}' 里的内容与 '{source}' 不同；CC Switch 只认得 '{source}' 的那份，改写会覆盖你在 DeepSeek Harness 里对 '{other}' 做的修改。请先在 DeepSeek Harness 里把两者改成一致，或手动编辑 profiles/{other}/cordis.patch.yml。"
        ),
        format!(
            "Provider '{id}' differs between the '{other}' and '{source}' profiles; CC Switch only knows the '{source}' copy, so writing would discard your changes in '{other}'. Make them identical in DeepSeek Harness, or edit profiles/{other}/cordis.patch.yml directly."
        ),
    )
}

/// Refuse to add a provider whose key is already used by any profile, naming which one.
fn ensure_provider_key_is_free(
    paths: &[PathBuf],
    texts: &[String],
    id: &str,
) -> Result<(), AppError> {
    for (index, text) in texts.iter().enumerate() {
        if existing_provider_node(text, id)?.is_some() {
            let name = paths
                .get(index)
                .map(|path| profile_name(path))
                .unwrap_or_default();
            return Err(AppError::localized(
                "dsh.provider_key_taken",
                format!("供应商 key '{id}' 已经在 profile '{name}' 里被占用；请改用一个新 key，或先在那个 profile 里移除它。"),
                format!("Provider key '{id}' is already used by the '{name}' profile; choose another key, or remove it from that profile first."),
            ));
        }
    }
    Ok(())
}

/// The MCP client plugin entry on a profile for server `id`, if any.
fn existing_mcp_entry(text: &str, id: &str) -> Result<Option<Yaml>, AppError> {
    let all_entries = entries(text)?;
    let found = plugin_entries(&all_entries)
        .filter(|(_, _, entry)| {
            entry["name"].as_str() == Some("@deepseek-ai/dsh-mcp-client")
                && entry["config"]["serverName"].as_str() == Some(id)
        })
        .map(|(_, _, entry)| entry.clone())
        .next();
    Ok(found)
}

/// Compare existing copies, preferring the displayed profile when it has one. A missing copy
/// is expected when a profile was initialized later; only differing contents are a conflict.
fn ensure_mcp_copies_agree(
    paths: &[PathBuf],
    authoritative: usize,
    texts: &[String],
    id: &str,
) -> Result<Option<Yaml>, AppError> {
    let mut source = texts
        .get(authoritative)
        .map(|text| existing_mcp_entry(text, id))
        .transpose()?
        .flatten()
        .map(|entry| (authoritative, entry));
    for (index, text) in texts.iter().enumerate() {
        let Some(copy) = existing_mcp_entry(text, id)? else {
            continue;
        };
        if let Some((source_index, source)) = &source {
            if copy != *source {
                return Err(diverged_mcp_error(
                    id,
                    paths.get(index),
                    paths.get(*source_index),
                ));
            }
        } else {
            source = Some((index, copy));
        }
    }
    Ok(source.map(|(_, entry)| entry))
}

fn diverged_mcp_error(id: &str, other: Option<&PathBuf>, source: Option<&PathBuf>) -> AppError {
    let other = other.map(|path| profile_name(path)).unwrap_or_default();
    let source = source.map(|path| profile_name(path)).unwrap_or_default();
    AppError::localized(
        "dsh.mcp_diverged",
        format!(
            "MCP 服务器 '{id}' 在 profile '{other}' 里的配置与 '{source}' 不同；改写会覆盖你在 DeepSeek Harness 里对 '{other}' 做的修改。请先在 DeepSeek Harness 里把两者改成一致，或手动编辑 profiles/{other}/cordis.patch.yml。"
        ),
        format!(
            "MCP server '{id}' differs between the '{other}' and '{source}' profiles; writing would discard your changes in '{other}'. Make them identical in DeepSeek Harness, or edit profiles/{other}/cordis.patch.yml directly."
        ),
    )
}

/// The native `llm-pi-ai` provider node for an editor config, and the credentials text with a new
/// key stored under CC Switch's reference (unchanged when the key is unchanged).
fn native_provider(
    id: &str,
    config: &Value,
    credentials_text: &str,
) -> Result<(String, Value), AppError> {
    let mut credentials_next = credentials_text.to_owned();
    let native = {
        validate_provider(id, config)?;
        let mut native = config.clone();
        let object = native.as_object_mut().unwrap();
        object.remove("name");
        object.remove("options");
        object.remove("kind");
        object.remove("enabled");
        object.remove("headers");
        object.insert(
            "displayName".into(),
            config.get("name").cloned().unwrap_or(json!(id)),
        );
        object.insert("baseURL".into(), config["options"]["baseURL"].clone());
        if let Some(headers) = config.pointer("/options/headers") {
            object.insert("headers".into(), headers.clone());
        }
        let mut models = vec![];
        for (model_id, model) in config["models"].as_object().unwrap() {
            let mut model = model.clone();
            model["id"] = json!(model_id);
            for (from, to) in [("context", "contextWindow"), ("output", "maxTokens")] {
                if let Some(value) = model
                    .pointer(&format!("/limit/{from}"))
                    .filter(|value| !value.is_null())
                    .cloned()
                {
                    model[to] = value;
                }
            }
            model.as_object_mut().unwrap().remove("limit");
            if let Some(input) = model.pointer("/modalities/input").cloned() {
                model["input"] = input;
            }
            let object = model.as_object_mut().unwrap();
            object.remove("modalities");
            if object.get("reasoning") == Some(&Value::Bool(false)) {
                object.insert("reasoningEfforts".into(), Value::Bool(false));
            } else if object.get("reasoning") == Some(&Value::Bool(true))
                && !object.contains_key("reasoningEfforts")
            {
                object.insert(
                    "reasoningEfforts".into(),
                    json!({"off":null,"low":"low","medium":"medium","high":"high"}),
                );
            }
            object.remove("reasoning");
            models.push(model);
        }
        object.insert("models".into(), json!(models));
        if let Some(key) = config
            .pointer("/options/apiKey")
            .and_then(Value::as_str)
            .filter(|key| !key.trim().is_empty())
        {
            let reference = format!(
                "CC_SWITCH_DSH_{}",
                id.replace('-', "_").to_ascii_uppercase()
            );
            let mut credentials = credential_document_from(credentials_text)?;
            let unchanged_key =
                config
                    .get("apiKeyEnv")
                    .and_then(Value::as_str)
                    .is_some_and(|reference| {
                        credentials["refs"][reference].as_str() == Some(key)
                            || std::env::var(reference).ok().as_deref() == Some(key)
                    });
            if !unchanged_key {
                if credentials["refs"].is_null() {
                    credentials["refs"] = Yaml::Mapping(Default::default());
                }
                credentials["refs"][&reference] = Yaml::from(key);
                credentials_next = serde_yaml::to_string(&credentials)
                    .map_err(|error| AppError::Config(error.to_string()))?;
                object.insert("apiKeyEnv".into(), json!(reference));
            }
        }
        native
    };
    Ok((credentials_next, native))
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    save_and_commit(id, Some(config), false, || Ok(()))
}
pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    save_and_commit(id, None, false, || Ok(()))
}
/// A key is taken when any target profile already has it (CC Switch never overwrites a node).
pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    for path in target_paths() {
        let entry = provider_entry(&read_text(&path)?)?;
        if entry["config"]["providers"]
            .as_mapping()
            .is_some_and(|providers| providers.contains_key(Yaml::from(id)))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// MCP servers as shown in CC Switch, read from the display profile ([`config_path`]).
/// MCP servers as shown in CC Switch: every initialized profile, authoritative profile first,
/// one entry per server name.
pub(crate) fn mcp_entries() -> Result<Vec<Yaml>, AppError> {
    let authoritative = config_path();
    let mut merged: Vec<Yaml> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for path in read_paths() {
        let entries = match mcp_entries_from(&read_text(&path)?) {
            Ok(entries) => entries,
            // 权威 profile 读不出来时照旧报错；其余 profile 只跳过并记日志。
            Err(error) if path != authoritative => {
                log::warn!(
                    "跳过无法解析的 DeepSeek Harness profile {}: {error}",
                    path.display()
                );
                continue;
            }
            Err(error) => return Err(error),
        };
        for entry in entries {
            let name = entry["config"]["serverName"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{entry:?}"));
            if names.contains(&name) {
                continue;
            }
            names.push(name);
            merged.push(entry);
        }
    }
    Ok(merged)
}

fn mcp_entries_from(text: &str) -> Result<Vec<Yaml>, AppError> {
    Ok(plugin_entries(&entries(text)?)
        .filter(|(_, _, entry)| entry["name"].as_str() == Some("@deepseek-ai/dsh-mcp-client"))
        .map(|(_, _, entry)| entry.clone())
        .collect())
}

fn plugin_entries(patches: &[Yaml]) -> impl Iterator<Item = (usize, Option<usize>, &Yaml)> {
    patches.iter().enumerate().flat_map(|(index, patch)| {
        std::iter::once((index, None, patch)).chain(
            patch["insert"]
                .as_sequence()
                .into_iter()
                .flatten()
                .enumerate()
                .map(move |(insert_index, entry)| (index, Some(insert_index), entry)),
        )
    })
}

/// Write one MCP server into every target profile as one change ([`edit_files_and_commit`]).
pub(crate) fn sync_mcp_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    sync_mcp_files_and_commit(&target_paths(), id, spec, commit)
}

#[cfg(test)]
fn sync_mcp_file_and_commit<T>(
    path: &Path,
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    sync_mcp_files_and_commit(&[path.to_path_buf()], id, spec, commit)
}

fn sync_mcp_files_and_commit<T>(
    paths: &[PathBuf],
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let paths: Vec<_> = if spec.is_none() {
        paths.iter().filter(|path| path.exists()).cloned().collect()
    } else {
        paths.to_vec()
    };
    if paths.is_empty() {
        return commit();
    }
    if let Some(spec) = spec {
        validate_mcp_spec(id, spec)?;
    }
    edit_files_and_commit(
        &paths,
        |texts| {
            let authoritative = authoritative_index(&paths);
            let source = ensure_mcp_copies_agree(&paths, authoritative, texts, id)?;
            texts
                .iter()
                .map(|text| mcp_edit_with_source(text, id, spec, source.as_ref()))
                .collect()
        },
        commit,
    )
}

fn validate_mcp_spec(id: &str, spec: &Value) -> Result<(), AppError> {
    crate::mcp::validation::validate_server_spec(spec)?;
    if spec["type"] == "sse" {
        return Err(AppError::InvalidInput("DeepSeek Harness supports stdio and Streamable HTTP MCP; use an HTTP endpoint instead of SSE".into()));
    }
    if id.is_empty()
        || id.len() > 32
        || id
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
    {
        return Err(AppError::InvalidInput(
            "DeepSeek Harness MCP names must contain 1–32 letters, digits, '-' or '_'".into(),
        ));
    }
    Ok(())
}

/// A missing copy inherits native plugin IDs and extension fields from an existing copy, so
/// creating it does not manufacture a conflict on the next edit or deletion.
fn mcp_edit_with_source(
    text: &str,
    id: &str,
    spec: Option<&Value>,
    source: Option<&Yaml>,
) -> Result<String, AppError> {
    if let Some(source) = source.filter(|_| spec.is_some()) {
        if existing_mcp_entry(text, id)?.is_none() {
            let seeded = insert_mcp_entry(text, source)?;
            return mcp_edit(&seeded, id, spec);
        }
    }
    mcp_edit(text, id, spec)
}

fn insert_mcp_entry(text: &str, entry: &Yaml) -> Result<String, AppError> {
    // serde_yaml does not round-trip native !!js tags. Refuse dynamic nodes before copying.
    serde_json::to_value(entry).map_err(|_| {
        AppError::Config(
            "This DeepSeek Harness MCP entry contains dynamic YAML; edit it in DeepSeek Harness"
                .into(),
        )
    })?;
    let parsed = entries(text)?;
    if let Some(id) = entry["id"].as_str() {
        if plugin_entries(&parsed).any(|(_, _, entry)| entry["id"].as_str() == Some(id)) {
            return Err(AppError::Conflict(format!(
                "DeepSeek Harness plugin id '{id}' is already in use"
            )));
        }
    }
    let patch = Yaml::Mapping(
        [(Yaml::from("insert"), Yaml::Sequence(vec![entry.clone()]))]
            .into_iter()
            .collect(),
    );
    replace_patch(text, &parsed, None, Some(patch))
}

/// One profile's text with MCP server `id` added, replaced or (when `spec` is `None`) removed.
fn mcp_edit(text: &str, id: &str, spec: Option<&Value>) -> Result<String, AppError> {
    {
        let all_entries = entries(text)?;
        let matches: Vec<_> = plugin_entries(&all_entries)
            .filter(|(_, _, entry)| {
                entry["name"].as_str() == Some("@deepseek-ai/dsh-mcp-client")
                    && entry["config"]["serverName"].as_str() == Some(id)
            })
            .collect();
        if matches.len() > 1 {
            return Err(AppError::Config(format!(
                "Duplicate DeepSeek Harness MCP server '{id}'"
            )));
        }
        let location = matches.into_iter().next();
        let existing = location.map(|(_, _, entry)| entry.clone());
        if existing.is_none() && spec.is_none() {
            return Ok(text.to_owned());
        }
        let plugin_id = existing
            .as_ref()
            .and_then(|entry| entry["id"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("cc-switch-mcp-{id}"));
        if existing.is_some()
            && existing
                .as_ref()
                .and_then(|entry| entry["id"].as_str())
                .is_none()
        {
            return Err(AppError::Config(
                "Assign an id to this native MCP entry before editing it in CC Switch".into(),
            ));
        }
        let id_matches = plugin_entries(&all_entries)
            .filter(|(_, _, entry)| entry["id"].as_str() == Some(&plugin_id))
            .count();
        if id_matches > usize::from(existing.is_some()) {
            return Err(AppError::Conflict(format!(
                "DeepSeek Harness plugin id '{plugin_id}' is already in use"
            )));
        }
        let next = if let Some(spec) = spec {
            let mut entry = existing.unwrap_or(
                serde_yaml::to_value(
                    json!({"id":plugin_id,"name":"@deepseek-ai/dsh-mcp-client","config":{}}),
                )
                .unwrap(),
            );
            entry
                .as_mapping_mut()
                .unwrap()
                .remove(Yaml::from("disabled"));
            let config = entry["config"].as_mapping_mut().ok_or_else(|| {
                AppError::Config("Invalid DeepSeek Harness MCP configuration".into())
            })?;
            for field in [
                "command",
                "args",
                "env",
                "cwd",
                "url",
                "headers",
                "transport",
            ] {
                config.remove(Yaml::from(field));
            }
            for (key, value) in spec.as_object().unwrap() {
                if key != "type" {
                    config.insert(key.as_str().into(), serde_yaml::to_value(value).unwrap());
                }
            }
            config.insert("serverName".into(), id.into());
            config.insert(
                "transport".into(),
                if spec.get("url").is_some() {
                    "streamable-http"
                } else {
                    "stdio"
                }
                .into(),
            );
            Some(entry)
        } else {
            None
        };
        match location {
            Some((index, Some(insert_index), _)) => {
                let mut patch = all_entries[index].clone();
                let inserted = patch["insert"].as_sequence_mut().unwrap();
                if let Some(next) = next {
                    inserted[insert_index] = next;
                } else {
                    inserted.remove(insert_index);
                }
                let next = if inserted.is_empty() {
                    None
                } else {
                    Some(patch)
                };
                replace_patch(text, &all_entries, Some(index), next)
            }
            Some((index, None, _)) => replace_patch(text, &all_entries, Some(index), next),
            None => replace_patch(
                text,
                &all_entries,
                None,
                next.map(|entry| {
                    let mut patch = serde_yaml::Mapping::new();
                    patch.insert("insert".into(), Yaml::Sequence(vec![entry]));
                    Yaml::Mapping(patch)
                }),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @deepseek-ai/dsh-app-boot@0.2.1-rc.2 PROFILE_PATCH_TEMPLATE.
    const FACTORY_PROFILE: &str = "# Your patch layer for this dsh profile, applied after every bundle layer:\n# a top-level YAML array of loader patch entries (id-targeted config\n# overrides, disables, and insert lists; `!!js` expressions allowed).\n[]\n";

    fn editor_config() -> Value {
        json!({"name":"Gateway","api":"openai-responses","options":{"baseURL":"https://gateway.example/v1","apiKey":"test-value","headers":{"X-Client":"cc-switch"}},"models":{"test-model":{"name":"Model","limit":{"context":100000,"output":1000},"modalities":{"input":["text","image"],"output":["text"]},"reasoningEfforts":{"off":null,"high":"high"},"compat":{"supportsStore":false}}}})
    }

    #[test]
    fn provider_initializes_factory_profile_as_valid_block_yaml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let credentials = temp.path().join(".credentials.yaml");
        fs::write(&path, FACTORY_PROFILE).unwrap();
        save_paths_and_commit(
            std::slice::from_ref(&path),
            &credentials,
            "gateway",
            Some(editor_config()),
            true,
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(FACTORY_PROFILE.trim_end_matches("[]\n")));
        assert_eq!(entries(&text).unwrap().len(), 1);
        assert_eq!(get_providers_from(&path, &credentials).unwrap().len(), 1);
        assert!(!text.lines().any(|line| line.trim() == "[]"));
    }

    #[test]
    fn mcp_initializes_factory_profile_as_valid_block_yaml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        fs::write(&path, FACTORY_PROFILE).unwrap();
        sync_mcp_file_and_commit(
            &path,
            "test",
            Some(&json!({"type":"http","url":"https://example.com/mcp"})),
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(FACTORY_PROFILE.trim_end_matches("[]\n")));
        let entries = entries(&text).unwrap();
        assert_eq!(entries.len(), 1);
        // DSH only creates plugins from insert lists; id-only patches cannot
        // create a plugin absent from the bundle's existing entry list.
        let plugins = entries[0]["insert"].as_sequence().unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0]["id"], "cc-switch-mcp-test");
        assert_eq!(plugins[0]["name"], "@deepseek-ai/dsh-mcp-client");
        assert_eq!(plugins[0]["config"]["serverName"], "test");
        assert_eq!(plugins[0]["config"]["transport"], "streamable-http");
        assert_eq!(mcp_entries_from(&text).unwrap(), *plugins);
        assert!(!text.lines().any(|line| line.trim() == "[]"));
    }

    #[test]
    fn removing_last_mcp_restores_an_empty_yaml_sequence() {
        for original in [FACTORY_PROFILE, ""] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("cordis.patch.yml");
            fs::write(&path, original).unwrap();
            sync_mcp_file_and_commit(&path, "test", Some(&json!({"command":"node"})), || Ok(()))
                .unwrap();
            sync_mcp_file_and_commit(&path, "test", None, || Ok(())).unwrap();
            let text = read_text(&path).unwrap();
            let document: Yaml = serde_yaml::from_str(&text).unwrap();
            assert_eq!(document, Yaml::Sequence(vec![]));
            assert_eq!(
                text,
                if original.is_empty() {
                    "[]\n"
                } else {
                    original
                }
            );
        }
        assert_eq!(replace_entry("- id: last\n", "last", None).unwrap(), "[]\n");
    }

    #[test]
    fn provider_credentials_allow_missing_or_null_refs_and_preserve_login_records() {
        for refs in ["", "refs: null\n"] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("cordis.patch.yml");
            let credentials = temp.path().join(".credentials.yaml");
            let original =
                format!("version: 1\n{refs}records: {{owner: {{account: {{value: keep}}}}}}\n");
            fs::write(&credentials, &original).unwrap();
            let original_document: Yaml = serde_yaml::from_str(&original).unwrap();
            assert!(get_providers_from(&path, &credentials).unwrap().is_empty());
            // Existing providers also remain readable without a stored ref.
            fs::write(&path, "- id: llm-pi-ai\n  config:\n    providers:\n      native: {apiKeyEnv: CC_SWITCH_DSH_TEST_MISSING_REF, models: []}\n").unwrap();
            assert_eq!(get_providers_from(&path, &credentials).unwrap().len(), 1);
            assert_eq!(read_text(&credentials).unwrap(), original);
            save_paths_and_commit(
                std::slice::from_ref(&path),
                &credentials,
                "gateway",
                Some(editor_config()),
                true,
                || Ok(()),
            )
            .unwrap();
            let saved = credential_document(&credentials).unwrap();
            assert_eq!(saved["records"], original_document["records"]);
            assert_eq!(saved["refs"]["CC_SWITCH_DSH_GATEWAY"], "test-value");
            assert_eq!(
                get_providers_from(&path, &credentials).unwrap()["gateway"]["options"]["apiKey"],
                "test-value"
            );
        }
    }

    #[test]
    fn provider_credentials_still_reject_invalid_refs_sections() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".credentials.yaml");
        for refs in ["[]", "invalid", "123"] {
            fs::write(&path, format!("version: 1\nrefs: {refs}\n")).unwrap();
            assert!(credential_document(&path).is_err());
        }
    }

    #[test]
    fn provider_round_trip_uses_private_credentials_and_preserves_extensions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let credentials = temp.path().join(".credentials.yaml");
        let original = "# native plugin\n- id: other\n  name: my-plugin\n  config: {value: keep}\n";
        fs::write(&path, original).unwrap();
        fs::write(
            &credentials,
            "version: 1\nrefs: {OTHER_KEY: other}\nrecords: {owner: {account: {value: keep}}}\n",
        )
        .unwrap();
        save_paths_and_commit(
            std::slice::from_ref(&path),
            &credentials,
            "gateway",
            Some(editor_config()),
            true,
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(original));
        assert!(!text.contains("test-value"));
        let entry = provider_entry(&text).unwrap();
        let model = &entry["config"]["providers"]["gateway"]["models"][0];
        assert_eq!(model["contextWindow"], 100000);
        assert_eq!(model["input"][1], "image");
        assert_eq!(model["compat"]["supportsStore"], false);
        let imported = get_providers_from(&path, &credentials).unwrap();
        assert_eq!(imported["gateway"]["options"]["apiKey"], "test-value");
        let previous_credentials = fs::read(&credentials).unwrap();
        let mut edited = imported["gateway"].clone();
        edited["name"] = json!("Renamed");
        save_paths_and_commit(
            std::slice::from_ref(&path),
            &credentials,
            "gateway",
            Some(edited),
            false,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(fs::read(&credentials).unwrap(), previous_credentials);
        assert_eq!(
            credential_document(&credentials).unwrap()["refs"]["OTHER_KEY"],
            "other"
        );
        save_paths_and_commit(
            std::slice::from_ref(&path),
            &credentials,
            "gateway",
            None,
            false,
            || Ok(()),
        )
        .unwrap();
        assert!(get_providers_from(&path, &credentials).unwrap().is_empty());
        assert_eq!(fs::read(&credentials).unwrap(), previous_credentials);
    }

    #[test]
    fn provider_and_credentials_roll_back_together_on_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let credentials = temp.path().join(".credentials.yaml");
        fs::write(&path, "- id: other\n").unwrap();
        let fail = || Err::<(), _>(AppError::Database("injected failure".into()));
        assert!(save_paths_and_commit(
            std::slice::from_ref(&path),
            &credentials,
            "gateway",
            Some(editor_config()),
            true,
            fail
        )
        .is_err());
        assert_eq!(read_text(&path).unwrap(), "- id: other\n");
        assert!(!credentials.exists());
        let _guard = FileLock::acquire(&path).unwrap();
        assert!(save_paths_and_commit(
            std::slice::from_ref(&path),
            &credentials,
            "gateway",
            Some(editor_config()),
            true,
            || Ok(())
        )
        .is_err());
        assert!(!credentials.exists());
    }

    #[test]
    fn mcp_transport_changes_preserve_native_options_and_other_plugins() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        fs::write(&path,"# keep\n- id: other\n  config: {dynamic: !!js process.env.VALUE}\n- id: github\n  name: '@deepseek-ai/dsh-mcp-client'\n  disabled: true\n  config:\n    serverName: github\n    transport: stdio\n    command: node\n    toolCallTimeoutMs: 12345\n").unwrap();
        sync_mcp_file_and_commit(
            &path,
            "github",
            Some(&json!({"type":"http","url":"https://example.com/mcp"})),
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.contains("dynamic: !!js process.env.VALUE"));
        let entry = entries(&text).unwrap().remove(1);
        assert_eq!(entry["config"]["toolCallTimeoutMs"], 12345);
        assert_eq!(entry["config"]["transport"], "streamable-http");
        assert!(entry["config"].get("command").is_none());
        assert!(entry.get("disabled").is_none());
        assert!(sync_mcp_file_and_commit(
            &path,
            "github",
            Some(&json!({"type":"sse","url":"https://example.com/sse"})),
            || Ok(())
        )
        .is_err());
        assert_eq!(read_text(&path).unwrap(), text);
    }

    #[test]
    fn mcp_edits_and_removes_inserted_plugins_without_removing_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let untouched = "# keep\n- id: other\n  config: {dynamic: !!js process.env.VALUE}\n";
        let group = "- insert:\n  - id: github\n    name: '@deepseek-ai/dsh-mcp-client'\n    disabled: true\n    config: {serverName: github, transport: stdio, command: node, toolCallTimeoutMs: 12345}\n  - id: sibling\n    name: custom-plugin\n    config: {dynamic: other}\n";
        fs::write(&path, format!("{untouched}{group}")).unwrap();
        let sibling = entries(group).unwrap()[0]["insert"][1].clone();
        assert_eq!(mcp_entries_from(group).unwrap().len(), 1);
        sync_mcp_file_and_commit(
            &path,
            "github",
            Some(&json!({"type":"http","url":"https://example.com/mcp"})),
            || Ok(()),
        )
        .unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(untouched));
        let entry = mcp_entries_from(&text).unwrap().remove(0);
        assert_eq!(entry["config"]["toolCallTimeoutMs"], 12345);
        assert_eq!(entry["config"]["transport"], "streamable-http");
        assert!(entry["config"].get("command").is_none());
        assert!(entry.get("disabled").is_none());
        sync_mcp_file_and_commit(&path, "github", None, || Ok(())).unwrap();
        let text = read_text(&path).unwrap();
        assert!(text.starts_with(untouched));
        assert!(mcp_entries_from(&text).unwrap().is_empty());
        assert_eq!(
            entries(&text).unwrap()[1]["insert"],
            Yaml::Sequence(vec![sibling])
        );
    }

    #[test]
    fn mcp_edits_refuse_to_drop_tags_from_rewritten_entries() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let spec = json!({"type":"stdio","command":"node"});
        // Editing or removing one plugin rewrites its whole insert group.
        let group = "- insert:\n  - id: github\n    name: '@deepseek-ai/dsh-mcp-client'\n    config: {serverName: github, transport: stdio, command: node}\n  - id: sibling\n    name: custom-plugin\n    config: {dynamic: !!js process.env.OTHER}\n";
        fs::write(&path, group).unwrap();
        for next in [Some(&spec), None] {
            assert!(sync_mcp_file_and_commit(&path, "github", next, || Ok(())).is_err());
            assert_eq!(read_text(&path).unwrap(), group);
        }
        // A tagged top-level entry can't be edited, but removing it rewrites nothing else.
        let own = "- id: cc-switch-mcp-github\n  name: '@deepseek-ai/dsh-mcp-client'\n  config: {serverName: github, transport: stdio, command: node, env: {TOKEN: !!js process.env.TOKEN}}\n";
        fs::write(&path, own).unwrap();
        assert!(sync_mcp_file_and_commit(&path, "github", Some(&spec), || Ok(())).is_err());
        assert_eq!(read_text(&path).unwrap(), own);
        sync_mcp_file_and_commit(&path, "github", None, || Ok(())).unwrap();
        assert_eq!(read_text(&path).unwrap(), "[]\n");
    }

    #[test]
    fn mcp_rejects_plugin_id_collisions_and_rolls_back_failed_commit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        let original = "- id: cc-switch-mcp-test\n  name: unrelated-plugin\n";
        fs::write(&path, original).unwrap();
        assert!(
            sync_mcp_file_and_commit(&path, "test", Some(&json!({"command":"node"})), || Ok(()))
                .is_err()
        );
        assert_eq!(read_text(&path).unwrap(), original);
        assert!(sync_mcp_file_and_commit(
            &path,
            "other",
            Some(&json!({"command":"node"})),
            || Err::<(), _>(AppError::Database("injected".into()))
        )
        .is_err());
        assert_eq!(read_text(&path).unwrap(), original);
    }

    #[test]
    fn additive_profile_edit_preserves_other_plugins_and_providers() {
        let original = "# user comment\n- id: other\n  name: custom-plugin\n  config:\n    value: !!js process.env.USER_VALUE\n- id: llm-pi-ai\n  config:\n    retryPolicy: {mode: normal}\n    providers:\n      existing: {apiKeyEnv: EXISTING_KEY}\n";
        let next = update_provider(
            original,
            "new-provider",
            Some(json!({"api":"openai-responses","models":[{"id":"model"}]})),
            true,
        )
        .unwrap();
        assert!(next.starts_with(original.split("- id: llm-pi-ai").next().unwrap()));
        let entry = provider_entry(&next).unwrap();
        assert_eq!(entry["config"]["retryPolicy"]["mode"], "normal");
        assert_eq!(
            entry["config"]["providers"]["existing"]["apiKeyEnv"],
            "EXISTING_KEY"
        );
        assert!(update_provider(&next, "new-provider", Some(json!({})), true).is_err());
        let removed = update_provider(&next, "new-provider", None, false).unwrap();
        assert!(provider_entry(&removed).unwrap()["config"]["providers"]
            .get("new-provider")
            .is_none());
    }

    #[test]
    fn provider_edits_refuse_to_reserialize_tags_and_anchors() {
        for native in [
            "      native: {apiKey: !!js process.env.NATIVE_KEY}\n",
            "      native: &shared {apiKeyEnv: NATIVE_KEY}\n      copy: *shared\n",
        ] {
            let original = format!("- id: llm-pi-ai\n  config:\n    providers:\n{native}");
            let error = update_provider(
                &original,
                "new-provider",
                Some(json!({"api":"openai-responses","models":[{"id":"model"}]})),
                true,
            )
            .unwrap_err();
            assert!(error.to_string().contains("YAML tags"));
        }
        // Tag-like text inside quotes and comments is plain data.
        let quoted = "- id: llm-pi-ai\n  config:\n    providers:\n      native: {name: '!!js not a tag', apiKeyEnv: KEY} # !!js note\n";
        let next =
            update_provider(quoted, "new-provider", Some(json!({"models":[]})), true).unwrap();
        assert_eq!(
            provider_entry(&next).unwrap()["config"]["providers"]["native"]["name"],
            "!!js not a tag"
        );
    }

    #[test]
    fn malformed_yaml_and_duplicate_overrides_are_rejected() {
        for text in [
            "{broken",
            "providers: {}",
            "- id: llm-pi-ai\n- id: llm-pi-ai\n",
        ] {
            assert!(update_provider(text, "test", Some(json!({})), false).is_err());
        }
    }

    #[test]
    fn profile_write_holds_native_lock_and_rolls_back_database_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cordis.patch.yml");
        fs::write(&path, "# keep\n[]\n").unwrap();
        let result: Result<(), AppError> = edit_path_and_commit(
            &path,
            |_| Ok("- id: test\n".into()),
            || {
                assert!(path.with_extension("yml.lock").exists());
                Err(AppError::Database("injected failure".into()))
            },
        );
        assert!(result.is_err());
        assert_eq!(read_text(&path).unwrap(), "# keep\n[]\n");
        let guard = FileLock::acquire(&path).unwrap();
        assert!(edit_path_and_commit(&path, |_| Ok(String::new()), || Ok(())).is_err());
        drop(guard);
        assert!(!path.with_extension("yml.lock").exists());
    }

    /// Points `DSH_HOME` at a temporary directory for one test and restores it afterwards.
    struct DshHome {
        dir: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }
    impl DshHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("DSH_HOME");
            std::env::set_var("DSH_HOME", dir.path());
            Self { dir, previous }
        }
    }
    impl Drop for DshHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("DSH_HOME", value),
                None => std::env::remove_var("DSH_HOME"),
            }
        }
    }

    fn init_profiles(home: &Path, names: &[&str]) {
        for name in names {
            let path = home.join("profiles").join(name).join("cordis.patch.yml");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, FACTORY_PROFILE).unwrap();
        }
    }

    fn profile_text(home: &Path, name: &str) -> Option<String> {
        fs::read_to_string(home.join("profiles").join(name).join("cordis.patch.yml")).ok()
    }

    #[test]
    #[serial_test::serial]
    fn provider_writes_prepare_web_and_desktop_and_keep_optional_profiles_native_owned() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web", "headless"]);
        // sdk profiles are for embedding and stay native-owned
        init_profiles(root, &["sdk"]);

        set_provider("gateway", editor_config()).unwrap();
        for name in ["web", "desktop", "headless"] {
            assert!(profile_text(root, name)
                .unwrap()
                .contains("apiKeyEnv: CC_SWITCH_DSH_GATEWAY"));
        }
        assert_eq!(profile_text(root, "sdk").unwrap(), FACTORY_PROFILE);
        assert!(root.join("profiles/desktop/cordis.patch.yml").exists());
        assert!(!root.join("profiles/acp").exists());
        assert!(provider_key_exists("gateway").unwrap());
        assert_eq!(
            get_providers().unwrap()["gateway"]["options"]["apiKey"],
            "test-value"
        );

        remove_provider("gateway").unwrap();
        for name in ["web", "desktop", "headless"] {
            assert!(!profile_text(root, name).unwrap().contains("gateway"));
        }
        assert!(root.join("profiles/desktop/cordis.patch.yml").exists());
    }

    #[test]
    #[serial_test::serial]
    fn display_source_prefers_web_then_desktop() {
        let home = DshHome::new();
        let root = home.dir.path();
        assert_eq!(config_path(), root.join("profiles/web/cordis.patch.yml"));
        init_profiles(root, &["headless"]);
        assert_eq!(
            config_path(),
            root.join("profiles/headless/cordis.patch.yml")
        );
        init_profiles(root, &["desktop"]);
        assert_eq!(
            config_path(),
            root.join("profiles/desktop/cordis.patch.yml")
        );
        init_profiles(root, &["web"]);
        assert_eq!(config_path(), root.join("profiles/web/cordis.patch.yml"));
    }

    #[test]
    #[serial_test::serial]
    fn newly_initialized_desktop_imports_managed_configuration_only_once() {
        use crate::app_config::{AppType, McpApps, McpServer};
        use crate::provider::{Provider, ProviderMeta};
        use crate::services::provider::{initialize_dsh_profiles, ProviderService};

        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web"]);
        let web = profile_path("web");
        // Existing users only had Web when these rows were written.
        save_paths_and_commit(
            &[web.clone()],
            &credentials_path(),
            "gateway",
            Some(editor_config()),
            true,
            || Ok(()),
        )
        .unwrap();
        let spec = json!({"command":"node","args":["fetch.js"]});
        sync_mcp_file_and_commit(&web, "fetch", Some(&spec), || Ok(())).unwrap();
        let before_web = read_text(&web)
            .unwrap()
            .replace("command: node", "command: native-node");
        fs::write(&web, &before_web).unwrap();
        let native_credentials = read_text(&credentials_path())
            .unwrap()
            .replace("test-value", "native-key");
        fs::write(credentials_path(), native_credentials).unwrap();
        let before_credentials = fs::read(credentials_path()).unwrap();
        let state = crate::store::AppState::new(std::sync::Arc::new(
            crate::database::Database::memory().unwrap(),
        ));
        for (id, managed) in [("gateway", true), ("parked", false)] {
            let mut provider = Provider::with_id(id.into(), id.into(), editor_config(), None);
            provider.meta = Some(ProviderMeta {
                live_config_managed: Some(managed),
                ..Default::default()
            });
            state.db.save_provider("dsh", &provider).unwrap();
        }
        for (id, enabled) in [("fetch", true), ("disabled", false)] {
            state
                .db
                .save_mcp_server(&McpServer {
                    id: id.into(),
                    name: id.into(),
                    server: spec.clone(),
                    apps: McpApps {
                        dsh: enabled,
                        ..Default::default()
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: vec![],
                })
                .unwrap();
        }
        // First Desktop launch happens after the managed Web setup.
        init_profiles(root, &["desktop"]);
        let desktop = profile_path("desktop");
        let native = "- id: native-plugin\n  config: {value: !!js process.env.VALUE}\n";
        fs::write(&desktop, native).unwrap();
        let listed = ProviderService::list(&state, AppType::Dsh).unwrap();
        assert!(listed.contains_key("gateway"));
        let providers = get_providers_from(&desktop, &credentials_path()).unwrap();
        assert!(providers.contains_key("gateway"));
        assert_eq!(providers["gateway"]["options"]["apiKey"], "native-key");
        assert!(!providers.contains_key("parked"));
        let text = read_text(&desktop).unwrap();
        assert!(text.starts_with(native));
        assert_eq!(
            mcp_entries_from(&text).unwrap(),
            mcp_entries_from(&before_web).unwrap()
        );
        assert!(text.contains("native-node"));
        assert_eq!(read_text(&web).unwrap(), before_web);
        assert_eq!(fs::read(credentials_path()).unwrap(), before_credentials);

        // Even deleting every managed node natively must survive startup and page refresh.
        fs::write(&desktop, native).unwrap();
        initialize_dsh_profiles(&state).unwrap();
        ProviderService::list(&state, AppType::Dsh).unwrap();
        assert_eq!(read_text(&desktop).unwrap(), native);

        // Optional profiles that appear later receive the same one-time initialization.
        init_profiles(root, &["headless"]);
        initialize_dsh_profiles(&state).unwrap();
        assert!(
            get_providers_from(&profile_path("headless"), &credentials_path())
                .unwrap()
                .contains_key("gateway")
        );
        assert_eq!(read_text(&desktop).unwrap(), native);
        assert!(!profile_path("acp").exists());
    }

    #[test]
    #[serial_test::serial]
    fn profile_initialization_rolls_back_its_receipt_and_can_retry() {
        let _home = DshHome::new();
        let providers = IndexMap::from([("gateway".into(), editor_config())]);
        let servers = IndexMap::from([("fetch".into(), json!({"command":"node"}))]);
        let receipt = data_dir().join(".cc-switch-profile-sync.json");
        let result = initialize_profiles_and_commit(&providers, &servers, || {
            Err::<(), _>(AppError::Database("injected".into()))
        });
        assert!(result.is_err());
        assert!(!receipt.exists());
        assert!(!credentials_path().exists());
        for name in ["web", "desktop"] {
            assert!(!profile_path(name).exists());
        }
        initialize_profiles(&providers, &servers).unwrap();
        for name in ["web", "desktop"] {
            assert!(get_providers_from(&profile_path(name), &credentials_path())
                .unwrap()
                .contains_key("gateway"));
            assert_eq!(
                mcp_entries_from(&read_text(&profile_path(name)).unwrap())
                    .unwrap()
                    .len(),
                1
            );
        }
        let _guard = FileLock::acquire(&profile_path("desktop")).unwrap();
        // A completed import needs no native writes or locks on subsequent startup.
        initialize_profiles(&providers, &servers).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn profile_initialization_is_not_blocked_by_a_home_level_provider_config() {
        let home = DshHome::new();
        let root = home.dir.path();
        fs::write(
            root.join("cordis.patch.yml"),
            "- id: llm-pi-ai\n  config:\n    providers: {}\n",
        )
        .unwrap();
        let providers = IndexMap::from([("gateway".into(), editor_config())]);
        initialize_profiles(&providers, &IndexMap::new()).unwrap();
        assert!(data_dir().join(".cc-switch-profile-sync.json").exists());
        for name in ["web", "desktop"] {
            assert!(get_providers_from(&profile_path(name), &credentials_path())
                .unwrap()
                .contains_key("gateway"));
        }
    }

    #[test]
    #[serial_test::serial]
    fn multi_profile_provider_write_rolls_back_every_file() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web", "desktop"]);
        let credentials = root.join(".credentials.yaml");
        fs::write(&credentials, "version: 1\nrefs:\n  OTHER: kept\n").unwrap();
        let before = |name| profile_text(root, name).unwrap();
        let (web, desktop) = (before("web"), before("desktop"));

        // A failed database commit restores both profiles and the credentials.
        let failed: Result<(), AppError> =
            save_and_commit("gateway", Some(editor_config()), true, || {
                Err(AppError::Database("injected failure".into()))
            });
        assert!(failed.is_err());
        assert_eq!(profile_text(root, "web").unwrap(), web);
        assert_eq!(profile_text(root, "desktop").unwrap(), desktop);
        assert_eq!(
            fs::read_to_string(&credentials).unwrap(),
            "version: 1\nrefs:\n  OTHER: kept\n"
        );

        // An edit one profile refuses writes nothing anywhere.
        let tagged = "- id: llm-pi-ai\n  config:\n    providers:\n      native: !!js \"x\"\n";
        fs::write(root.join("profiles/desktop/cordis.patch.yml"), tagged).unwrap();
        assert!(set_provider("gateway", editor_config()).is_err());
        assert_eq!(profile_text(root, "web").unwrap(), web);
        assert_eq!(profile_text(root, "desktop").unwrap(), tagged);
        assert!(!fs::read_to_string(&credentials)
            .unwrap()
            .contains("CC_SWITCH_DSH_GATEWAY"));
        assert!(!root.join("profiles/web/cordis.patch.yml.lock").exists());
        assert!(!root.join(".credentials.yaml.lock").exists());
    }

    #[test]
    #[serial_test::serial]
    fn home_patch_llm_pi_ai_config_blocks_provider_writes() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web"]);
        fs::write(
            root.join("cordis.patch.yml"),
            "- id: other\n  config: {}\n- id: llm-pi-ai\n  disabled: false\n",
        )
        .unwrap();
        set_provider("gateway", editor_config()).unwrap();
        let written = profile_text(root, "web").unwrap();

        fs::write(
            root.join("cordis.patch.yml"),
            "- id: llm-pi-ai\n  config:\n    providers: {}\n",
        )
        .unwrap();
        let error = set_provider("gateway", editor_config()).unwrap_err();
        assert!(error.to_string().contains("cordis.patch.yml"), "{error}");
        assert_eq!(profile_text(root, "web").unwrap(), written);
        remove_provider("gateway").unwrap();
        assert!(get_providers().unwrap().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn mcp_is_written_to_every_initialized_profile_and_rolls_back_together() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web", "acp"]);
        let spec = json!({"type":"stdio","command":"node","args":["server.js"]});

        sync_mcp_and_commit("memory", Some(&spec), || Ok(())).unwrap();
        for name in ["web", "desktop", "acp"] {
            assert_eq!(
                mcp_entries_from(&profile_text(root, name).unwrap())
                    .unwrap()
                    .len(),
                1
            );
        }
        assert!(root.join("profiles/desktop/cordis.patch.yml").exists());
        assert_eq!(mcp_entries().unwrap().len(), 1);

        let (web, acp) = (
            profile_text(root, "web").unwrap(),
            profile_text(root, "acp").unwrap(),
        );
        let failed: Result<(), AppError> = sync_mcp_and_commit("memory", None, || {
            Err(AppError::Database("injected failure".into()))
        });
        assert!(failed.is_err());
        assert_eq!(profile_text(root, "web").unwrap(), web);
        assert_eq!(profile_text(root, "acp").unwrap(), acp);

        sync_mcp_and_commit("memory", None, || Ok(())).unwrap();
        assert!(mcp_entries().unwrap().is_empty());
    }

    /// 用户在 DSH 原生界面里加的一条供应商：只写进 headless 的 profile。
    const NATIVE_HEADLESS_PROVIDER: &str = "---\n- id: llm-pi-ai\n  config:\n    providers:\n      native-headless:\n        displayName: Native Headless Route\n        api: openai-completions\n        baseURL: https://native-headless.example/v1\n        apiKeyEnv: NATIVE_HEADLESS_KEY\n        models:\n          - id: native-model\n            contextWindow: 128000\n";

    /// 只写进某个 profile 的供应商也要出现在列表里：否则列表看不到它、`provider_key_exists`
    /// 又把它算成已占用，用户既加不进去也找不到，无法自救。
    #[test]
    #[serial_test::serial]
    fn providers_that_only_exist_in_another_profile_are_listed() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web", "headless"]);
        fs::write(
            root.join("profiles/headless/cordis.patch.yml"),
            NATIVE_HEADLESS_PROVIDER,
        )
        .unwrap();

        let listed = get_providers().unwrap();
        assert!(
            listed.contains_key("native-headless"),
            "headless 里的供应商应当出现在列表里: {:?}",
            listed.keys().collect::<Vec<_>>()
        );
        // 列表与 key 占用检查给出同一个结论
        assert!(provider_key_exists("native-headless").unwrap());
        assert_eq!(
            listed["native-headless"]["options"]["baseURL"],
            "https://native-headless.example/v1"
        );

        // 权威 profile（web）里的供应商仍然优先显示
        set_provider("gateway", editor_config()).unwrap();
        let listed = get_providers().unwrap();
        assert!(listed.contains_key("gateway") && listed.contains_key("native-headless"));
        assert_eq!(
            listed["gateway"]["options"]["baseURL"],
            "https://gateway.example/v1"
        );
    }

    /// 同名节点在别的 profile 里被原生改过时，改写会把那份修改静默抹掉：宁可拒绝并指名。
    #[test]
    #[serial_test::serial]
    fn editing_a_provider_is_refused_when_another_profile_differs() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web", "desktop"]);
        set_provider("gateway", editor_config()).unwrap();

        // 用户在 DSH Desktop 原生界面里把这个 key 的 endpoint 改成了别处
        let desktop_path = root.join("profiles/desktop/cordis.patch.yml");
        let edited = profile_text(root, "desktop")
            .unwrap()
            .replace("https://gateway.example/v1", "https://desktop.example/v1");
        fs::write(&desktop_path, &edited).unwrap();
        let web_before = profile_text(root, "web").unwrap();

        let mut renamed = editor_config();
        renamed["name"] = json!("Renamed");
        let error = set_provider("gateway", renamed).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("desktop"), "{message}");
        assert!(message.contains("gateway"), "{message}");
        // 一个字节都没写
        assert_eq!(profile_text(root, "desktop").unwrap(), edited);
        assert_eq!(profile_text(root, "web").unwrap(), web_before);
        assert!(!root.join("profiles/web/cordis.patch.yml.lock").exists());

        // 删除同理：不能把 diverged 的那份一起删掉
        let error = remove_provider("gateway").unwrap_err();
        assert!(error.to_string().contains("desktop"));
        assert_eq!(profile_text(root, "desktop").unwrap(), edited);

        // 把两边改回一致后照常可写
        fs::write(&desktop_path, &web_before).unwrap();
        set_provider("gateway", editor_config()).unwrap();
    }

    /// 添加时若 key 已被别的 profile 占用，报错要指名是哪个 profile。
    #[test]
    #[serial_test::serial]
    fn adding_a_key_that_another_profile_uses_names_that_profile() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web", "headless"]);
        fs::write(
            root.join("profiles/headless/cordis.patch.yml"),
            NATIVE_HEADLESS_PROVIDER,
        )
        .unwrap();
        let before = profile_text(root, "headless").unwrap();

        let mut config = editor_config();
        config["name"] = json!("Native Headless");
        let error = save_and_commit("native-headless", Some(config), true, || Ok(())).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("headless"), "{message}");
        assert_eq!(profile_text(root, "headless").unwrap(), before);
    }

    /// MCP 同款：别的 profile 里的同名服务器内容不同就拒绝改写。
    #[test]
    #[serial_test::serial]
    fn mcp_edit_is_refused_when_another_profile_differs() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web", "desktop"]);
        let spec = json!({"type":"stdio","command":"node","args":["server.js"]});
        sync_mcp_and_commit("memory", Some(&spec), || Ok(())).unwrap();

        let desktop_path = root.join("profiles/desktop/cordis.patch.yml");
        let edited = profile_text(root, "desktop")
            .unwrap()
            .replace("command: node", "command: python3");
        fs::write(&desktop_path, &edited).unwrap();

        let error = sync_mcp_and_commit("memory", Some(&spec), || Ok(())).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("desktop"), "{message}");
        assert_eq!(profile_text(root, "desktop").unwrap(), edited);
    }

    #[test]
    #[serial_test::serial]
    fn mcp_missing_from_web_can_be_edited_and_removed() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["desktop"]);
        let desktop = profile_path("desktop");
        fs::write(&desktop, "- insert:\n  - id: native-fetch\n    name: '@deepseek-ai/dsh-mcp-client'\n    config: {serverName: fetch, transport: stdio, command: node, args: [old.js], toolCallTimeoutMs: 12345}\n").unwrap();
        init_profiles(root, &["web"]);

        let updated = json!({"command":"node","args":["new.js"]});
        sync_mcp_and_commit("fetch", Some(&updated), || Ok(())).unwrap();
        for name in ["web", "desktop"] {
            let text = profile_text(root, name).unwrap();
            assert!(text.contains("new.js"));
            assert!(text.contains("native-fetch"));
            assert!(text.contains("toolCallTimeoutMs: 12345"));
        }
        // Exercise deletion in the same missing-copy window, before first-import runs.
        init_profiles(root, &["web"]);
        sync_mcp_and_commit("fetch", None, || Ok(())).unwrap();
        assert!(mcp_entries().unwrap().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn mcp_missing_from_web_still_rejects_divergent_existing_copies() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web", "desktop", "headless"]);
        for (name, command) in [("desktop", "node"), ("headless", "python")] {
            sync_mcp_file_and_commit(
                &profile_path(name),
                "fetch",
                Some(&json!({"command":command})),
                || Ok(()),
            )
            .unwrap();
        }
        let before = profile_text(root, "desktop");
        for spec in [Some(json!({"command":"npx"})), None] {
            let error = sync_mcp_and_commit("fetch", spec.as_ref(), || Ok(())).unwrap_err();
            assert!(error.to_string().contains("headless"));
            assert_eq!(profile_text(root, "desktop"), before);
            assert_eq!(profile_text(root, "web").unwrap(), FACTORY_PROFILE);
        }
    }

    /// 崩溃留下的 `<file>.lock` 不该把之后的写入永久卡死，但活着的写入方仍然要被尊重。
    #[test]
    #[serial_test::serial]
    fn a_stale_lock_file_does_not_block_writes() {
        let home = DshHome::new();
        let root = home.dir.path();
        init_profiles(root, &["web"]);

        // 主人已经不在了：接管
        let stale = root.join("profiles/web/cordis.patch.yml.lock");
        fs::write(&stale, "4194303\n").unwrap();
        set_provider("gateway", editor_config()).unwrap();
        assert!(!stale.exists(), "残留锁应当被清掉");

        // 主人就是自己：仍然当冲突处理
        fs::write(&stale, format!("{}\n", std::process::id())).unwrap();
        let error = set_provider("other", editor_config()).unwrap_err();
        assert!(
            error.to_string().contains("saving its configuration"),
            "{error}"
        );
        fs::remove_file(&stale).unwrap();
        set_provider("other", editor_config()).unwrap();
    }

    #[test]
    fn removing_an_entry_refuses_to_strand_an_alias() {
        let text = "- id: a\n  config: &shared\n    x: 1\n- id: b\n  config: *shared\n";
        assert!(replace_entry(text, "a", None).is_err());
        assert!(replace_entry(text, "b", None).is_ok());
    }

    #[test]
    #[serial_test::serial]
    fn shared_account_grant_is_read_and_removed_without_touching_api_keys() {
        let home = DshHome::new();
        assert_eq!(account_grant().unwrap(), None);
        let credentials = home.dir.path().join(".credentials.yaml");
        fs::write(
            &credentials,
            concat!(
                "version: 1\n",
                "refs:\n",
                "  DEEPSEEK_API_KEY: sk-test\n",
                "records:\n",
                "  deepseek-account-platform/default:\n",
                "    kind: grant\n",
                "    payload: {version: 1, token: tok, issuer: 'https://platform.deepseek.com'}\n",
                "  deepseek-account-platform/device:\n",
                "    kind: grant\n",
                "    payload: {id: 00000000-0000-4000-8000-000000000000}\n",
            ),
        )
        .unwrap();
        assert_eq!(
            account_grant().unwrap(),
            Some(("tok".into(), "https://platform.deepseek.com".into()))
        );

        assert_eq!(
            remove_account_grant().unwrap(),
            Some(("tok".into(), "https://platform.deepseek.com".into()))
        );
        assert_eq!(account_grant().unwrap(), None);
        let document: Yaml =
            serde_yaml::from_str(&fs::read_to_string(&credentials).unwrap()).unwrap();
        assert_eq!(document["refs"]["DEEPSEEK_API_KEY"], Yaml::from("sk-test"));
        assert!(!document["records"]["deepseek-account-platform/device"].is_null());
        assert_eq!(remove_account_grant().unwrap(), None);
    }
}
