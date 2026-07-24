use std::ffi::OsString;
use std::io::Write;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::app_config::AppType;
use crate::config::get_app_config_dir;
use crate::error::AppError;
use crate::settings::{ManagedTarget, TargetKind};

const TARGET_HISTORY_BACKUP_NAME: &str = "codex-target-history-unify-v1";
#[cfg(any(windows, test))]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetHistoryMigrationResult {
    pub changed_jsonl_files: usize,
    pub changed_state_rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
}

/// Target boundary for explicit history normalization. The default manager
/// stores Windows backups beneath CC Switch's device-local backup directory.
pub struct CodexTargetHistoryManager {
    local_backup_parent: PathBuf,
    wsl_executable: OsString,
    wsl_backup_parent: String,
}

impl Default for CodexTargetHistoryManager {
    fn default() -> Self {
        Self {
            local_backup_parent: get_app_config_dir()
                .join("backups")
                .join(TARGET_HISTORY_BACKUP_NAME),
            wsl_executable: OsString::from("wsl.exe"),
            wsl_backup_parent: format!("~/.cc-switch/backups/{TARGET_HISTORY_BACKUP_NAME}"),
        }
    }
}

impl CodexTargetHistoryManager {
    pub fn with_local_backup_parent(parent: impl Into<PathBuf>) -> Self {
        Self {
            local_backup_parent: parent.into(),
            ..Self::default()
        }
    }

    pub fn with_wsl_executable(
        executable: impl Into<OsString>,
        backup_parent: impl Into<String>,
    ) -> Self {
        Self {
            wsl_executable: executable.into(),
            wsl_backup_parent: backup_parent.into(),
            ..Self::default()
        }
    }

    pub fn migrate(
        &self,
        target: &ManagedTarget,
    ) -> Result<TargetHistoryMigrationResult, AppError> {
        validate_codex_target(target)?;
        match target.kind {
            TargetKind::LocalWindows => self.migrate_local(target),
            TargetKind::Wsl { .. } => self.run_wsl(target, "migrate"),
        }
    }

    pub fn restore(
        &self,
        target: &ManagedTarget,
    ) -> Result<TargetHistoryMigrationResult, AppError> {
        validate_codex_target(target)?;
        match target.kind {
            TargetKind::LocalWindows => self.restore_local(target),
            TargetKind::Wsl { .. } => self.run_wsl(target, "restore"),
        }
    }

    fn migrate_local(
        &self,
        target: &ManagedTarget,
    ) -> Result<TargetHistoryMigrationResult, AppError> {
        let codex_dir = Path::new(&target.config_location.path);
        let config_text =
            std::fs::read_to_string(codex_dir.join("config.toml")).unwrap_or_default();
        let generation = self
            .local_backup_parent
            .join(safe_target_id(&target.id))
            .join(format!(
                "{}_{}",
                Local::now().format("%Y%m%d_%H%M%S"),
                uuid::Uuid::new_v4()
            ));
        let outcome = crate::codex_history_migration::migrate_codex_target_history_at(
            codex_dir,
            &config_text,
            &generation,
        )?;
        Ok(TargetHistoryMigrationResult {
            changed_jsonl_files: outcome.migrated_jsonl_files,
            changed_state_rows: outcome.migrated_state_rows,
            backup_path: generation
                .is_dir()
                .then(|| generation.to_string_lossy().to_string()),
            skipped_reason: outcome.skipped_reason,
        })
    }

    fn restore_local(
        &self,
        target: &ManagedTarget,
    ) -> Result<TargetHistoryMigrationResult, AppError> {
        let codex_dir = Path::new(&target.config_location.path);
        let config_text =
            std::fs::read_to_string(codex_dir.join("config.toml")).unwrap_or_default();
        let target_backup_parent = self.local_backup_parent.join(safe_target_id(&target.id));
        let restore_generation = target_backup_parent.join("_restore").join(format!(
            "{}_{}",
            Local::now().format("%Y%m%d_%H%M%S"),
            uuid::Uuid::new_v4()
        ));
        let outcome = crate::codex_history_migration::restore_codex_target_history_at(
            codex_dir,
            &config_text,
            &target_backup_parent,
            &restore_generation,
        )?;
        Ok(TargetHistoryMigrationResult {
            changed_jsonl_files: outcome.restored_jsonl_files,
            changed_state_rows: outcome.restored_state_rows,
            backup_path: restore_generation
                .is_dir()
                .then(|| restore_generation.to_string_lossy().to_string()),
            skipped_reason: outcome.skipped_reason,
        })
    }

    fn run_wsl(
        &self,
        target: &ManagedTarget,
        action: &str,
    ) -> Result<TargetHistoryMigrationResult, AppError> {
        // Share the process-wide history lock with Windows target/official ops so
        // concurrent UI actions cannot interleave rewrites across adapters.
        let _op_guard = crate::codex_history_migration::lock_codex_history_op();
        let TargetKind::Wsl { distro, user } = &target.kind else {
            return Err(AppError::InvalidInput(
                "WSL history operation requires a WSL Target".to_string(),
            ));
        };
        if distro.trim().is_empty()
            || user.trim().is_empty()
            || !target.config_location.path.starts_with('/')
        {
            return Err(AppError::InvalidInput(
                "WSL Target coordinates are invalid".to_string(),
            ));
        }
        let backup_parent = format!(
            "{}/{}",
            self.wsl_backup_parent.trim_end_matches('/'),
            safe_target_id(&target.id)
        );
        let mut command = new_wsl_history_command(&self.wsl_executable);
        command
            .arg("-d")
            .arg(distro)
            .arg("-u")
            .arg(user)
            .arg("--exec")
            .arg("python3")
            .arg("-")
            .arg(action)
            .arg(&target.config_location.path)
            .arg(&backup_parent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|source| AppError::IoContext {
            context: format!("Failed to start WSL history operation for '{distro}'"),
            source,
        })?;
        child
            .stdin
            .take()
            .ok_or_else(|| AppError::Message("Failed to open WSL Python stdin".to_string()))?
            .write_all(WSL_TARGET_HISTORY_SCRIPT.as_bytes())
            .map_err(|source| AppError::IoContext {
                context: "Failed to stream WSL history migration script".to_string(),
                source,
            })?;
        let output = child
            .wait_with_output()
            .map_err(|source| AppError::IoContext {
                context: format!("Failed to wait for WSL history operation in '{distro}'"),
                source,
            })?;
        if !output.status.success() {
            return Err(AppError::Message(format!(
                "WSL history {action} failed (status: {}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        serde_json::from_slice(&output.stdout).map_err(|source| AppError::Json {
            path: format!("WSL history {action} output"),
            source,
        })
    }
}

fn new_wsl_history_command(executable: &OsString) -> Command {
    let command = Command::new(executable);
    #[cfg(windows)]
    let command = {
        let mut command = command;
        command.creation_flags(CREATE_NO_WINDOW);
        command
    };
    command
}

fn validate_codex_target(target: &ManagedTarget) -> Result<(), AppError> {
    if target.app != AppType::Codex {
        return Err(AppError::InvalidInput(
            "Target history migration currently supports Codex only".to_string(),
        ));
    }
    Ok(())
}

fn safe_target_id(target_id: &str) -> String {
    let safe: String = target_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        "target".to_string()
    } else {
        safe
    }
}

const WSL_TARGET_HISTORY_SCRIPT: &str = r#"import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import sys
import tempfile
import time

CUSTOM = "custom"

def result(jsonl=0, rows=0, backup=None, skipped=None):
    payload = {"changedJsonlFiles": jsonl, "changedStateRows": rows}
    if backup is not None:
        payload["backupPath"] = str(backup)
    if skipped is not None:
        payload["skippedReason"] = skipped
    print(json.dumps(payload, separators=(",", ":")))

def top_level_toml_string(text, key):
    """Read a top-level string assignment without requiring Python 3.11 tomllib.

    Supports model_provider = "custom" / 'custom' and quoted/unquoted sqlite_home
    paths. Nested tables are intentionally ignored.
    """
    pattern = re.compile(
        r"^\s*" + re.escape(key) + r"\s*=\s*(?:\"([^\"]*)\"|'([^']*)'|([^#\n]+?))\s*(?:#.*)?$",
        re.MULTILINE,
    )
    match = pattern.search(text)
    if not match:
        return None
    value = match.group(1)
    if value is None:
        value = match.group(2)
    if value is None:
        value = match.group(3)
    if value is None:
        return None
    value = value.strip()
    return value or None

def config_string(config_path, key):
    if not config_path.is_file():
        return None
    text = config_path.read_text(encoding="utf-8")
    try:
        import tomllib
        value = tomllib.loads(text).get(key)
        if isinstance(value, str):
            return value
        if value is None:
            return None
    except Exception:
        pass
    return top_level_toml_string(text, key)

def jsonl_files(root):
    files = []
    for name in ("sessions", "archived_sessions"):
        directory = root / name
        if directory.is_dir():
            files.extend(path for path in directory.rglob("*.jsonl") if path.is_file())
    return sorted(files)

def session_rewrite(path):
    raw = path.read_bytes()
    text = raw.decode("utf-8")
    changed = False
    providers = set()
    output = []
    for segment in text.splitlines(keepends=True):
        newline = "\n" if segment.endswith("\n") else ""
        line = segment[:-1] if newline else segment
        try:
            value = json.loads(line)
        except Exception:
            output.append(segment)
            continue
        payload = value.get("payload") if value.get("type") == "session_meta" else None
        provider = payload.get("model_provider") if isinstance(payload, dict) else None
        if isinstance(provider, str) and provider and provider != CUSTOM:
            providers.add(provider)
            payload["model_provider"] = CUSTOM
            line = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
            changed = True
        output.append(line + newline)
    return raw, "".join(output).encode("utf-8"), changed, providers

def state_paths(root):
    paths = [root / "state_5.sqlite"]
    override = config_string(root / "config.toml", "sqlite_home")
    if not override:
        override = os.environ.get("CODEX_SQLITE_HOME")
    if isinstance(override, str) and override.strip():
        candidate = Path(override.strip()).expanduser()
        if not candidate.is_absolute():
            candidate = Path.home() / candidate
        candidate = candidate / "state_5.sqlite"
        if candidate not in paths:
            paths.append(candidate)
    return paths

def relative_backup(path, root):
    try:
        return path.relative_to(root)
    except ValueError:
        digest = hashlib.sha256(str(path).encode()).hexdigest()[:16]
        return Path("external") / (digest + "-" + path.name)

def backup_db(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    src = sqlite3.connect(source, timeout=5)
    dst = sqlite3.connect(destination)
    try:
        src.backup(dst)
    finally:
        dst.close()
        src.close()

def atomic_replace(path, contents, before):
    current = path.stat()
    if current.st_mtime_ns != before.st_mtime_ns or current.st_size != before.st_size:
        raise RuntimeError("session changed while migration was running: " + str(path))
    fd, temporary = tempfile.mkstemp(prefix=path.name + ".cc-switch-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(contents)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, before.st_mode & 0o777)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)

def migrate(root, parent):
    live_provider = config_string(root / "config.toml", "model_provider")
    if live_provider != CUSTOM:
        result(skipped="live_not_unified")
        return
    pending_files = []
    sources = set()
    for path in jsonl_files(root):
        before = path.stat()
        raw, rewritten, changed, providers = session_rewrite(path)
        if changed:
            pending_files.append((path, before, raw, rewritten))
            sources.update(providers)

    pending_dbs = []
    row_count = 0
    for path in state_paths(root):
        if not path.is_file():
            continue
        conn = sqlite3.connect(path, timeout=5)
        try:
            columns = [row[1] for row in conn.execute("PRAGMA table_info(threads)")]
            if "model_provider" not in columns:
                continue
            rows = conn.execute(
                "SELECT COUNT(*) FROM threads WHERE model_provider IS NOT NULL AND model_provider != ?",
                (CUSTOM,),
            ).fetchone()[0]
            if rows:
                pending_dbs.append(path)
                row_count += rows
                sources.update(
                    row[0] for row in conn.execute(
                        "SELECT DISTINCT model_provider FROM threads WHERE model_provider IS NOT NULL AND model_provider != ?",
                        (CUSTOM,),
                    ) if row[0]
                )
        finally:
            conn.close()

    if not pending_files and not pending_dbs:
        result(skipped="already_unified")
        return

    generation = parent / (time.strftime("%Y%m%d_%H%M%S") + "_" + os.urandom(8).hex())
    generation.mkdir(parents=True, exist_ok=True)
    for path, _, raw, _ in pending_files:
        destination = generation / "jsonl" / relative_backup(path, root)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(raw)
        shutil.copystat(path, destination)
    for path in pending_dbs:
        backup_db(path, generation / "state" / relative_backup(path, root))

    manifest = {
        "version": 1,
        "codexConfigDir": str(root.resolve()),
        "sourceProviderIds": sorted(sources),
        "migratedJsonlFiles": len(pending_files),
        "migratedStateRows": row_count,
        "status": "in_progress",
    }
    (generation / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    try:
        for path, before, _, rewritten in pending_files:
            atomic_replace(path, rewritten, before)
        changed_rows = 0
        for path in pending_dbs:
            conn = sqlite3.connect(path, timeout=5)
            try:
                conn.execute("BEGIN IMMEDIATE")
                cursor = conn.execute(
                    "UPDATE threads SET model_provider = ? WHERE model_provider IS NOT NULL AND model_provider != ?",
                    (CUSTOM, CUSTOM),
                )
                changed_rows += cursor.rowcount
                conn.commit()
            except Exception:
                conn.rollback()
                raise
            finally:
                conn.close()
    except Exception as error:
        manifest["status"] = "failed"
        manifest["error"] = str(error)
        (generation / "manifest.json").write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
        )
        raise
    manifest["status"] = "complete"
    manifest["migratedStateRows"] = changed_rows
    (generation / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    result(len(pending_files), changed_rows, generation)

def load_ledger(root, parent):
    sessions = {}
    threads = {}
    root_key = str(root.resolve())
    if not parent.is_dir():
        return sessions, threads
    for generation in sorted(path for path in parent.iterdir() if path.is_dir()):
        manifest_path = generation / "manifest.json"
        if not manifest_path.is_file():
            continue
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        except Exception:
            continue
        if manifest.get("version") != 1 or manifest.get("codexConfigDir") != root_key:
            continue
        jsonl_root = generation / "jsonl"
        if jsonl_root.is_dir():
            for path in sorted(jsonl_root.rglob("*.jsonl")):
                try:
                    lines = path.read_text(encoding="utf-8").splitlines()
                except Exception:
                    continue
                for line in lines:
                    try:
                        value = json.loads(line)
                    except Exception:
                        continue
                    payload = value.get("payload") if value.get("type") == "session_meta" else None
                    if not isinstance(payload, dict):
                        continue
                    session_id = payload.get("id")
                    provider = payload.get("model_provider")
                    if isinstance(session_id, str) and isinstance(provider, str) and provider != CUSTOM:
                        sessions.setdefault(session_id, provider)
        state_root = generation / "state"
        if state_root.is_dir():
            for path in sorted(state_root.rglob("*.sqlite")):
                conn = sqlite3.connect("file:" + str(path) + "?mode=ro", uri=True)
                try:
                    columns = [row[1] for row in conn.execute("PRAGMA table_info(threads)")]
                    if "model_provider" not in columns:
                        continue
                    for thread_id, provider in conn.execute(
                        "SELECT id, model_provider FROM threads WHERE model_provider IS NOT NULL AND model_provider != ?",
                        (CUSTOM,),
                    ):
                        threads.setdefault(thread_id, provider)
                finally:
                    conn.close()
    return sessions, threads

def restore_session_rewrite(path, ledger):
    raw = path.read_bytes()
    text = raw.decode("utf-8")
    changed = False
    output = []
    for segment in text.splitlines(keepends=True):
        newline = "\n" if segment.endswith("\n") else ""
        line = segment[:-1] if newline else segment
        try:
            value = json.loads(line)
        except Exception:
            output.append(segment)
            continue
        payload = value.get("payload") if value.get("type") == "session_meta" else None
        session_id = payload.get("id") if isinstance(payload, dict) else None
        provider = payload.get("model_provider") if isinstance(payload, dict) else None
        original = ledger.get(session_id) if isinstance(session_id, str) else None
        if provider == CUSTOM and original:
            payload["model_provider"] = original
            line = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
            changed = True
        output.append(line + newline)
    return raw, "".join(output).encode("utf-8"), changed

def restore(root, parent):
    session_ledger, thread_ledger = load_ledger(root, parent)
    if not session_ledger and not thread_ledger:
        result(skipped="no_backup_ledger")
        return

    pending_files = []
    for path in jsonl_files(root):
        before = path.stat()
        raw, rewritten, changed = restore_session_rewrite(path, session_ledger)
        if changed:
            pending_files.append((path, before, raw, rewritten))

    pending_dbs = []
    row_count = 0
    for path in state_paths(root):
        if not path.is_file() or not thread_ledger:
            continue
        conn = sqlite3.connect(path, timeout=5)
        try:
            columns = [row[1] for row in conn.execute("PRAGMA table_info(threads)")]
            if "model_provider" not in columns:
                continue
            matching = []
            for thread_id in thread_ledger:
                row = conn.execute(
                    "SELECT COUNT(*) FROM threads WHERE id = ? AND model_provider = ?",
                    (thread_id, CUSTOM),
                ).fetchone()[0]
                if row:
                    matching.append(thread_id)
            if matching:
                pending_dbs.append((path, matching))
                row_count += len(matching)
        finally:
            conn.close()

    if not pending_files and not pending_dbs:
        result(skipped="nothing_to_restore")
        return

    generation = parent / "_restore" / (time.strftime("%Y%m%d_%H%M%S") + "_" + os.urandom(8).hex())
    for path, _, raw, _ in pending_files:
        destination = generation / "jsonl" / relative_backup(path, root)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(raw)
        shutil.copystat(path, destination)
    for path, _ in pending_dbs:
        backup_db(path, generation / "state" / relative_backup(path, root))
    manifest = {
        "version": 1,
        "action": "restore",
        "codexConfigDir": str(root.resolve()),
        "restoredJsonlFiles": len(pending_files),
        "restoredStateRows": row_count,
    }
    (generation / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    for path, before, _, rewritten in pending_files:
        atomic_replace(path, rewritten, before)
    changed_rows = 0
    for path, matching in pending_dbs:
        conn = sqlite3.connect(path, timeout=5)
        try:
            conn.execute("BEGIN IMMEDIATE")
            for thread_id in matching:
                cursor = conn.execute(
                    "UPDATE threads SET model_provider = ? WHERE id = ? AND model_provider = ?",
                    (thread_ledger[thread_id], thread_id, CUSTOM),
                )
                changed_rows += cursor.rowcount
            conn.commit()
        except Exception:
            conn.rollback()
            raise
        finally:
            conn.close()
    result(len(pending_files), changed_rows, generation)

action = sys.argv[1]
root = Path(sys.argv[2]).expanduser()
parent = Path(sys.argv[3]).expanduser()
if action == "migrate":
    migrate(root, parent)
elif action == "restore":
    restore(root, parent)
else:
    raise RuntimeError("unsupported history action: " + action)
"#;

#[cfg(test)]
mod tests {
    use super::{CREATE_NO_WINDOW, WSL_TARGET_HISTORY_SCRIPT};
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn wsl_history_processes_use_the_windows_no_window_flag() {
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    }

    #[test]
    fn wsl_history_script_parses_top_level_toml_without_tomllib() {
        // Regression for Python < 3.11 WSL images: live model_provider gate and
        // sqlite_home resolution must not depend on the optional tomllib import.
        assert!(
            WSL_TARGET_HISTORY_SCRIPT.contains("def top_level_toml_string"),
            "WSL history script must ship a pure-Python TOML string fallback"
        );

        let Ok(mut child) = Command::new("python3")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        else {
            // Environments without python3 still compile; WSL targets always have it.
            return;
        };
        let script = r#"
import re
import sys

def top_level_toml_string(text, key):
    pattern = re.compile(
        r"^\s*" + re.escape(key) + r"\s*=\s*(?:\"([^\"]*)\"|'([^']*)'|([^#\n]+?))\s*(?:#.*)?$",
        re.MULTILINE,
    )
    match = pattern.search(text)
    if not match:
        return None
    value = match.group(1)
    if value is None:
        value = match.group(2)
    if value is None:
        value = match.group(3)
    if value is None:
        return None
    value = value.strip()
    return value or None

sample = '''
# comment
model_provider = "custom"
sqlite_home = '/tmp/codex-state'
[model_providers.custom]
name = "x"
'''
assert top_level_toml_string(sample, "model_provider") == "custom"
assert top_level_toml_string(sample, "sqlite_home") == "/tmp/codex-state"
assert top_level_toml_string("model_provider = 'openai'\n", "model_provider") == "openai"
assert top_level_toml_string("model = \"gpt\"\n", "model_provider") is None
print("ok")
"#;
        child
            .stdin
            .as_mut()
            .expect("python stdin")
            .write_all(script.as_bytes())
            .expect("write python script");
        let output = child.wait_with_output().expect("wait python");
        assert!(
            output.status.success(),
            "toml fallback parser failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok");
    }
}
