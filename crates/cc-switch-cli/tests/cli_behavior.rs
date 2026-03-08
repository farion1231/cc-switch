use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::prelude::*;
use cc_switch_core::{
    provider::ProviderMeta, AppSettings, AppState, Database, InstalledSkill, Provider, SkillApps,
};
use rusqlite::{params, Connection};
use serde_json::Value;
use serial_test::serial;
use tempfile::tempdir;

fn run_cli(home: &Path, args: &[&str]) -> Output {
    run_cli_with_env(home, args, &[])
}

fn run_cli_with_env(home: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cc-switch"))
        .args(args)
        .env("HOME", home)
        .env("CC_SWITCH_TEST_HOME", home)
        .envs(envs.iter().copied())
        .output()
        .expect("cli command should run")
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be utf-8")
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be utf-8")
}

fn database_path(home: &Path) -> PathBuf {
    home.join(".cc-switch").join("cc-switch.db")
}

fn backup_dir(home: &Path) -> PathBuf {
    home.join(".cc-switch").join("backups")
}

fn claude_settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

fn claude_plugin_config_path(home: &Path) -> PathBuf {
    home.join(".claude").join("config.json")
}

fn zshrc_path(home: &Path) -> PathBuf {
    home.join(".zshrc")
}

fn claude_prompt_path(home: &Path) -> PathBuf {
    home.join(".claude").join("CLAUDE.md")
}

fn claude_mcp_path(home: &Path) -> PathBuf {
    home.join(".claude.json")
}

fn codex_config_path(home: &Path) -> PathBuf {
    home.join(".codex").join("config.toml")
}

fn opencode_config_path(home: &Path) -> PathBuf {
    home.join(".config").join("opencode").join("opencode.json")
}

fn omo_local_path(home: &Path) -> PathBuf {
    home.join(".config")
        .join("opencode")
        .join("oh-my-opencode.jsonc")
}

fn omo_slim_local_path(home: &Path) -> PathBuf {
    home.join(".config")
        .join("opencode")
        .join("oh-my-opencode-slim.jsonc")
}

fn openclaw_workspace_file_path(home: &Path, filename: &str) -> PathBuf {
    home.join(".openclaw").join("workspace").join(filename)
}

fn openclaw_config_path(home: &Path) -> PathBuf {
    home.join(".openclaw").join("openclaw.json")
}

fn openclaw_memory_file_path(home: &Path, filename: &str) -> PathBuf {
    home.join(".openclaw")
        .join("workspace")
        .join("memory")
        .join(filename)
}

fn claude_session_path(home: &Path, project: &str, filename: &str) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(project)
        .join(filename)
}

fn skill_ssot_dir(home: &Path, directory: &str) -> PathBuf {
    home.join(".cc-switch").join("skills").join(directory)
}

fn claude_skill_dir(home: &Path, directory: &str) -> PathBuf {
    home.join(".claude").join("skills").join(directory)
}

fn exists_or_symlink(path: &Path) -> bool {
    path.exists()
        || path
            .symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink())
}

fn with_seeded_state<T>(home: &Path, f: impl FnOnce(&AppState) -> T) -> T {
    let previous = env::var("CC_SWITCH_TEST_HOME").ok();
    let previous_home = env::var("HOME").ok();
    env::set_var("HOME", home);
    env::set_var("CC_SWITCH_TEST_HOME", home);
    cc_switch_core::settings::update_settings(AppSettings::default()).expect("default settings");
    let state = AppState::new(Database::new().expect("file database"));
    let result = f(&state);
    match previous {
        Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
        None => env::remove_var("CC_SWITCH_TEST_HOME"),
    }
    match previous_home {
        Some(value) => env::set_var("HOME", value),
        None => env::remove_var("HOME"),
    }
    result
}

fn ensure_persisted_state(home: &Path) {
    with_seeded_state(home, |_state| ());
}

fn seed_installed_skill(home: &Path, id: &str, directory: &str) {
    with_seeded_state(home, |state| {
        let ssot_dir = skill_ssot_dir(home, directory);
        fs::create_dir_all(&ssot_dir).expect("ssot skill dir");
        fs::write(
            ssot_dir.join("SKILL.md"),
            "---\nname: Demo Skill\ndescription: seeded skill\n---\n",
        )
        .expect("write skill");
        state
            .db
            .save_skill(&InstalledSkill {
                id: id.to_string(),
                name: "Demo Skill".to_string(),
                description: Some("seeded skill".to_string()),
                directory: directory.to_string(),
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps: SkillApps::default(),
                installed_at: 1,
            })
            .expect("save skill");
    });
}

fn seed_unmanaged_skill(home: &Path, relative_dir: &str, name: &str, description: &str) {
    let dir = home.join(relative_dir);
    fs::create_dir_all(&dir).expect("create unmanaged skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .expect("write unmanaged skill");
}

fn create_skill_zip(zip_path: &Path, root_dir: &str, name: &str, description: &str) {
    let zip_file = fs::File::create(zip_path).expect("create zip file");
    let mut writer = zip::ZipWriter::new(zip_file);
    let options = zip::write::SimpleFileOptions::default();

    writer
        .add_directory(root_dir, options)
        .expect("add zip root dir");
    writer
        .start_file(format!("{root_dir}/SKILL.md"), options)
        .expect("start SKILL.md");
    writer
        .write_all(format!("---\nname: {name}\ndescription: {description}\n---\n").as_bytes())
        .expect("write SKILL.md");
    writer
        .start_file(format!("{root_dir}/notes.txt"), options)
        .expect("start notes file");
    writer.write_all(b"zip body").expect("write notes");
    writer.finish().expect("finish zip");
}

fn seed_provider(home: &Path, app_type: &str, provider: Provider) {
    with_seeded_state(home, |state| {
        state
            .db
            .save_provider(app_type, &provider)
            .expect("save provider");
    });
}

fn seed_claude_session(home: &Path, project: &str, filename: &str) -> PathBuf {
    let path = claude_session_path(home, project, filename);
    fs::create_dir_all(
        path.parent()
            .expect("claude session file should have a parent directory"),
    )
    .expect("create claude session directory");
    fs::write(
        &path,
        concat!(
            "{\"sessionId\":\"session-1\",\"cwd\":\"/work/demo-project\",\"timestamp\":\"2026-03-08T10:00:00Z\",\"isMeta\":true}\n",
            "{\"message\":{\"role\":\"user\",\"content\":\"hello from claude\"},\"timestamp\":\"2026-03-08T10:01:00Z\"}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"done\"},\"timestamp\":\"2026-03-08T10:02:00Z\"}\n"
        ),
    )
    .expect("write claude session fixture");
    path
}

#[allow(clippy::too_many_arguments)]
fn insert_usage_log(
    home: &Path,
    request_id: &str,
    app_type: &str,
    provider_id: &str,
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
    total_cost: &str,
    created_at: i64,
) {
    ensure_persisted_state(home);
    let conn = Connection::open(database_path(home)).expect("open database");
    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model,
            input_tokens, output_tokens, total_cost_usd,
            latency_ms, status_code, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            request_id,
            provider_id,
            app_type,
            model,
            input_tokens,
            output_tokens,
            total_cost,
            120i64,
            200i64,
            created_at,
        ],
    )
    .expect("insert usage log");
}

fn spawn_json_server(response_body: String, expected_requests: usize) -> String {
    spawn_routing_json_server(vec![("/".to_string(), response_body)], expected_requests)
}

fn spawn_routing_json_server(routes: Vec<(String, String)>, expected_requests: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let addr = listener.local_addr().expect("server addr");

    std::thread::spawn(move || {
        for stream in listener.incoming().take(expected_requests) {
            let Ok(mut stream) = stream else {
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer);
            let request = String::from_utf8_lossy(&buffer);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");
            let body = routes
                .iter()
                .find(|(candidate, _)| candidate == path)
                .or_else(|| routes.iter().find(|(candidate, _)| candidate == "/"))
                .map(|(_, body)| body.as_str())
                .unwrap_or("{}");

            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    format!("http://{}", addr)
}

#[cfg(unix)]
fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).expect("write executable");
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("set permissions");
}

fn prepend_path(dir: &Path) -> String {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }
    env::join_paths(paths)
        .expect("join PATH")
        .to_string_lossy()
        .into_owned()
}

struct TestWebDavServer {
    base_url: String,
}

type RawHttpRequest = (String, String, HashMap<String, String>, Vec<u8>);

impl TestWebDavServer {
    fn base_url(&self) -> &str {
        &self.base_url
    }
}

fn spawn_webdav_server() -> TestWebDavServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind webdav server");
    let addr = listener.local_addr().expect("webdav addr");
    let files = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));
    let etags = Arc::new(Mutex::new(HashMap::<String, String>::new()));
    let directories = Arc::new(Mutex::new(HashSet::<String>::from([
        normalize_webdav_path("/dav"),
    ])));

    let files_for_thread = files.clone();
    let etags_for_thread = etags.clone();
    let directories_for_thread = directories.clone();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let _ = handle_webdav_connection(
                &mut stream,
                &files_for_thread,
                &etags_for_thread,
                &directories_for_thread,
            );
        }
    });

    TestWebDavServer {
        base_url: format!("http://{addr}/dav"),
    }
}

fn handle_webdav_connection(
    stream: &mut TcpStream,
    files: &Arc<Mutex<HashMap<String, Vec<u8>>>>,
    etags: &Arc<Mutex<HashMap<String, String>>>,
    directories: &Arc<Mutex<HashSet<String>>>,
) -> std::io::Result<()> {
    let Some((method, raw_target, headers, body)) = read_http_request(stream)? else {
        return Ok(());
    };
    let path = normalize_webdav_path(&raw_target);

    match method.as_str() {
        "PROPFIND" => write_response(
            stream,
            207,
            "Multi-Status",
            &[("content-type", "application/xml")],
            b"<?xml version=\"1.0\"?><multistatus xmlns=\"DAV:\"/>",
        ),
        "MKCOL" => {
            directories.lock().expect("directories lock").insert(path);
            write_response(stream, 201, "Created", &[], b"")
        }
        "PUT" => {
            let etag = format!("\"{}-{}\"", raw_target.len(), body.len());
            files.lock().expect("files lock").insert(path.clone(), body);
            etags.lock().expect("etags lock").insert(path, etag.clone());
            write_response(stream, 201, "Created", &[("etag", &etag)], b"")
        }
        "HEAD" => {
            if let Some(file) = files.lock().expect("files lock").get(&path).cloned() {
                let etag = etags
                    .lock()
                    .expect("etags lock")
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| "\"etag\"".to_string());
                let content_length = file.len().to_string();
                write_response(
                    stream,
                    200,
                    "OK",
                    &[("etag", &etag), ("content-length", &content_length)],
                    b"",
                )
            } else {
                write_response(stream, 404, "Not Found", &[], b"")
            }
        }
        "GET" => {
            if let Some(file) = files.lock().expect("files lock").get(&path).cloned() {
                let etag = etags
                    .lock()
                    .expect("etags lock")
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| "\"etag\"".to_string());
                write_response(
                    stream,
                    200,
                    "OK",
                    &[
                        ("etag", &etag),
                        ("content-type", content_type_for_path(&path)),
                    ],
                    &file,
                )
            } else {
                write_response(stream, 404, "Not Found", &[], b"")
            }
        }
        _ => {
            let _depth = headers.get("depth").cloned().unwrap_or_default();
            write_response(stream, 405, "Method Not Allowed", &[], b"")
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Option<RawHttpRequest>> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
    };

    let header_bytes = &buffer[..header_end];
    let mut body = buffer[header_end + 4..].to_vec();
    let header_text = String::from_utf8_lossy(header_bytes);
    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect::<HashMap<_, _>>();

    let content_length = headers
        .get("content-length")
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(0);
    while body.len() < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Ok(Some((method, target, headers, body)))
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn normalize_webdav_path(raw_target: &str) -> String {
    let path = raw_target.split('?').next().unwrap_or(raw_target);
    if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path.to_string()
    }
}

fn content_type_for_path(path: &str) -> &'static str {
    if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    }
}

fn write_response(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> std::io::Result<()> {
    let mut response = format!(
        "HTTP/1.1 {status_code} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");

    stream.write_all(response.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[test]
#[serial]
fn quiet_mode_suppresses_success_output_and_config_get_returns_json() {
    let temp = tempdir().expect("tempdir");

    let set_output = run_cli(temp.path(), &["--quiet", "config", "set", "language", "zh"]);
    assert!(
        set_output.status.success(),
        "stderr: {}",
        stderr_text(&set_output)
    );
    assert!(stdout_text(&set_output).trim().is_empty());

    let get_output = run_cli(
        temp.path(),
        &["--format", "json", "config", "get", "language"],
    );
    assert!(
        get_output.status.success(),
        "stderr: {}",
        stderr_text(&get_output)
    );

    let value: Value =
        serde_json::from_slice(&get_output.stdout).expect("config get should return json");
    assert_eq!(value.get("language").and_then(Value::as_str), Some("zh"));
}

#[test]
#[serial]
fn quiet_mode_overrides_verbose_output_for_value_commands() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(
        temp.path(),
        &["--quiet", "--verbose", "--format", "json", "config", "path"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));
    assert!(stdout_text(&output).trim().is_empty());
    assert!(stderr_text(&output).trim().is_empty());
}

#[test]
#[serial]
fn workspace_read_write_round_trip() {
    let temp = tempdir().expect("tempdir");

    let write_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "workspace",
            "write",
            "AGENTS.md",
            "--value",
            "workspace hello",
        ],
    );
    assert!(
        write_output.status.success(),
        "stderr: {}",
        stderr_text(&write_output)
    );
    let written: Value =
        serde_json::from_slice(&write_output.stdout).expect("workspace write should return json");
    assert_eq!(written.get("written").and_then(Value::as_bool), Some(true));
    assert_eq!(
        fs::read_to_string(openclaw_workspace_file_path(temp.path(), "AGENTS.md"))
            .expect("workspace file should exist"),
        "workspace hello"
    );

    let read_output = run_cli(
        temp.path(),
        &["--format", "json", "workspace", "read", "AGENTS.md"],
    );
    assert!(
        read_output.status.success(),
        "stderr: {}",
        stderr_text(&read_output)
    );
    let read: Value =
        serde_json::from_slice(&read_output.stdout).expect("workspace read should return json");
    assert_eq!(
        read.get("filename").and_then(Value::as_str),
        Some("AGENTS.md")
    );
    assert_eq!(
        read.get("content").and_then(Value::as_str),
        Some("workspace hello")
    );
}

#[test]
#[serial]
fn workspace_memory_list_search_read_write_and_delete_round_trip() {
    let temp = tempdir().expect("tempdir");
    let memory_source = temp.path().join("memory.md");
    fs::write(&memory_source, "Phase one notes\nWorkspace migration\n").expect("write memory");

    let write_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "workspace",
            "memory",
            "write",
            "2026-03-08.md",
            "--file",
            memory_source.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        write_output.status.success(),
        "stderr: {}",
        stderr_text(&write_output)
    );
    assert_eq!(
        fs::read_to_string(openclaw_memory_file_path(temp.path(), "2026-03-08.md"))
            .expect("memory file should exist"),
        "Phase one notes\nWorkspace migration\n"
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "workspace", "memory", "list"],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let listed: Value =
        serde_json::from_slice(&list_output.stdout).expect("memory list should return json");
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    assert_eq!(
        listed[0].get("filename").and_then(Value::as_str),
        Some("2026-03-08.md")
    );

    let search_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "workspace",
            "memory",
            "search",
            "phase one",
        ],
    );
    assert!(
        search_output.status.success(),
        "stderr: {}",
        stderr_text(&search_output)
    );
    let searched: Value =
        serde_json::from_slice(&search_output.stdout).expect("memory search should return json");
    assert_eq!(searched.as_array().map(Vec::len), Some(1));
    assert!(
        searched[0]
            .get("snippet")
            .and_then(Value::as_str)
            .is_some_and(|snippet| snippet.contains("Phase one")),
        "search result should include content snippet"
    );

    let read_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "workspace",
            "memory",
            "read",
            "2026-03-08.md",
        ],
    );
    assert!(
        read_output.status.success(),
        "stderr: {}",
        stderr_text(&read_output)
    );
    let read: Value =
        serde_json::from_slice(&read_output.stdout).expect("memory read should return json");
    assert_eq!(
        read.get("content").and_then(Value::as_str),
        Some("Phase one notes\nWorkspace migration\n")
    );

    let delete_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "workspace",
            "memory",
            "delete",
            "2026-03-08.md",
        ],
    );
    assert!(
        delete_output.status.success(),
        "stderr: {}",
        stderr_text(&delete_output)
    );
    assert!(!openclaw_memory_file_path(temp.path(), "2026-03-08.md").exists());
}

#[test]
#[serial]
fn workspace_path_returns_explicit_workspace_and_memory_paths() {
    let temp = tempdir().expect("tempdir");

    let workspace_output = run_cli(
        temp.path(),
        &["--format", "json", "workspace", "path", "workspace"],
    );
    assert!(
        workspace_output.status.success(),
        "stderr: {}",
        stderr_text(&workspace_output)
    );
    let workspace: Value = serde_json::from_slice(&workspace_output.stdout)
        .expect("workspace path should return json");
    assert_eq!(
        workspace.get("target").and_then(Value::as_str),
        Some("workspace")
    );
    assert_eq!(
        workspace.get("path").and_then(Value::as_str),
        Some(
            temp.path()
                .join(".openclaw")
                .join("workspace")
                .to_string_lossy()
                .as_ref()
        )
    );

    let memory_output = run_cli(
        temp.path(),
        &["--format", "json", "workspace", "path", "memory"],
    );
    assert!(
        memory_output.status.success(),
        "stderr: {}",
        stderr_text(&memory_output)
    );
    let memory: Value =
        serde_json::from_slice(&memory_output.stdout).expect("memory path should return json");
    assert_eq!(memory.get("target").and_then(Value::as_str), Some("memory"));
    assert_eq!(
        memory.get("path").and_then(Value::as_str),
        Some(
            temp.path()
                .join(".openclaw")
                .join("workspace")
                .join("memory")
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn webdav_show_save_test_remote_info_upload_and_download_round_trip() {
    let temp = tempdir().expect("tempdir");
    let server = spawn_webdav_server();

    let show_before = run_cli(temp.path(), &["--format", "json", "webdav", "show"]);
    assert!(
        show_before.status.success(),
        "stderr: {}",
        stderr_text(&show_before)
    );
    let empty: Value =
        serde_json::from_slice(&show_before.stdout).expect("webdav show should return json");
    assert_eq!(
        empty.get("configured").and_then(Value::as_bool),
        Some(false)
    );

    let save_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "webdav",
            "save",
            "--base-url",
            server.base_url(),
            "--username",
            "alice",
            "--password",
            "secret",
            "--remote-root",
            "sync-root",
            "--profile",
            "stage-two",
            "--enable",
            "--auto-sync",
        ],
    );
    assert!(
        save_output.status.success(),
        "stderr: {}",
        stderr_text(&save_output)
    );
    let saved: Value =
        serde_json::from_slice(&save_output.stdout).expect("webdav save should return json");
    assert_eq!(saved.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(
        saved["settings"].get("baseUrl").and_then(Value::as_str),
        Some(server.base_url())
    );
    assert_eq!(
        saved["settings"]
            .get("passwordConfigured")
            .and_then(Value::as_bool),
        Some(true)
    );

    let preserve_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "webdav",
            "save",
            "--profile",
            "stage-three",
        ],
    );
    assert!(
        preserve_output.status.success(),
        "stderr: {}",
        stderr_text(&preserve_output)
    );
    let preserved: Value = serde_json::from_slice(&preserve_output.stdout)
        .expect("webdav preserve save should return json");
    assert_eq!(
        preserved["settings"].get("profile").and_then(Value::as_str),
        Some("stage-three")
    );
    assert_eq!(
        preserved["settings"]
            .get("passwordConfigured")
            .and_then(Value::as_bool),
        Some(true)
    );

    let test_output = run_cli(temp.path(), &["--format", "json", "webdav", "test"]);
    assert!(
        test_output.status.success(),
        "stderr: {}",
        stderr_text(&test_output)
    );
    let tested: Value =
        serde_json::from_slice(&test_output.stdout).expect("webdav test should return json");
    assert_eq!(tested.get("success").and_then(Value::as_bool), Some(true));

    let add_before = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "before-webdav",
            "--base-url",
            "https://before.example.com",
            "--api-key",
            "sk-before",
        ],
    );
    assert!(
        add_before.status.success(),
        "stderr: {}",
        stderr_text(&add_before)
    );

    let upload_output = run_cli(temp.path(), &["--format", "json", "webdav", "upload"]);
    assert!(
        upload_output.status.success(),
        "stderr: {}",
        stderr_text(&upload_output)
    );
    let uploaded: Value =
        serde_json::from_slice(&upload_output.stdout).expect("webdav upload should return json");
    assert_eq!(
        uploaded.get("status").and_then(Value::as_str),
        Some("uploaded")
    );

    let remote_info_output = run_cli(temp.path(), &["--format", "json", "webdav", "remote-info"]);
    assert!(
        remote_info_output.status.success(),
        "stderr: {}",
        stderr_text(&remote_info_output)
    );
    let remote_info: Value = serde_json::from_slice(&remote_info_output.stdout)
        .expect("webdav remote-info should return json");
    assert_eq!(
        remote_info.get("compatible").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(remote_info.get("version").and_then(Value::as_u64), Some(2));

    let add_after = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "after-webdav",
            "--base-url",
            "https://after.example.com",
            "--api-key",
            "sk-after",
        ],
    );
    assert!(
        add_after.status.success(),
        "stderr: {}",
        stderr_text(&add_after)
    );

    let download_output = run_cli(temp.path(), &["--format", "json", "webdav", "download"]);
    assert!(
        download_output.status.success(),
        "stderr: {}",
        stderr_text(&download_output)
    );
    let downloaded: Value = serde_json::from_slice(&download_output.stdout)
        .expect("webdav download should return json");
    assert_eq!(
        downloaded.get("status").and_then(Value::as_str),
        Some("downloaded")
    );

    let provider_list = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    assert!(
        provider_list.status.success(),
        "stderr: {}",
        stderr_text(&provider_list)
    );
    let providers: Value =
        serde_json::from_slice(&provider_list.stdout).expect("provider list should return json");
    assert!(
        providers.as_object().is_some_and(|items| {
            items.len() == 1
                && items.values().any(|provider| {
                    provider.get("name").and_then(Value::as_str) == Some("before-webdav")
                })
        }),
        "webdav download should restore the uploaded database snapshot"
    );
}

#[test]
#[serial]
fn sessions_list_messages_and_resume_command_round_trip() {
    let temp = tempdir().expect("tempdir");
    let session_path = seed_claude_session(temp.path(), "demo-project", "session-1.jsonl");

    let list_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "sessions",
            "list",
            "--provider",
            "claude",
            "--query",
            "demo-project",
        ],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let sessions: Value =
        serde_json::from_slice(&list_output.stdout).expect("sessions list should return json");
    assert_eq!(sessions.as_array().map(Vec::len), Some(1));
    assert_eq!(
        sessions[0].get("providerId").and_then(Value::as_str),
        Some("claude")
    );
    assert_eq!(
        sessions[0].get("resumeCommand").and_then(Value::as_str),
        Some("claude --resume session-1")
    );

    let messages_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "sessions",
            "messages",
            "--provider",
            "claude",
            "--source-path",
            session_path.to_str().expect("utf-8 source path"),
        ],
    );
    assert!(
        messages_output.status.success(),
        "stderr: {}",
        stderr_text(&messages_output)
    );
    let messages: Value = serde_json::from_slice(&messages_output.stdout)
        .expect("sessions messages should return json");
    assert_eq!(messages.as_array().map(Vec::len), Some(2));
    assert_eq!(
        messages[0].get("role").and_then(Value::as_str),
        Some("user")
    );
    assert_eq!(
        messages[0].get("content").and_then(Value::as_str),
        Some("hello from claude")
    );

    let resume_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "sessions",
            "resume-command",
            "session-1",
            "--provider",
            "claude",
        ],
    );
    assert!(
        resume_output.status.success(),
        "stderr: {}",
        stderr_text(&resume_output)
    );
    let resume: Value = serde_json::from_slice(&resume_output.stdout)
        .expect("sessions resume-command should return json");
    assert_eq!(
        resume.get("resumeCommand").and_then(Value::as_str),
        Some("claude --resume session-1")
    );
    assert_eq!(
        resume.get("sourcePath").and_then(Value::as_str),
        Some(session_path.to_str().expect("utf-8 source path"))
    );
}

#[test]
#[serial]
fn settings_structured_commands_round_trip() {
    let temp = tempdir().expect("tempdir");

    let language_set = run_cli(
        temp.path(),
        &["--format", "json", "settings", "language", "set", "zh"],
    );
    assert!(
        language_set.status.success(),
        "stderr: {}",
        stderr_text(&language_set)
    );

    let language_get = run_cli(
        temp.path(),
        &["--format", "json", "settings", "language", "get"],
    );
    assert!(
        language_get.status.success(),
        "stderr: {}",
        stderr_text(&language_get)
    );
    let language: Value =
        serde_json::from_slice(&language_get.stdout).expect("language get should return json");
    assert_eq!(language.get("language").and_then(Value::as_str), Some("zh"));

    let visible_apps_set = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "settings",
            "visible-apps",
            "set",
            "--codex",
            "false",
            "--openclaw",
            "false",
        ],
    );
    assert!(
        visible_apps_set.status.success(),
        "stderr: {}",
        stderr_text(&visible_apps_set)
    );
    let visible_apps: Value =
        serde_json::from_slice(&visible_apps_set.stdout).expect("visible apps should return json");
    assert_eq!(
        visible_apps["visibleApps"]
            .get("codex")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        visible_apps["visibleApps"]
            .get("openclaw")
            .and_then(Value::as_bool),
        Some(false)
    );

    let terminal_set = run_cli(
        temp.path(),
        &["--format", "json", "settings", "terminal", "set", "wezterm"],
    );
    assert!(
        terminal_set.status.success(),
        "stderr: {}",
        stderr_text(&terminal_set)
    );
    let terminal: Value =
        serde_json::from_slice(&terminal_set.stdout).expect("terminal set should return json");
    assert_eq!(
        terminal.get("preferredTerminal").and_then(Value::as_str),
        Some("wezterm")
    );

    let startup_set = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "settings",
            "startup",
            "set",
            "--show-in-tray",
            "false",
            "--minimize-to-tray-on-close",
            "false",
            "--launch-on-startup",
            "true",
            "--silent-startup",
            "true",
        ],
    );
    assert!(
        startup_set.status.success(),
        "stderr: {}",
        stderr_text(&startup_set)
    );
    let startup: Value =
        serde_json::from_slice(&startup_set.stdout).expect("startup set should return json");
    assert_eq!(
        startup.get("showInTray").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        startup
            .get("minimizeToTrayOnClose")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        startup.get("launchOnStartup").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        startup.get("silentStartup").and_then(Value::as_bool),
        Some(true)
    );

    let plugin_enable = run_cli(
        temp.path(),
        &["--format", "json", "settings", "plugin", "enable"],
    );
    assert!(
        plugin_enable.status.success(),
        "stderr: {}",
        stderr_text(&plugin_enable)
    );
    let plugin: Value =
        serde_json::from_slice(&plugin_enable.stdout).expect("plugin enable should return json");
    assert_eq!(
        plugin.get("enabledInSettings").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(plugin.get("applied").and_then(Value::as_bool), Some(true));
    assert!(claude_plugin_config_path(temp.path()).exists());

    let onboarding_skip = run_cli(
        temp.path(),
        &["--format", "json", "settings", "onboarding", "skip"],
    );
    assert!(
        onboarding_skip.status.success(),
        "stderr: {}",
        stderr_text(&onboarding_skip)
    );
    let onboarding: Value = serde_json::from_slice(&onboarding_skip.stdout)
        .expect("onboarding skip should return json");
    assert_eq!(
        onboarding.get("skipInSettings").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        onboarding.get("applied").and_then(Value::as_bool),
        Some(true)
    );
    assert!(fs::read_to_string(claude_mcp_path(temp.path()))
        .expect("onboarding file should exist")
        .contains("hasCompletedOnboarding"));

    let onboarding_clear = run_cli(
        temp.path(),
        &["--format", "json", "settings", "onboarding", "clear"],
    );
    assert!(
        onboarding_clear.status.success(),
        "stderr: {}",
        stderr_text(&onboarding_clear)
    );
    let onboarding_after: Value = serde_json::from_slice(&onboarding_clear.stdout)
        .expect("onboarding clear should return json");
    assert_eq!(
        onboarding_after
            .get("skipInSettings")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        onboarding_after.get("applied").and_then(Value::as_bool),
        Some(false)
    );

    let plugin_disable = run_cli(
        temp.path(),
        &["--format", "json", "settings", "plugin", "disable"],
    );
    assert!(
        plugin_disable.status.success(),
        "stderr: {}",
        stderr_text(&plugin_disable)
    );
    let plugin_after: Value =
        serde_json::from_slice(&plugin_disable.stdout).expect("plugin disable should return json");
    assert_eq!(
        plugin_after
            .get("enabledInSettings")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        plugin_after.get("applied").and_then(Value::as_bool),
        Some(false)
    );

    let terminal_clear = run_cli(
        temp.path(),
        &["--format", "json", "settings", "terminal", "clear"],
    );
    assert!(
        terminal_clear.status.success(),
        "stderr: {}",
        stderr_text(&terminal_clear)
    );
    let terminal_after: Value =
        serde_json::from_slice(&terminal_clear.stdout).expect("terminal clear should return json");
    assert!(terminal_after
        .get("preferredTerminal")
        .is_some_and(Value::is_null));

    let language_clear = run_cli(
        temp.path(),
        &["--format", "json", "settings", "language", "clear"],
    );
    assert!(
        language_clear.status.success(),
        "stderr: {}",
        stderr_text(&language_clear)
    );
    let language_after: Value =
        serde_json::from_slice(&language_clear.stdout).expect("language clear should return json");
    assert!(language_after.get("language").is_some_and(Value::is_null));
}

#[test]
#[serial]
fn backup_create_list_rename_and_delete_round_trip() {
    let temp = tempdir().expect("tempdir");

    let seed_output = run_cli(temp.path(), &["config", "set", "language", "zh"]);
    assert!(
        seed_output.status.success(),
        "stderr: {}",
        stderr_text(&seed_output)
    );

    let create_output = run_cli(temp.path(), &["--format", "json", "backup", "create"]);
    assert!(
        create_output.status.success(),
        "stderr: {}",
        stderr_text(&create_output)
    );
    let created: Value =
        serde_json::from_slice(&create_output.stdout).expect("backup create should return json");
    let filename = created
        .get("filename")
        .and_then(Value::as_str)
        .expect("backup filename");
    assert!(backup_dir(temp.path()).join(filename).exists());

    let list_output = run_cli(temp.path(), &["--format", "json", "backup", "list"]);
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let backups: Value =
        serde_json::from_slice(&list_output.stdout).expect("backup list should return json");
    assert!(
        backups.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("filename").and_then(Value::as_str) == Some(filename))
        }),
        "created backup should appear in list"
    );

    let rename_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "backup",
            "rename",
            filename,
            "phase-one-smoke",
        ],
    );
    assert!(
        rename_output.status.success(),
        "stderr: {}",
        stderr_text(&rename_output)
    );
    let renamed: Value =
        serde_json::from_slice(&rename_output.stdout).expect("backup rename should return json");
    let renamed_filename = renamed
        .get("renamedTo")
        .and_then(Value::as_str)
        .expect("renamed filename");
    assert!(backup_dir(temp.path()).join(renamed_filename).exists());

    let delete_without_yes = run_cli(temp.path(), &["backup", "delete", renamed_filename]);
    assert!(
        !delete_without_yes.status.success(),
        "delete should fail without --yes"
    );
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "backup",
            "delete",
            renamed_filename,
            "--yes",
        ],
    );
    assert!(
        delete_output.status.success(),
        "stderr: {}",
        stderr_text(&delete_output)
    );
    assert!(!backup_dir(temp.path()).join(renamed_filename).exists());
}

#[test]
#[serial]
fn backup_restore_round_trip_restores_previous_state() {
    let temp = tempdir().expect("tempdir");

    let add_before = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "before-backup",
            "--base-url",
            "https://before.example.com",
            "--api-key",
            "sk-before",
        ],
    );
    assert!(
        add_before.status.success(),
        "stderr: {}",
        stderr_text(&add_before)
    );

    let create_output = run_cli(temp.path(), &["--format", "json", "backup", "create"]);
    assert!(
        create_output.status.success(),
        "stderr: {}",
        stderr_text(&create_output)
    );
    let created: Value =
        serde_json::from_slice(&create_output.stdout).expect("backup create should return json");
    let filename = created
        .get("filename")
        .and_then(Value::as_str)
        .expect("backup filename");

    let add_after = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "after-backup",
            "--base-url",
            "https://after.example.com",
            "--api-key",
            "sk-after",
        ],
    );
    assert!(
        add_after.status.success(),
        "stderr: {}",
        stderr_text(&add_after)
    );

    let restore_without_yes = run_cli(temp.path(), &["backup", "restore", filename]);
    assert!(
        !restore_without_yes.status.success(),
        "restore should fail without --yes"
    );
    assert!(stderr_text(&restore_without_yes).contains("Re-run with --yes"));

    let restore_output = run_cli(
        temp.path(),
        &["--format", "json", "backup", "restore", filename, "--yes"],
    );
    assert!(
        restore_output.status.success(),
        "stderr: {}",
        stderr_text(&restore_output)
    );
    let restored: Value =
        serde_json::from_slice(&restore_output.stdout).expect("backup restore should return json");
    assert_eq!(
        restored.get("filename").and_then(Value::as_str),
        Some(filename)
    );
    assert!(restored
        .get("safetyBackupId")
        .and_then(Value::as_str)
        .is_some());

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let providers: Value =
        serde_json::from_slice(&list_output.stdout).expect("provider list should return json");
    let providers = providers
        .as_object()
        .expect("provider list should be an object");
    assert_eq!(
        providers.len(),
        1,
        "restored snapshot should only contain one provider"
    );
    assert_eq!(
        providers
            .get("before-backup")
            .and_then(|provider| provider.get("name"))
            .and_then(Value::as_str),
        Some("before-backup")
    );
}

#[test]
#[serial]
fn env_check_reports_file_conflicts() {
    let temp = tempdir().expect("tempdir");
    fs::write(
        zshrc_path(temp.path()),
        "export ANTHROPIC_E2E_TOKEN=sk-env\nexport OTHER_VAR=ok\n",
    )
    .expect("write zshrc");

    let check_output = run_cli(
        temp.path(),
        &["--format", "json", "env", "check", "--app", "claude"],
    );
    assert!(
        check_output.status.success(),
        "stderr: {}",
        stderr_text(&check_output)
    );

    let conflicts: Value =
        serde_json::from_slice(&check_output.stdout).expect("env check should return json");
    assert!(conflicts.as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item.get("varName").and_then(Value::as_str) == Some("ANTHROPIC_E2E_TOKEN")
                && item.get("sourceType").and_then(Value::as_str) == Some("file")
        })
    }));
}

#[test]
#[serial]
fn env_delete_and_restore_round_trip_shell_file_conflicts() {
    let temp = tempdir().expect("tempdir");
    fs::write(
        zshrc_path(temp.path()),
        "export ANTHROPIC_E2E_TOKEN=sk-env\nexport OTHER_VAR=ok\n",
    )
    .expect("write zshrc");

    let delete_without_yes = run_cli(temp.path(), &["env", "delete", "--app", "claude"]);
    assert!(
        !delete_without_yes.status.success(),
        "delete should fail without --yes"
    );
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_output = run_cli(
        temp.path(),
        &[
            "--format", "json", "env", "delete", "--app", "claude", "--yes",
        ],
    );
    assert!(
        delete_output.status.success(),
        "stderr: {}",
        stderr_text(&delete_output)
    );
    let deleted: Value =
        serde_json::from_slice(&delete_output.stdout).expect("env delete should return json");
    let backup_path = deleted
        .get("backupPath")
        .and_then(Value::as_str)
        .expect("env delete should return backup path");
    assert!(
        fs::read_to_string(zshrc_path(temp.path()))
            .expect("read zshrc after delete")
            .lines()
            .all(|line| !line.contains("ANTHROPIC_E2E_TOKEN")),
        "delete should remove the target export line from the shell file"
    );

    let restore_output = run_cli(
        temp.path(),
        &["--format", "json", "env", "restore", backup_path],
    );
    assert!(
        restore_output.status.success(),
        "stderr: {}",
        stderr_text(&restore_output)
    );
    let restored_text =
        fs::read_to_string(zshrc_path(temp.path())).expect("read zshrc after restore");
    assert!(
        restored_text.contains("export ANTHROPIC_E2E_TOKEN=sk-env"),
        "restore should put the target export line back into the shell file"
    );
}

#[test]
#[serial]
fn openclaw_env_tools_default_model_and_catalog_round_trip() {
    let temp = tempdir().expect("tempdir");
    let model_catalog_file = temp.path().join("model-catalog.json");
    fs::write(
        &model_catalog_file,
        r#"{
  "demo/gpt-5": {
    "alias": "GPT-5",
    "contextWindow": 200000
  }
}"#,
    )
    .expect("write model catalog json");

    let set_env = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "openclaw",
            "env",
            "set",
            "--value",
            r#"{"OPENAI_API_KEY":"sk-openclaw","OPENCLAW_FEATURE":"enabled"}"#,
        ],
    );
    assert!(
        set_env.status.success(),
        "stderr: {}",
        stderr_text(&set_env)
    );

    let set_tools = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "openclaw",
            "tools",
            "set",
            "--value",
            r#"{"profile":"strict","allow":["read:*"],"deny":["write:*"]}"#,
        ],
    );
    assert!(
        set_tools.status.success(),
        "stderr: {}",
        stderr_text(&set_tools)
    );

    let set_default_model = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "openclaw",
            "default-model",
            "set",
            "--value",
            r#"{"primary":"demo/gpt-5","fallbacks":["demo/gpt-4.1"]}"#,
        ],
    );
    assert!(
        set_default_model.status.success(),
        "stderr: {}",
        stderr_text(&set_default_model)
    );

    let set_model_catalog = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "openclaw",
            "model-catalog",
            "set",
            "--file",
            model_catalog_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        set_model_catalog.status.success(),
        "stderr: {}",
        stderr_text(&set_model_catalog)
    );

    let set_agents_defaults = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "openclaw",
            "agents-defaults",
            "set",
            "--value",
            r#"{
  "model": {
    "primary": "demo/gpt-5",
    "fallbacks": ["demo/gpt-4.1"]
  },
  "models": {
    "demo/gpt-5": {
      "alias": "GPT-5"
    }
  }
}"#,
        ],
    );
    assert!(
        set_agents_defaults.status.success(),
        "stderr: {}",
        stderr_text(&set_agents_defaults)
    );

    let env_output = run_cli(temp.path(), &["--format", "json", "openclaw", "env", "get"]);
    let env_json: Value =
        serde_json::from_slice(&env_output.stdout).expect("openclaw env get should return json");
    assert_eq!(
        env_json.get("OPENAI_API_KEY").and_then(Value::as_str),
        Some("sk-openclaw")
    );

    let tools_output = run_cli(
        temp.path(),
        &["--format", "json", "openclaw", "tools", "get"],
    );
    let tools_json: Value = serde_json::from_slice(&tools_output.stdout)
        .expect("openclaw tools get should return json");
    assert_eq!(
        tools_json.get("profile").and_then(Value::as_str),
        Some("strict")
    );

    let default_model_output = run_cli(
        temp.path(),
        &["--format", "json", "openclaw", "default-model", "get"],
    );
    let default_model_json: Value = serde_json::from_slice(&default_model_output.stdout)
        .expect("openclaw default-model get should return json");
    assert_eq!(
        default_model_json.get("primary").and_then(Value::as_str),
        Some("demo/gpt-5")
    );

    let model_catalog_output = run_cli(
        temp.path(),
        &["--format", "json", "openclaw", "model-catalog", "get"],
    );
    let model_catalog_json: Value = serde_json::from_slice(&model_catalog_output.stdout)
        .expect("openclaw model-catalog get should return json");
    assert_eq!(
        model_catalog_json
            .get("demo/gpt-5")
            .and_then(|entry| entry.get("alias"))
            .and_then(Value::as_str),
        Some("GPT-5")
    );

    let agents_defaults_output = run_cli(
        temp.path(),
        &["--format", "json", "openclaw", "agents-defaults", "get"],
    );
    let agents_defaults_json: Value = serde_json::from_slice(&agents_defaults_output.stdout)
        .expect("openclaw agents-defaults get should return json");
    assert_eq!(
        agents_defaults_json
            .get("model")
            .and_then(|model| model.get("primary"))
            .and_then(Value::as_str),
        Some("demo/gpt-5")
    );

    let live_config = fs::read_to_string(openclaw_config_path(temp.path()))
        .expect("openclaw config file should exist");
    assert!(live_config.contains("\"env\""));
    assert!(live_config.contains("\"tools\""));
    assert!(live_config.contains("\"agents\""));
}

#[test]
#[serial]
fn omo_and_omo_slim_read_import_current_and_disable_round_trip() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".config").join("opencode")).expect("create opencode dir");
    fs::write(
        omo_local_path(temp.path()),
        r#"{
  // comment
  "agents": { "writer": { "prompt": "hi" } },
  "categories": { "default": ["writer"] },
  "theme": "default"
}"#,
    )
    .expect("write omo jsonc");
    fs::write(
        omo_slim_local_path(temp.path()),
        r#"{
  "agents": { "reviewer": { "prompt": "ship it" } },
  "theme": "slim"
}"#,
    )
    .expect("write omo slim jsonc");

    let read_omo = run_cli(temp.path(), &["--format", "json", "omo", "read-local"]);
    assert!(
        read_omo.status.success(),
        "stderr: {}",
        stderr_text(&read_omo)
    );
    let read_omo_json: Value =
        serde_json::from_slice(&read_omo.stdout).expect("omo read-local should return json");
    assert!(read_omo_json.get("agents").is_some());
    assert!(read_omo_json.get("categories").is_some());

    let import_omo = run_cli(temp.path(), &["--format", "json", "omo", "import-local"]);
    assert!(
        import_omo.status.success(),
        "stderr: {}",
        stderr_text(&import_omo)
    );
    let imported_omo: Value =
        serde_json::from_slice(&import_omo.stdout).expect("omo import-local should return json");
    let imported_omo_id = imported_omo
        .get("id")
        .and_then(Value::as_str)
        .expect("omo import-local should return provider id");

    let current_omo = run_cli(temp.path(), &["--format", "json", "omo", "current"]);
    let current_omo_json: Value =
        serde_json::from_slice(&current_omo.stdout).expect("omo current should return json");
    assert_eq!(
        current_omo_json.get("providerId").and_then(Value::as_str),
        Some(imported_omo_id)
    );

    let disable_omo = run_cli(temp.path(), &["--format", "json", "omo", "disable-current"]);
    assert!(
        disable_omo.status.success(),
        "stderr: {}",
        stderr_text(&disable_omo)
    );
    assert!(!omo_local_path(temp.path()).exists());

    let read_omo_slim = run_cli(temp.path(), &["--format", "json", "omo-slim", "read-local"]);
    assert!(
        read_omo_slim.status.success(),
        "stderr: {}",
        stderr_text(&read_omo_slim)
    );
    let read_omo_slim_json: Value = serde_json::from_slice(&read_omo_slim.stdout)
        .expect("omo-slim read-local should return json");
    assert!(read_omo_slim_json.get("agents").is_some());
    assert!(
        read_omo_slim_json.get("categories").is_none()
            || read_omo_slim_json
                .get("categories")
                .is_some_and(Value::is_null)
    );

    let import_omo_slim = run_cli(
        temp.path(),
        &["--format", "json", "omo-slim", "import-local"],
    );
    assert!(
        import_omo_slim.status.success(),
        "stderr: {}",
        stderr_text(&import_omo_slim)
    );
    let imported_omo_slim: Value = serde_json::from_slice(&import_omo_slim.stdout)
        .expect("omo-slim import-local should return json");
    let imported_omo_slim_id = imported_omo_slim
        .get("id")
        .and_then(Value::as_str)
        .expect("omo-slim import-local should return provider id");

    let current_omo_slim = run_cli(temp.path(), &["--format", "json", "omo-slim", "current"]);
    let current_omo_slim_json: Value = serde_json::from_slice(&current_omo_slim.stdout)
        .expect("omo-slim current should return json");
    assert_eq!(
        current_omo_slim_json
            .get("providerId")
            .and_then(Value::as_str),
        Some(imported_omo_slim_id)
    );

    let disable_omo_slim = run_cli(
        temp.path(),
        &["--format", "json", "omo-slim", "disable-current"],
    );
    assert!(
        disable_omo_slim.status.success(),
        "stderr: {}",
        stderr_text(&disable_omo_slim)
    );
    assert!(!omo_slim_local_path(temp.path()).exists());
}

#[test]
#[serial]
fn prompt_lifecycle_round_trips_through_cli_and_live_file() {
    let temp = tempdir().expect("tempdir");
    let review_file = temp.path().join("review.txt");
    let draft_file = temp.path().join("draft.txt");
    fs::write(&review_file, "Review the diff carefully.\n").expect("write review prompt");
    fs::write(&draft_file, "Draft prompt.\n").expect("write draft prompt");

    let add_review = run_cli(
        temp.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "review",
            "--file",
            review_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_review.status.success(),
        "stderr: {}",
        stderr_text(&add_review)
    );

    let add_draft = run_cli(
        temp.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "draft",
            "--file",
            draft_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_draft.status.success(),
        "stderr: {}",
        stderr_text(&add_draft)
    );

    let enable_output = run_cli(
        temp.path(),
        &["prompt", "enable", "review", "--app", "claude"],
    );
    assert!(
        enable_output.status.success(),
        "stderr: {}",
        stderr_text(&enable_output)
    );
    let live_prompt = fs::read_to_string(claude_prompt_path(temp.path())).expect("live prompt");
    assert_eq!(live_prompt, "Review the diff carefully.\n");

    let delete_without_yes = run_cli(
        temp.path(),
        &["prompt", "delete", "draft", "--app", "claude"],
    );
    assert!(!delete_without_yes.status.success(), "delete should fail");
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_with_yes = run_cli(
        temp.path(),
        &["prompt", "delete", "draft", "--app", "claude", "--yes"],
    );
    assert!(
        delete_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&delete_with_yes)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "prompt", "list", "--app", "claude"],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let prompts: Value =
        serde_json::from_slice(&list_output.stdout).expect("prompt list should return json");
    assert!(prompts.get("review").is_some());
    assert!(prompts.get("draft").is_none());
}

#[test]
#[serial]
fn prompt_import_reads_live_file_on_first_launch() {
    let temp = tempdir().expect("tempdir");
    let live_path = claude_prompt_path(temp.path());
    fs::create_dir_all(live_path.parent().expect("claude dir")).expect("claude dir");
    fs::write(&live_path, "Seed live prompt.\n").expect("write live prompt");

    let import_output = run_cli(temp.path(), &["prompt", "import", "--app", "claude"]);
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "prompt", "list", "--app", "claude"],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let prompts: Value =
        serde_json::from_slice(&list_output.stdout).expect("prompt list should return json");
    let prompt = prompts
        .as_object()
        .and_then(|items| items.values().next())
        .expect("imported prompt should exist");
    assert_eq!(
        prompt.get("content").and_then(Value::as_str),
        Some("Seed live prompt.\n")
    );
    assert_eq!(prompt.get("enabled").and_then(Value::as_bool), Some(true));
}

#[test]
#[serial]
fn prompt_current_live_file_content_reads_live_prompt_file() {
    let temp = tempdir().expect("tempdir");
    let live_path = claude_prompt_path(temp.path());
    fs::create_dir_all(live_path.parent().expect("claude dir")).expect("claude dir");
    fs::write(&live_path, "Current live prompt.\n").expect("write live prompt");

    let output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "prompt",
            "current-live-file-content",
            "--app",
            "claude",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));

    let value: Value =
        serde_json::from_slice(&output.stdout).expect("current live prompt should return json");
    assert_eq!(value.get("app").and_then(Value::as_str), Some("claude"));
    assert_eq!(
        value.get("content").and_then(Value::as_str),
        Some("Current live prompt.\n")
    );
}

#[test]
#[serial]
fn mcp_add_from_json_edit_and_delete_round_trip_with_live_sync() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".claude")).expect("claude dir");
    fs::create_dir_all(temp.path().join(".codex")).expect("codex dir");
    let mcp_file = temp.path().join("mcp.json");
    fs::write(
        &mcp_file,
        r#"{"type":"stdio","command":"npx","args":["foo","bar"]}"#,
    )
    .expect("write mcp json");

    let add_output = run_cli(
        temp.path(),
        &[
            "--quiet",
            "mcp",
            "add",
            "--id",
            "demo",
            "--from-json",
            mcp_file.to_str().expect("utf-8 path"),
            "--apps",
            "claude,codex",
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );
    assert!(stdout_text(&add_output).trim().is_empty());

    let claude_mcp = fs::read_to_string(claude_mcp_path(temp.path())).expect("claude mcp");
    assert!(claude_mcp.contains("\"demo\""));
    let codex_config = fs::read_to_string(codex_config_path(temp.path())).expect("codex config");
    assert!(codex_config.contains("demo"));

    let edit_output = run_cli(
        temp.path(),
        &["mcp", "edit", "demo", "--disable-app", "codex"],
    );
    assert!(
        edit_output.status.success(),
        "stderr: {}",
        stderr_text(&edit_output)
    );

    let list_output = run_cli(temp.path(), &["--format", "json", "mcp", "list"]);
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("mcp list should return json");
    let apps = value
        .get("demo")
        .and_then(|item| item.get("apps"))
        .expect("apps should exist");
    assert_eq!(apps.get("claude").and_then(Value::as_bool), Some(true));
    assert_eq!(apps.get("codex").and_then(Value::as_bool), Some(false));

    let codex_after_edit =
        fs::read_to_string(codex_config_path(temp.path())).expect("codex config");
    assert!(!codex_after_edit.contains("demo"));

    let delete_without_yes = run_cli(temp.path(), &["mcp", "delete", "demo"]);
    assert!(!delete_without_yes.status.success(), "delete should fail");
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_with_yes = run_cli(temp.path(), &["mcp", "delete", "demo", "--yes"]);
    assert!(
        delete_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&delete_with_yes)
    );

    let list_after_delete = run_cli(temp.path(), &["--format", "json", "mcp", "list"]);
    let servers: Value =
        serde_json::from_slice(&list_after_delete.stdout).expect("mcp list should return json");
    assert!(servers.get("demo").is_none());
    let claude_mcp_after_delete =
        fs::read_to_string(claude_mcp_path(temp.path())).expect("claude mcp");
    assert!(!claude_mcp_after_delete.contains("\"demo\""));
}

#[test]
#[serial]
fn mcp_import_reads_existing_live_configs() {
    let temp = tempdir().expect("tempdir");
    fs::write(
        claude_mcp_path(temp.path()),
        r#"{"mcpServers":{"from-claude":{"type":"stdio","command":"npx","args":["claude"]}}}"#,
    )
    .expect("write claude mcp");
    fs::create_dir_all(temp.path().join(".codex")).expect("codex dir");
    fs::write(
        codex_config_path(temp.path()),
        r#"[mcp_servers.from_codex]
type = "stdio"
command = "npx"
args = ["codex"]
"#,
    )
    .expect("write codex config");

    let import_output = run_cli(temp.path(), &["mcp", "import"]);
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let list_output = run_cli(temp.path(), &["--format", "json", "mcp", "list"]);
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("mcp list should return json");
    assert_eq!(
        value
            .get("from-claude")
            .and_then(|item| item.get("apps"))
            .and_then(|apps| apps.get("claude"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .get("from_codex")
            .and_then(|item| item.get("apps"))
            .and_then(|apps| apps.get("codex"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
#[serial]
fn mcp_validate_docs_link_and_toggle_output_are_structured() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".claude")).expect("claude dir");
    let mcp_file = temp.path().join("mcp-full.json");
    fs::write(
        &mcp_file,
        r#"{
  "id": "docs-mcp",
  "name": "docs-mcp",
  "server": { "type": "stdio", "command": "npx", "args": ["docs"] },
  "apps": { "claude": true, "codex": false, "gemini": false, "opencode": false, "openclaw": false },
  "homepage": "https://example.com/mcp",
  "docs": "https://example.com/mcp/docs",
  "tags": ["fixture"]
}"#,
    )
    .expect("write mcp full json");

    let add_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "mcp",
            "add",
            "--from-json",
            mcp_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );
    let added: Value =
        serde_json::from_slice(&add_output.stdout).expect("mcp add should return json");
    assert_eq!(added.get("id").and_then(Value::as_str), Some("docs-mcp"));

    let validate_output = run_cli(
        temp.path(),
        &["--format", "json", "mcp", "validate", "docs-mcp"],
    );
    assert!(
        validate_output.status.success(),
        "stderr: {}",
        stderr_text(&validate_output)
    );
    let validated: Value =
        serde_json::from_slice(&validate_output.stdout).expect("mcp validate should return json");
    assert_eq!(validated.get("valid").and_then(Value::as_bool), Some(true));

    let docs_output = run_cli(
        temp.path(),
        &["--format", "json", "mcp", "docs-link", "docs-mcp"],
    );
    assert!(
        docs_output.status.success(),
        "stderr: {}",
        stderr_text(&docs_output)
    );
    let docs: Value =
        serde_json::from_slice(&docs_output.stdout).expect("mcp docs-link should return json");
    assert_eq!(
        docs.get("homepage").and_then(Value::as_str),
        Some("https://example.com/mcp")
    );
    assert_eq!(
        docs.get("docs").and_then(Value::as_str),
        Some("https://example.com/mcp/docs")
    );

    let enable_output = run_cli(
        temp.path(),
        &[
            "--format", "json", "mcp", "enable", "docs-mcp", "--app", "codex",
        ],
    );
    assert!(
        enable_output.status.success(),
        "stderr: {}",
        stderr_text(&enable_output)
    );
    let enabled: Value =
        serde_json::from_slice(&enable_output.stdout).expect("mcp enable should return json");
    assert_eq!(
        enabled
            .get("apps")
            .and_then(|apps| apps.get("codex"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
#[serial]
fn provider_add_from_json_switch_and_delete_round_trip() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://old.example","ANTHROPIC_AUTH_TOKEN":"sk-old"}}"#,
    )
    .expect("write provider json");

    let import_output = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Imported Router",
            "--base-url",
            "https://new.example",
            "--api-key",
            "sk-new",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let show_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "show",
            "imported-router",
            "--app",
            "claude",
        ],
    );
    assert!(
        show_output.status.success(),
        "stderr: {}",
        stderr_text(&show_output)
    );
    let provider: Value =
        serde_json::from_slice(&show_output.stdout).expect("provider show should return json");
    assert_eq!(
        provider
            .get("settingsConfig")
            .and_then(|config| config.get("env"))
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(Value::as_str),
        Some("https://new.example")
    );
    assert_eq!(
        provider
            .get("settingsConfig")
            .and_then(|config| config.get("env"))
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(Value::as_str),
        Some("sk-new")
    );

    let switch_output = run_cli(
        temp.path(),
        &["provider", "switch", "imported-router", "--app", "claude"],
    );
    assert!(
        switch_output.status.success(),
        "stderr: {}",
        stderr_text(&switch_output)
    );
    let live_settings =
        fs::read_to_string(claude_settings_path(temp.path())).expect("claude settings");
    assert!(live_settings.contains("https://new.example"));
    assert!(live_settings.contains("sk-new"));

    let add_delete_target = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Delete Me",
            "--base-url",
            "https://delete.example",
            "--api-key",
            "sk-delete",
        ],
    );
    assert!(
        add_delete_target.status.success(),
        "stderr: {}",
        stderr_text(&add_delete_target)
    );

    let delete_without_yes = run_cli(
        temp.path(),
        &["provider", "delete", "delete-me", "--app", "claude"],
    );
    assert!(!delete_without_yes.status.success(), "delete should fail");
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_with_yes = run_cli(
        temp.path(),
        &[
            "provider",
            "delete",
            "delete-me",
            "--app",
            "claude",
            "--yes",
        ],
    );
    assert!(
        delete_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&delete_with_yes)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    let providers: Value =
        serde_json::from_slice(&list_output.stdout).expect("provider list should return json");
    assert!(providers.get("imported-router").is_some());
    assert!(providers.get("delete-me").is_none());
}

#[test]
#[serial]
fn provider_usage_without_script_falls_back_to_local_summary() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://usage.example","ANTHROPIC_AUTH_TOKEN":"sk-usage"}}"#,
    )
    .expect("write provider json");

    let add_output = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Local Usage Provider",
            "--base-url",
            "https://usage.example",
            "--api-key",
            "sk-usage",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );

    insert_usage_log(
        temp.path(),
        "req-provider-usage",
        "claude",
        "local-usage-provider",
        "claude-haiku",
        12,
        8,
        "0.0015",
        chrono::Utc::now().timestamp(),
    );

    let usage_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "usage",
            "local-usage-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        usage_output.status.success(),
        "stderr: {}",
        stderr_text(&usage_output)
    );
    assert!(stderr_text(&usage_output).contains("local proxy usage"));

    let value: Value =
        serde_json::from_slice(&usage_output.stdout).expect("provider usage should return json");
    assert_eq!(value.get("totalRequests").and_then(Value::as_u64), Some(1));
    assert_eq!(value.get("totalTokens").and_then(Value::as_u64), Some(20));
    assert_eq!(
        value
            .get("requestsByModel")
            .and_then(|items| items.get("claude-haiku"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[test]
#[serial]
fn provider_duplicate_sort_order_and_read_live_round_trip() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://dup.example","ANTHROPIC_AUTH_TOKEN":"sk-dup"}}"#,
    )
    .expect("write provider json");

    let add_output = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Primary Provider",
            "--base-url",
            "https://dup.example",
            "--api-key",
            "sk-dup",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );

    let duplicate_output = run_cli(
        temp.path(),
        &[
            "provider",
            "duplicate",
            "primary-provider",
            "--app",
            "claude",
            "--name",
            "Primary Provider Backup",
        ],
    );
    assert!(
        duplicate_output.status.success(),
        "stderr: {}",
        stderr_text(&duplicate_output)
    );

    let sort_output = run_cli(
        temp.path(),
        &[
            "provider",
            "sort-order",
            "primary-provider-backup",
            "--app",
            "claude",
            "--index",
            "7",
        ],
    );
    assert!(
        sort_output.status.success(),
        "stderr: {}",
        stderr_text(&sort_output)
    );

    let providers_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    assert!(
        providers_output.status.success(),
        "stderr: {}",
        stderr_text(&providers_output)
    );
    let providers: Value =
        serde_json::from_slice(&providers_output.stdout).expect("provider list should return json");
    assert!(providers.get("primary-provider").is_some());
    assert_eq!(
        providers
            .get("primary-provider-backup")
            .and_then(|item| item.get("sortIndex"))
            .and_then(Value::as_u64),
        Some(7)
    );

    let switch_output = run_cli(
        temp.path(),
        &[
            "provider",
            "switch",
            "primary-provider-backup",
            "--app",
            "claude",
        ],
    );
    assert!(
        switch_output.status.success(),
        "stderr: {}",
        stderr_text(&switch_output)
    );

    let read_live_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "read-live",
            "--app",
            "claude",
        ],
    );
    assert!(
        read_live_output.status.success(),
        "stderr: {}",
        stderr_text(&read_live_output)
    );
    let live: Value =
        serde_json::from_slice(&read_live_output.stdout).expect("read-live should return json");
    assert_eq!(
        live.get("env")
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(Value::as_str),
        Some("https://dup.example")
    );
}

#[test]
#[serial]
fn provider_import_live_and_remove_from_live_round_trip() {
    let temp = tempdir().expect("tempdir");

    let claude_live = claude_settings_path(temp.path());
    fs::create_dir_all(claude_live.parent().expect("parent")).expect("claude dir");
    fs::write(
        &claude_live,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://live.example","ANTHROPIC_AUTH_TOKEN":"sk-live"}}"#,
    )
    .expect("write claude live config");

    let import_claude = run_cli(temp.path(), &["provider", "import-live", "--app", "claude"]);
    assert!(
        import_claude.status.success(),
        "stderr: {}",
        stderr_text(&import_claude)
    );

    let show_default = run_cli(
        temp.path(),
        &[
            "--format", "json", "provider", "show", "default", "--app", "claude",
        ],
    );
    assert!(
        show_default.status.success(),
        "stderr: {}",
        stderr_text(&show_default)
    );
    let default_provider: Value =
        serde_json::from_slice(&show_default.stdout).expect("provider show should return json");
    assert_eq!(
        default_provider
            .get("settingsConfig")
            .and_then(|config| config.get("env"))
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(Value::as_str),
        Some("sk-live")
    );

    let opencode_file = temp.path().join("opencode-provider.json");
    fs::write(
        &opencode_file,
        r#"{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://open.live","apiKey":"sk-open"},"models":{}}"#,
    )
    .expect("write opencode provider");

    let add_opencode = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "opencode",
            "--name",
            "Open Live",
            "--from-json",
            opencode_file.to_str().expect("utf-8 path"),
            "--base-url",
            "https://open.live",
            "--api-key",
            "sk-open",
        ],
    );
    assert!(
        add_opencode.status.success(),
        "stderr: {}",
        stderr_text(&add_opencode)
    );

    let opencode_live_before: Value = serde_json::from_str(
        &fs::read_to_string(opencode_config_path(temp.path())).expect("opencode config"),
    )
    .expect("opencode json");
    assert!(opencode_live_before
        .get("provider")
        .and_then(|providers| providers.get("open-live"))
        .is_some());

    let remove_live = run_cli(
        temp.path(),
        &[
            "provider",
            "remove-from-live",
            "open-live",
            "--app",
            "opencode",
        ],
    );
    assert!(
        remove_live.status.success(),
        "stderr: {}",
        stderr_text(&remove_live)
    );

    let providers_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "opencode"],
    );
    let providers: Value =
        serde_json::from_slice(&providers_output.stdout).expect("provider list should return json");
    assert!(
        providers.get("open-live").is_some(),
        "db record should remain"
    );

    let opencode_live_after: Value = serde_json::from_str(
        &fs::read_to_string(opencode_config_path(temp.path())).expect("opencode config"),
    )
    .expect("opencode json");
    assert!(
        opencode_live_after
            .get("provider")
            .and_then(|providers| providers.get("open-live"))
            .is_none(),
        "live config should no longer contain removed provider"
    );
}

#[test]
#[serial]
fn provider_sort_order_and_remove_from_live_require_existing_provider() {
    let temp = tempdir().expect("tempdir");

    let sort_output = run_cli(
        temp.path(),
        &[
            "provider",
            "sort-order",
            "missing-provider",
            "--app",
            "claude",
            "--index",
            "1",
        ],
    );
    assert!(!sort_output.status.success(), "sort-order should fail");
    assert!(stderr_text(&sort_output).contains("Provider not found"));

    let remove_output = run_cli(
        temp.path(),
        &[
            "provider",
            "remove-from-live",
            "missing-provider",
            "--app",
            "opencode",
        ],
    );
    assert!(
        !remove_output.status.success(),
        "remove-from-live should fail"
    );
    assert!(stderr_text(&remove_output).contains("Provider not found"));
}

#[test]
#[serial]
fn provider_endpoint_lifecycle_and_speedtest_round_trip() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:9/v1","ANTHROPIC_AUTH_TOKEN":"sk-endpoint"}}"#,
    )
    .expect("write provider json");

    let add_provider = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Endpoint Provider",
            "--base-url",
            "http://127.0.0.1:9/v1",
            "--api-key",
            "sk-endpoint",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_provider.status.success(),
        "stderr: {}",
        stderr_text(&add_provider)
    );

    let add_invalid = run_cli(
        temp.path(),
        &[
            "provider",
            "endpoint",
            "add",
            "endpoint-provider",
            "--app",
            "claude",
            "--url",
            "not-a-url",
        ],
    );
    assert!(
        add_invalid.status.success(),
        "stderr: {}",
        stderr_text(&add_invalid)
    );

    let add_secondary = run_cli(
        temp.path(),
        &[
            "provider",
            "endpoint",
            "add",
            "endpoint-provider",
            "--app",
            "claude",
            "--url",
            "http://127.0.0.1:9/secondary/",
        ],
    );
    assert!(
        add_secondary.status.success(),
        "stderr: {}",
        stderr_text(&add_secondary)
    );

    let mark_used = run_cli(
        temp.path(),
        &[
            "provider",
            "endpoint",
            "mark-used",
            "endpoint-provider",
            "--app",
            "claude",
            "--url",
            "http://127.0.0.1:9/secondary",
        ],
    );
    assert!(
        mark_used.status.success(),
        "stderr: {}",
        stderr_text(&mark_used)
    );

    let list_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "endpoint",
            "list",
            "endpoint-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let endpoints: Value =
        serde_json::from_slice(&list_output.stdout).expect("endpoint list should return json");
    assert_eq!(endpoints.as_array().map(Vec::len), Some(2));
    assert!(endpoints
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("url") == Some(&Value::String("http://127.0.0.1:9/secondary".to_string()))
            })
        })
        .and_then(|item| item.get("lastUsed"))
        .and_then(Value::as_i64)
        .is_some());

    let speedtest_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "endpoint",
            "speedtest",
            "endpoint-provider",
            "--app",
            "claude",
            "--timeout",
            "2",
        ],
    );
    assert!(
        speedtest_output.status.success(),
        "stderr: {}",
        stderr_text(&speedtest_output)
    );
    let speedtest: Value =
        serde_json::from_slice(&speedtest_output.stdout).expect("speedtest should return json");
    assert!(speedtest
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item.get("url")
            == Some(&Value::String("not-a-url".to_string()))
            && item
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|text| text.starts_with("URL 无效")))));

    let remove_endpoint = run_cli(
        temp.path(),
        &[
            "provider",
            "endpoint",
            "remove",
            "endpoint-provider",
            "--app",
            "claude",
            "--url",
            "not-a-url",
        ],
    );
    assert!(
        remove_endpoint.status.success(),
        "stderr: {}",
        stderr_text(&remove_endpoint)
    );

    let list_after_remove = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "endpoint",
            "list",
            "endpoint-provider",
            "--app",
            "claude",
        ],
    );
    let endpoints_after_remove: Value = serde_json::from_slice(&list_after_remove.stdout)
        .expect("endpoint list should return json");
    assert_eq!(endpoints_after_remove.as_array().map(Vec::len), Some(1));
    assert!(endpoints_after_remove.as_array().is_some_and(|items| items
        .iter()
        .all(|item| item.get("url") != Some(&Value::String("not-a-url".to_string())))));
}

#[test]
#[serial]
fn provider_common_config_snippet_extract_get_set_and_clear_round_trip() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://extract.example","ANTHROPIC_AUTH_TOKEN":"sk-extract","HTTPS_PROXY":"http://127.0.0.1:8080"},"permissions":{"allow":["Bash"]}}"#,
    )
    .expect("write provider json");

    let add_provider = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Snippet Provider",
            "--base-url",
            "https://extract.example",
            "--api-key",
            "sk-extract",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_provider.status.success(),
        "stderr: {}",
        stderr_text(&add_provider)
    );

    let switch_output = run_cli(
        temp.path(),
        &["provider", "switch", "snippet-provider", "--app", "claude"],
    );
    assert!(
        switch_output.status.success(),
        "stderr: {}",
        stderr_text(&switch_output)
    );

    let extract_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "common-config-snippet",
            "extract",
            "--app",
            "claude",
        ],
    );
    assert!(
        extract_output.status.success(),
        "stderr: {}",
        stderr_text(&extract_output)
    );
    let extracted: Value =
        serde_json::from_slice(&extract_output.stdout).expect("extract should return json");
    let snippet = extracted
        .get("snippet")
        .and_then(Value::as_str)
        .expect("snippet should exist");
    let snippet_json: Value =
        serde_json::from_str(snippet).expect("snippet should be valid json content");
    assert_eq!(
        snippet_json
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_BASE_URL")),
        None
    );
    assert_eq!(
        snippet_json
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN")),
        None
    );
    assert_eq!(
        snippet_json
            .get("env")
            .and_then(|env| env.get("HTTPS_PROXY"))
            .and_then(Value::as_str),
        Some("http://127.0.0.1:8080")
    );
    assert_eq!(
        snippet_json
            .get("permissions")
            .and_then(|value| value.get("allow"))
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_str),
        Some("Bash")
    );

    let snippet_file = temp.path().join("snippet.json");
    let saved_snippet =
        r#"{"env":{"HTTPS_PROXY":"http://10.0.0.2:8080"},"permissions":{"allow":["Read"]}}"#;
    fs::write(&snippet_file, saved_snippet).expect("write snippet file");

    let set_output = run_cli(
        temp.path(),
        &[
            "provider",
            "common-config-snippet",
            "set",
            "--app",
            "claude",
            "--file",
            snippet_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        set_output.status.success(),
        "stderr: {}",
        stderr_text(&set_output)
    );

    let get_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "common-config-snippet",
            "get",
            "--app",
            "claude",
        ],
    );
    assert!(
        get_output.status.success(),
        "stderr: {}",
        stderr_text(&get_output)
    );
    let saved: Value = serde_json::from_slice(&get_output.stdout).expect("get should return json");
    assert_eq!(
        saved.get("snippet").and_then(Value::as_str),
        Some(saved_snippet)
    );

    let invalid_output = run_cli(
        temp.path(),
        &[
            "provider",
            "common-config-snippet",
            "set",
            "--app",
            "claude",
            "--value",
            "{not-json}",
        ],
    );
    assert!(!invalid_output.status.success(), "invalid json should fail");
    assert!(stderr_text(&invalid_output).contains("Invalid claude common config snippet JSON"));

    let clear_output = run_cli(
        temp.path(),
        &[
            "provider",
            "common-config-snippet",
            "set",
            "--app",
            "claude",
            "--clear",
        ],
    );
    assert!(
        clear_output.status.success(),
        "stderr: {}",
        stderr_text(&clear_output)
    );

    let get_after_clear = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "common-config-snippet",
            "get",
            "--app",
            "claude",
        ],
    );
    let cleared: Value =
        serde_json::from_slice(&get_after_clear.stdout).expect("get should return json");
    assert!(cleared.get("snippet").is_some_and(Value::is_null));
}

#[test]
#[serial]
fn provider_common_config_snippet_allows_codex_toml() {
    let temp = tempdir().expect("tempdir");
    let snippet = "approval_policy = \"on-request\"\n";

    let set_output = run_cli(
        temp.path(),
        &[
            "provider",
            "common-config-snippet",
            "set",
            "--app",
            "codex",
            "--value",
            snippet,
        ],
    );
    assert!(
        set_output.status.success(),
        "stderr: {}",
        stderr_text(&set_output)
    );

    let get_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "common-config-snippet",
            "get",
            "--app",
            "codex",
        ],
    );
    assert!(
        get_output.status.success(),
        "stderr: {}",
        stderr_text(&get_output)
    );
    let saved: Value = serde_json::from_slice(&get_output.stdout).expect("get should return json");
    assert_eq!(saved.get("snippet").and_then(Value::as_str), Some(snippet));
}

#[test]
#[serial]
fn provider_usage_script_save_show_test_query_and_clear_round_trip() {
    let temp = tempdir().expect("tempdir");
    let provider_file = temp.path().join("provider.json");
    fs::write(
        &provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://unused.example","ANTHROPIC_AUTH_TOKEN":"sk-usage-script"}}"#,
    )
    .expect("write provider json");

    let add_provider = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Usage Script Provider",
            "--base-url",
            "https://unused.example",
            "--api-key",
            "sk-usage-script",
            "--from-json",
            provider_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        add_provider.status.success(),
        "stderr: {}",
        stderr_text(&add_provider)
    );

    let server_url = spawn_json_server(r#"{"balance":42}"#.to_string(), 2);
    let script_file = temp.path().join("usage-script.json");
    let script = serde_json::json!({
        "enabled": true,
        "language": "javascript",
        "code": format!(
            "({{ request: {{ url: \"{server_url}/usage\", method: \"GET\", headers: {{}} }}, extractor: function(response) {{ return {{ isValid: true, remaining: response.balance, unit: \"USD\" }}; }} }})"
        ),
        "timeout": 5,
        "templateType": "custom",
    });
    fs::write(
        &script_file,
        serde_json::to_string_pretty(&script).expect("serialize usage script"),
    )
    .expect("write usage script");

    let save_output = run_cli(
        temp.path(),
        &[
            "provider",
            "usage-script",
            "save",
            "usage-script-provider",
            "--app",
            "claude",
            "--file",
            script_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        save_output.status.success(),
        "stderr: {}",
        stderr_text(&save_output)
    );

    let show_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "usage-script",
            "show",
            "usage-script-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        show_output.status.success(),
        "stderr: {}",
        stderr_text(&show_output)
    );
    let shown: Value =
        serde_json::from_slice(&show_output.stdout).expect("usage-script show should return json");
    assert_eq!(
        shown
            .get("usageScript")
            .and_then(|value| value.get("enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let test_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "usage-script",
            "test",
            "usage-script-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        test_output.status.success(),
        "stderr: {}",
        stderr_text(&test_output)
    );
    let tested: Value =
        serde_json::from_slice(&test_output.stdout).expect("usage-script test should return json");
    assert_eq!(tested.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(
        tested
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("remaining"))
            .and_then(Value::as_f64),
        Some(42.0)
    );

    let query_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "usage-script",
            "query",
            "usage-script-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        query_output.status.success(),
        "stderr: {}",
        stderr_text(&query_output)
    );
    let queried: Value = serde_json::from_slice(&query_output.stdout)
        .expect("usage-script query should return json");
    assert_eq!(queried.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(
        queried
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("remaining"))
            .and_then(Value::as_f64),
        Some(42.0)
    );

    let clear_output = run_cli(
        temp.path(),
        &[
            "provider",
            "usage-script",
            "save",
            "usage-script-provider",
            "--app",
            "claude",
            "--clear",
        ],
    );
    assert!(
        clear_output.status.success(),
        "stderr: {}",
        stderr_text(&clear_output)
    );

    let show_after_clear = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "usage-script",
            "show",
            "usage-script-provider",
            "--app",
            "claude",
        ],
    );
    let shown_after_clear: Value = serde_json::from_slice(&show_after_clear.stdout)
        .expect("usage-script show should return json");
    assert!(shown_after_clear
        .get("usageScript")
        .is_some_and(Value::is_null));
}

#[test]
#[serial]
fn provider_stream_check_run_all_and_config_round_trip() {
    let temp = tempdir().expect("tempdir");
    let valid_provider_file = temp.path().join("valid-provider.json");
    let broken_provider_file = temp.path().join("broken-provider.json");
    let mock_response = serde_json::json!({
        "id": "msg_mock_123",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-5",
        "content": [{ "type": "text", "text": "mock anthropic ok" }],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 12,
            "output_tokens": 34
        }
    });
    let server_url = spawn_json_server(mock_response.to_string(), 2);

    fs::write(
        &valid_provider_file,
        format!(
            r#"{{"env":{{"ANTHROPIC_BASE_URL":"{server_url}","ANTHROPIC_AUTH_TOKEN":"sk-stream-ok"}}}}"#
        ),
    )
    .expect("write valid provider json");
    fs::write(
        &broken_provider_file,
        r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:9","ANTHROPIC_AUTH_TOKEN":"sk-stream-bad"}}"#,
    )
    .expect("write broken provider json");

    for (name, file, base_url, api_key) in [
        (
            "Healthy Stream",
            &valid_provider_file,
            server_url.as_str(),
            "sk-stream-ok",
        ),
        (
            "Broken Stream",
            &broken_provider_file,
            "http://127.0.0.1:9",
            "sk-stream-bad",
        ),
    ] {
        let add_output = run_cli(
            temp.path(),
            &[
                "provider",
                "add",
                "--app",
                "claude",
                "--name",
                name,
                "--base-url",
                base_url,
                "--api-key",
                api_key,
                "--from-json",
                file.to_str().expect("utf-8 path"),
            ],
        );
        assert!(
            add_output.status.success(),
            "stderr: {}",
            stderr_text(&add_output)
        );
    }

    let config_file = temp.path().join("stream-check.json");
    let config = serde_json::json!({
        "timeoutSecs": 2,
        "maxRetries": 0,
        "degradedThresholdMs": 9999,
        "claudeModel": "claude-haiku-4-5-20251001",
        "codexModel": "gpt-5.1-codex@low",
        "geminiModel": "gemini-3-pro-preview",
        "testPrompt": "hello stream check"
    });
    fs::write(
        &config_file,
        serde_json::to_string_pretty(&config).expect("serialize stream check config"),
    )
    .expect("write config");

    let set_config = run_cli(
        temp.path(),
        &[
            "provider",
            "stream-check",
            "config",
            "set",
            "--file",
            config_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        set_config.status.success(),
        "stderr: {}",
        stderr_text(&set_config)
    );

    let get_config = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "stream-check",
            "config",
            "get",
        ],
    );
    assert!(
        get_config.status.success(),
        "stderr: {}",
        stderr_text(&get_config)
    );
    let saved_config: Value =
        serde_json::from_slice(&get_config.stdout).expect("config should return json");
    assert_eq!(
        saved_config.get("timeoutSecs").and_then(Value::as_u64),
        Some(2)
    );

    let run_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "stream-check",
            "run",
            "healthy-stream",
            "--app",
            "claude",
        ],
    );
    assert!(
        run_output.status.success(),
        "stderr: {}",
        stderr_text(&run_output)
    );
    let run_json: Value =
        serde_json::from_slice(&run_output.stdout).expect("run should return json");
    assert_eq!(run_json.get("success").and_then(Value::as_bool), Some(true));
    assert_eq!(
        run_json.get("status").and_then(Value::as_str),
        Some("operational")
    );

    let run_all_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "stream-check",
            "run-all",
            "--app",
            "claude",
        ],
    );
    assert!(
        run_all_output.status.success(),
        "stderr: {}",
        stderr_text(&run_all_output)
    );
    let run_all: Value =
        serde_json::from_slice(&run_all_output.stdout).expect("run-all should return json");
    assert_eq!(run_all.as_array().map(Vec::len), Some(2));
    let healthy = run_all
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("providerId") == Some(&Value::String("healthy-stream".to_string()))
            })
        })
        .expect("healthy stream provider should exist");
    assert_eq!(healthy.get("success").and_then(Value::as_bool), Some(true));
    let broken = run_all
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("providerId") == Some(&Value::String("broken-stream".to_string()))
            })
        })
        .expect("broken stream provider should exist");
    assert_eq!(broken.get("success").and_then(Value::as_bool), Some(false));
}

#[test]
#[serial]
fn usage_advanced_queries_model_pricing_and_limits_round_trip() {
    let temp = tempdir().expect("tempdir");
    let mut provider = Provider::with_id(
        "limit-provider".to_string(),
        "Limit Provider".to_string(),
        serde_json::json!({}),
        None,
    );
    provider.meta = Some(ProviderMeta {
        limit_daily_usd: Some("1.00".to_string()),
        limit_monthly_usd: Some("10.00".to_string()),
        ..ProviderMeta::default()
    });
    seed_provider(temp.path(), "claude", provider);

    let now = chrono::Utc::now().timestamp_millis();
    insert_usage_log(
        temp.path(),
        "req-usage-1",
        "claude",
        "limit-provider",
        "claude-haiku-4-5-20251001",
        100,
        50,
        "0.75",
        now,
    );
    insert_usage_log(
        temp.path(),
        "req-usage-2",
        "claude",
        "limit-provider",
        "claude-haiku-4-5-20251001",
        80,
        20,
        "0.50",
        now + 1_000,
    );

    let trends_output = run_cli(temp.path(), &["--format", "json", "usage", "trends"]);
    assert!(
        trends_output.status.success(),
        "stderr: {}",
        stderr_text(&trends_output)
    );
    let trends: Value =
        serde_json::from_slice(&trends_output.stdout).expect("trends should return json");
    assert!(trends.as_array().is_some_and(|items| !items.is_empty()));

    let provider_stats_output = run_cli(
        temp.path(),
        &["--format", "json", "usage", "provider-stats"],
    );
    assert!(
        provider_stats_output.status.success(),
        "stderr: {}",
        stderr_text(&provider_stats_output)
    );
    let provider_stats: Value = serde_json::from_slice(&provider_stats_output.stdout)
        .expect("provider stats should return json");
    let provider_row = provider_stats
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("providerId") == Some(&Value::String("limit-provider".to_string()))
            })
        })
        .expect("provider stats should include limit-provider");
    assert_eq!(
        provider_row.get("requestCount").and_then(Value::as_u64),
        Some(2)
    );

    let model_stats_output = run_cli(temp.path(), &["--format", "json", "usage", "model-stats"]);
    assert!(
        model_stats_output.status.success(),
        "stderr: {}",
        stderr_text(&model_stats_output)
    );
    let model_stats: Value =
        serde_json::from_slice(&model_stats_output.stdout).expect("model stats should return json");
    let model_row = model_stats
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("model") == Some(&Value::String("claude-haiku-4-5-20251001".to_string()))
            })
        })
        .expect("model stats should include seeded model");
    assert_eq!(
        model_row.get("requestCount").and_then(Value::as_u64),
        Some(2)
    );

    let detail_output = run_cli(
        temp.path(),
        &["--format", "json", "usage", "request-detail", "req-usage-1"],
    );
    assert!(
        detail_output.status.success(),
        "stderr: {}",
        stderr_text(&detail_output)
    );
    let detail: Value =
        serde_json::from_slice(&detail_output.stdout).expect("request detail should return json");
    assert_eq!(
        detail.get("requestId").and_then(Value::as_str),
        Some("req-usage-1")
    );

    let update_pricing = run_cli(
        temp.path(),
        &[
            "usage",
            "model-pricing",
            "update",
            "custom-model",
            "--display-name",
            "Custom Model",
            "--input-cost",
            "1.23",
            "--output-cost",
            "4.56",
            "--cache-read-cost",
            "0.11",
            "--cache-creation-cost",
            "0.22",
        ],
    );
    assert!(
        update_pricing.status.success(),
        "stderr: {}",
        stderr_text(&update_pricing)
    );

    let pricing_list = run_cli(
        temp.path(),
        &["--format", "json", "usage", "model-pricing", "list"],
    );
    assert!(
        pricing_list.status.success(),
        "stderr: {}",
        stderr_text(&pricing_list)
    );
    let pricing: Value =
        serde_json::from_slice(&pricing_list.stdout).expect("pricing list should return json");
    assert!(pricing.as_array().is_some_and(|items| items
        .iter()
        .any(|item| { item.get("modelId") == Some(&Value::String("custom-model".to_string())) })));

    let limits_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "usage",
            "provider-limits",
            "check",
            "limit-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        limits_output.status.success(),
        "stderr: {}",
        stderr_text(&limits_output)
    );
    let limits: Value =
        serde_json::from_slice(&limits_output.stdout).expect("limits should return json");
    assert_eq!(
        limits.get("dailyExceeded").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        limits.get("monthlyExceeded").and_then(Value::as_bool),
        Some(false)
    );

    let delete_pricing = run_cli(
        temp.path(),
        &["usage", "model-pricing", "delete", "custom-model"],
    );
    assert!(
        delete_pricing.status.success(),
        "stderr: {}",
        stderr_text(&delete_pricing)
    );

    let pricing_after_delete = run_cli(
        temp.path(),
        &["--format", "json", "usage", "model-pricing", "list"],
    );
    let pricing_after: Value = serde_json::from_slice(&pricing_after_delete.stdout)
        .expect("pricing list should return json");
    assert!(pricing_after.as_array().is_some_and(|items| items
        .iter()
        .all(|item| item.get("modelId") != Some(&Value::String("custom-model".to_string())))));
}

#[test]
#[serial]
fn universal_provider_sync_adds_target_app_providers() {
    let temp = tempdir().expect("tempdir");

    let add_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "universal",
            "add",
            "--name",
            "Omni",
            "--apps",
            "claude,codex",
            "--base-url",
            "https://api.example.com",
            "--api-key",
            "sk-omni",
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );
    let added: Value =
        serde_json::from_slice(&add_output.stdout).expect("universal add should return json");
    assert_eq!(added.get("id").and_then(Value::as_str), Some("omni"));

    let show_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "universal", "show", "omni"],
    );
    assert!(
        show_output.status.success(),
        "stderr: {}",
        stderr_text(&show_output)
    );
    let shown: Value =
        serde_json::from_slice(&show_output.stdout).expect("universal show should return json");
    assert_eq!(shown.get("name").and_then(Value::as_str), Some("Omni"));

    let edit_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "universal",
            "edit",
            "omni",
            "--set-name",
            "Omni Prime",
            "--set-apps",
            "claude,codex,gemini",
            "--set-base-url",
            "https://api2.example.com",
            "--set-api-key",
            "sk-prime",
        ],
    );
    assert!(
        edit_output.status.success(),
        "stderr: {}",
        stderr_text(&edit_output)
    );
    let edited: Value =
        serde_json::from_slice(&edit_output.stdout).expect("universal edit should return json");
    assert_eq!(
        edited.get("name").and_then(Value::as_str),
        Some("Omni Prime")
    );
    assert_eq!(
        edited.get("baseUrl").and_then(Value::as_str),
        Some("https://api2.example.com")
    );
    assert_eq!(
        edited
            .get("apps")
            .and_then(|apps| apps.get("gemini"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let sync_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "universal", "sync", "omni"],
    );
    assert!(
        sync_output.status.success(),
        "stderr: {}",
        stderr_text(&sync_output)
    );
    let synced: Value =
        serde_json::from_slice(&sync_output.stdout).expect("universal sync should return json");
    assert_eq!(
        synced
            .get("syncedApps")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );

    let claude_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    let claude_providers: Value =
        serde_json::from_slice(&claude_output.stdout).expect("provider list should return json");
    assert!(claude_providers.get("universal-claude-omni").is_some());

    let codex_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "codex"],
    );
    let codex_providers: Value =
        serde_json::from_slice(&codex_output.stdout).expect("provider list should return json");
    assert!(codex_providers.get("universal-codex-omni").is_some());

    let gemini_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "gemini"],
    );
    let gemini_providers: Value =
        serde_json::from_slice(&gemini_output.stdout).expect("provider list should return json");
    assert!(gemini_providers.get("universal-gemini-omni").is_some());

    let save_and_sync_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "provider",
            "universal",
            "save-and-sync",
            "--name",
            "Nova",
            "--apps",
            "claude,gemini",
            "--base-url",
            "https://nova.example.com",
            "--api-key",
            "sk-nova",
        ],
    );
    assert!(
        save_and_sync_output.status.success(),
        "stderr: {}",
        stderr_text(&save_and_sync_output)
    );
    let save_and_sync: Value = serde_json::from_slice(&save_and_sync_output.stdout)
        .expect("save-and-sync should return json");
    assert_eq!(
        save_and_sync
            .get("provider")
            .and_then(|provider| provider.get("id"))
            .and_then(Value::as_str),
        Some("nova")
    );

    let claude_after_save = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    let claude_after_save_json: Value = serde_json::from_slice(&claude_after_save.stdout)
        .expect("provider list should return json");
    assert!(claude_after_save_json
        .get("universal-claude-nova")
        .is_some());

    let delete_without_yes = run_cli(temp.path(), &["provider", "universal", "delete", "omni"]);
    assert!(!delete_without_yes.status.success(), "delete should fail");
    assert!(stderr_text(&delete_without_yes).contains("Re-run with --yes"));

    let delete_with_yes = run_cli(
        temp.path(),
        &["provider", "universal", "delete", "omni", "--yes"],
    );
    assert!(
        delete_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&delete_with_yes)
    );
    let delete_nova = run_cli(
        temp.path(),
        &["provider", "universal", "delete", "nova", "--yes"],
    );
    assert!(
        delete_nova.status.success(),
        "stderr: {}",
        stderr_text(&delete_nova)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "universal", "list"],
    );
    let providers: Value = serde_json::from_slice(&list_output.stdout)
        .expect("universal provider list should return json");
    assert_eq!(providers, Value::Object(Default::default()));
}

#[test]
#[serial]
fn proxy_config_set_then_show_round_trips_through_cli() {
    let temp = tempdir().expect("tempdir");

    let set_output = run_cli(
        temp.path(),
        &[
            "--quiet",
            "proxy",
            "config",
            "set",
            "--port",
            "9999",
            "--host",
            "127.0.0.2",
        ],
    );
    assert!(
        set_output.status.success(),
        "stderr: {}",
        stderr_text(&set_output)
    );

    let show_output = run_cli(
        temp.path(),
        &["--format", "json", "proxy", "config", "show"],
    );
    assert!(
        show_output.status.success(),
        "stderr: {}",
        stderr_text(&show_output)
    );

    let value: Value =
        serde_json::from_slice(&show_output.stdout).expect("proxy config should return json");
    assert_eq!(value.get("listen_port").and_then(Value::as_u64), Some(9999));
    assert_eq!(
        value.get("listen_address").and_then(Value::as_str),
        Some("127.0.0.2")
    );
}

#[test]
#[serial]
fn proxy_failover_queue_switch_and_priority_round_trip() {
    let temp = tempdir().expect("tempdir");

    for (name, base_url, api_key) in [
        ("Alpha", "https://alpha.example", "sk-alpha"),
        ("Beta", "https://beta.example", "sk-beta"),
    ] {
        let output = run_cli(
            temp.path(),
            &[
                "provider",
                "add",
                "--app",
                "claude",
                "--name",
                name,
                "--base-url",
                base_url,
                "--api-key",
                api_key,
            ],
        );
        assert!(output.status.success(), "stderr: {}", stderr_text(&output));
    }

    let add_alpha = run_cli(
        temp.path(),
        &[
            "proxy",
            "failover",
            "add",
            "alpha",
            "--app",
            "claude",
            "--priority",
            "5",
        ],
    );
    assert!(
        add_alpha.status.success(),
        "stderr: {}",
        stderr_text(&add_alpha)
    );

    let add_beta = run_cli(
        temp.path(),
        &[
            "proxy",
            "failover",
            "add",
            "beta",
            "--app",
            "claude",
            "--priority",
            "1",
        ],
    );
    assert!(
        add_beta.status.success(),
        "stderr: {}",
        stderr_text(&add_beta)
    );

    let queue_output = run_cli(
        temp.path(),
        &[
            "--format", "json", "proxy", "failover", "queue", "--app", "claude",
        ],
    );
    assert!(
        queue_output.status.success(),
        "stderr: {}",
        stderr_text(&queue_output)
    );
    let queue: Value =
        serde_json::from_slice(&queue_output.stdout).expect("failover queue should return json");
    let items = queue.as_array().expect("queue should be an array");
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].get("providerId").and_then(Value::as_str),
        Some("beta")
    );
    assert_eq!(items[0].get("priority").and_then(Value::as_u64), Some(1));
    assert_eq!(
        items[1].get("providerId").and_then(Value::as_str),
        Some("alpha")
    );
    assert_eq!(items[1].get("priority").and_then(Value::as_u64), Some(5));

    let switch_output = run_cli(
        temp.path(),
        &["proxy", "failover", "switch", "beta", "--app", "claude"],
    );
    assert!(
        switch_output.status.success(),
        "stderr: {}",
        stderr_text(&switch_output)
    );

    let current_output = run_cli(
        temp.path(),
        &["--format", "json", "config", "get", "currentProviderClaude"],
    );
    assert!(
        current_output.status.success(),
        "stderr: {}",
        stderr_text(&current_output)
    );
    let current: Value =
        serde_json::from_slice(&current_output.stdout).expect("current provider should be json");
    assert_eq!(
        current.get("currentProviderClaude").and_then(Value::as_str),
        Some("beta")
    );

    let remove_output = run_cli(
        temp.path(),
        &["proxy", "failover", "remove", "alpha", "--app", "claude"],
    );
    assert!(
        remove_output.status.success(),
        "stderr: {}",
        stderr_text(&remove_output)
    );

    let queue_after_remove = run_cli(
        temp.path(),
        &[
            "--format", "json", "proxy", "failover", "queue", "--app", "claude",
        ],
    );
    let items_after_remove: Value =
        serde_json::from_slice(&queue_after_remove.stdout).expect("queue should return json");
    assert_eq!(items_after_remove.as_array().map(Vec::len), Some(1));
}

#[test]
#[serial]
fn proxy_commands_reject_missing_and_unsupported_providers() {
    let temp = tempdir().expect("tempdir");

    let unsupported_output = run_cli(
        temp.path(),
        &["proxy", "takeover", "enable", "--app", "openclaw"],
    );
    assert!(!unsupported_output.status.success(), "command should fail");
    assert!(stderr_text(&unsupported_output).contains("claude, codex, gemini"));

    let missing_output = run_cli(
        temp.path(),
        &["proxy", "failover", "add", "missing", "--app", "claude"],
    );
    assert!(!missing_output.status.success(), "command should fail");
    assert!(stderr_text(&missing_output).contains("Provider 'missing' not found"));

    let negative_priority = run_cli(
        temp.path(),
        &[
            "proxy",
            "failover",
            "add",
            "missing",
            "--app",
            "claude",
            "--priority=-1",
        ],
    );
    assert!(!negative_priority.status.success(), "command should fail");
    assert!(stderr_text(&negative_priority).contains("zero or greater"));
}

#[test]
#[serial]
fn proxy_circuit_config_rejects_half_open_requests() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(
        temp.path(),
        &[
            "proxy",
            "circuit",
            "config",
            "set",
            "--half-open-requests",
            "2",
        ],
    );
    assert!(!output.status.success(), "command should fail");
    assert!(stderr_text(&output).contains("not supported"));
}

#[test]
#[serial]
fn proxy_global_and_app_config_round_trip() {
    let temp = tempdir().expect("tempdir");

    let global_set = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "global-config",
            "set",
            "--proxy-enabled",
            "true",
            "--host",
            "0.0.0.0",
            "--port",
            "18080",
            "--log-enabled",
            "false",
        ],
    );
    assert!(
        global_set.status.success(),
        "stderr: {}",
        stderr_text(&global_set)
    );
    let global_set_json: Value =
        serde_json::from_slice(&global_set.stdout).expect("global-config set should return json");
    assert_eq!(
        global_set_json.get("proxyEnabled").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        global_set_json.get("listenAddress").and_then(Value::as_str),
        Some("0.0.0.0")
    );
    assert_eq!(
        global_set_json.get("listenPort").and_then(Value::as_u64),
        Some(18080)
    );
    assert_eq!(
        global_set_json
            .get("enableLogging")
            .and_then(Value::as_bool),
        Some(false)
    );

    let global_show = run_cli(
        temp.path(),
        &["--format", "json", "proxy", "global-config", "show"],
    );
    assert!(
        global_show.status.success(),
        "stderr: {}",
        stderr_text(&global_show)
    );
    let global_show_json: Value =
        serde_json::from_slice(&global_show.stdout).expect("global-config show should return json");
    assert_eq!(
        global_show_json
            .get("listenAddress")
            .and_then(Value::as_str),
        Some("0.0.0.0")
    );

    let app_set = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "app-config",
            "set",
            "--app",
            "claude",
            "--enabled",
            "true",
            "--auto-failover-enabled",
            "true",
            "--max-retries",
            "8",
            "--streaming-first-byte-timeout",
            "11",
            "--streaming-idle-timeout",
            "22",
            "--non-streaming-timeout",
            "33",
            "--circuit-failure-threshold",
            "7",
            "--circuit-success-threshold",
            "3",
            "--circuit-timeout-seconds",
            "44",
            "--circuit-error-rate-threshold",
            "0.75",
            "--circuit-min-requests",
            "15",
        ],
    );
    assert!(
        app_set.status.success(),
        "stderr: {}",
        stderr_text(&app_set)
    );
    let app_set_json: Value =
        serde_json::from_slice(&app_set.stdout).expect("app-config set should return json");
    assert_eq!(
        app_set_json.get("appType").and_then(Value::as_str),
        Some("claude")
    );
    assert_eq!(
        app_set_json.get("enabled").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        app_set_json
            .get("autoFailoverEnabled")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        app_set_json.get("maxRetries").and_then(Value::as_u64),
        Some(8)
    );
    assert_eq!(
        app_set_json
            .get("streamingFirstByteTimeout")
            .and_then(Value::as_u64),
        Some(11)
    );
    assert_eq!(
        app_set_json
            .get("streamingIdleTimeout")
            .and_then(Value::as_u64),
        Some(22)
    );
    assert_eq!(
        app_set_json
            .get("nonStreamingTimeout")
            .and_then(Value::as_u64),
        Some(33)
    );
    assert_eq!(
        app_set_json
            .get("circuitFailureThreshold")
            .and_then(Value::as_u64),
        Some(7)
    );
    assert_eq!(
        app_set_json
            .get("circuitSuccessThreshold")
            .and_then(Value::as_u64),
        Some(3)
    );
    assert_eq!(
        app_set_json
            .get("circuitTimeoutSeconds")
            .and_then(Value::as_u64),
        Some(44)
    );
    assert_eq!(
        app_set_json
            .get("circuitErrorRateThreshold")
            .and_then(Value::as_f64),
        Some(0.75)
    );
    assert_eq!(
        app_set_json
            .get("circuitMinRequests")
            .and_then(Value::as_u64),
        Some(15)
    );

    let app_show = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "app-config",
            "show",
            "--app",
            "claude",
        ],
    );
    assert!(
        app_show.status.success(),
        "stderr: {}",
        stderr_text(&app_show)
    );
    let app_show_json: Value =
        serde_json::from_slice(&app_show.stdout).expect("app-config show should return json");
    assert_eq!(
        app_show_json
            .get("autoFailoverEnabled")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        app_show_json
            .get("circuitMinRequests")
            .and_then(Value::as_u64),
        Some(15)
    );
}

#[test]
#[serial]
fn proxy_auto_failover_available_providers_and_cost_settings_round_trip() {
    let temp = tempdir().expect("tempdir");

    for (name, base_url, api_key) in [
        ("Alpha", "https://alpha.example", "sk-alpha"),
        ("Beta", "https://beta.example", "sk-beta"),
    ] {
        let output = run_cli(
            temp.path(),
            &[
                "provider",
                "add",
                "--app",
                "claude",
                "--name",
                name,
                "--base-url",
                base_url,
                "--api-key",
                api_key,
            ],
        );
        assert!(output.status.success(), "stderr: {}", stderr_text(&output));
    }

    let switch_output = run_cli(
        temp.path(),
        &["provider", "switch", "alpha", "--app", "claude"],
    );
    assert!(
        switch_output.status.success(),
        "stderr: {}",
        stderr_text(&switch_output)
    );

    let auto_failover_show = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "auto-failover",
            "show",
            "--app",
            "claude",
        ],
    );
    let auto_failover_show_json: Value = serde_json::from_slice(&auto_failover_show.stdout)
        .expect("auto-failover show should return json");
    assert_eq!(
        auto_failover_show_json
            .get("enabled")
            .and_then(Value::as_bool),
        Some(false)
    );

    let enable_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "auto-failover",
            "enable",
            "--app",
            "claude",
        ],
    );
    assert!(
        enable_output.status.success(),
        "stderr: {}",
        stderr_text(&enable_output)
    );
    let enable_json: Value =
        serde_json::from_slice(&enable_output.stdout).expect("enable should return json");
    assert_eq!(
        enable_json.get("enabled").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        enable_json.get("activeProviderId").and_then(Value::as_str),
        Some("alpha")
    );

    let queue_output = run_cli(
        temp.path(),
        &[
            "--format", "json", "proxy", "failover", "queue", "--app", "claude",
        ],
    );
    let queue: Value =
        serde_json::from_slice(&queue_output.stdout).expect("queue should return json");
    assert_eq!(queue.as_array().map(Vec::len), Some(1));
    assert_eq!(
        queue[0].get("providerId").and_then(Value::as_str),
        Some("alpha")
    );

    let available_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "available-providers",
            "--app",
            "claude",
        ],
    );
    assert!(
        available_output.status.success(),
        "stderr: {}",
        stderr_text(&available_output)
    );
    let available_json: Value = serde_json::from_slice(&available_output.stdout)
        .expect("available providers should return json");
    assert!(available_json.get("alpha").is_none());
    assert!(available_json.get("beta").is_some());

    let multiplier_get = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "default-cost-multiplier",
            "get",
            "--app",
            "claude",
        ],
    );
    let multiplier_get_json: Value = serde_json::from_slice(&multiplier_get.stdout)
        .expect("default-cost-multiplier get should return json");
    assert_eq!(
        multiplier_get_json.get("value").and_then(Value::as_str),
        Some("1")
    );

    let multiplier_set = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "default-cost-multiplier",
            "set",
            "--app",
            "claude",
            "1.5",
        ],
    );
    assert!(
        multiplier_set.status.success(),
        "stderr: {}",
        stderr_text(&multiplier_set)
    );
    let multiplier_set_json: Value = serde_json::from_slice(&multiplier_set.stdout)
        .expect("default-cost-multiplier set should return json");
    assert_eq!(
        multiplier_set_json.get("value").and_then(Value::as_str),
        Some("1.5")
    );

    let pricing_get = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "pricing-model-source",
            "get",
            "--app",
            "claude",
        ],
    );
    let pricing_get_json: Value = serde_json::from_slice(&pricing_get.stdout)
        .expect("pricing-model-source get should return json");
    assert_eq!(
        pricing_get_json.get("value").and_then(Value::as_str),
        Some("response")
    );

    let pricing_set = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "pricing-model-source",
            "set",
            "--app",
            "claude",
            "request",
        ],
    );
    assert!(
        pricing_set.status.success(),
        "stderr: {}",
        stderr_text(&pricing_set)
    );
    let pricing_set_json: Value = serde_json::from_slice(&pricing_set.stdout)
        .expect("pricing-model-source set should return json");
    assert_eq!(
        pricing_set_json.get("value").and_then(Value::as_str),
        Some("request")
    );

    let disable_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "auto-failover",
            "disable",
            "--app",
            "claude",
        ],
    );
    assert!(
        disable_output.status.success(),
        "stderr: {}",
        stderr_text(&disable_output)
    );
    let disable_json: Value =
        serde_json::from_slice(&disable_output.stdout).expect("disable should return json");
    assert_eq!(
        disable_json.get("enabled").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
#[serial]
fn proxy_provider_health_and_circuit_stats_are_exposed() {
    let temp = tempdir().expect("tempdir");

    let add_output = run_cli(
        temp.path(),
        &[
            "provider",
            "add",
            "--app",
            "claude",
            "--name",
            "Healthy Provider",
            "--base-url",
            "https://healthy.example",
            "--api-key",
            "sk-health",
        ],
    );
    assert!(
        add_output.status.success(),
        "stderr: {}",
        stderr_text(&add_output)
    );

    let health_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "provider-health",
            "healthy-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        health_output.status.success(),
        "stderr: {}",
        stderr_text(&health_output)
    );
    let health_json: Value =
        serde_json::from_slice(&health_output.stdout).expect("provider-health should return json");
    assert_eq!(
        health_json.get("provider_id").and_then(Value::as_str),
        Some("healthy-provider")
    );
    assert_eq!(
        health_json.get("is_healthy").and_then(Value::as_bool),
        Some(true)
    );

    let stats_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "proxy",
            "circuit",
            "stats",
            "healthy-provider",
            "--app",
            "claude",
        ],
    );
    assert!(
        stats_output.status.success(),
        "stderr: {}",
        stderr_text(&stats_output)
    );
    let stats_json: Value =
        serde_json::from_slice(&stats_output.stdout).expect("circuit stats should return json");
    assert_eq!(
        stats_json.get("providerId").and_then(Value::as_str),
        Some("healthy-provider")
    );
    assert!(
        stats_json
            .get("stats")
            .is_some_and(serde_json::Value::is_null),
        "circuit stats should be null when proxy is not running"
    );
}

#[test]
#[serial]
fn proxy_cost_and_pricing_validation_errors_surface_to_cli() {
    let temp = tempdir().expect("tempdir");

    let invalid_multiplier = run_cli(
        temp.path(),
        &[
            "proxy",
            "default-cost-multiplier",
            "set",
            "--app",
            "claude",
            "not-a-number",
        ],
    );
    assert!(!invalid_multiplier.status.success(), "command should fail");
    assert!(stderr_text(&invalid_multiplier).contains("Invalid multiplier"));

    let invalid_pricing_source = run_cli(
        temp.path(),
        &[
            "proxy",
            "pricing-model-source",
            "set",
            "--app",
            "claude",
            "invalid",
        ],
    );
    assert!(
        !invalid_pricing_source.status.success(),
        "command should fail"
    );
    assert!(stderr_text(&invalid_pricing_source).contains("Invalid pricing mode"));
}

#[test]
#[serial]
fn usage_summary_defaults_to_all_history_and_days_filters_when_requested() {
    let temp = tempdir().expect("tempdir");

    insert_usage_log(
        temp.path(),
        "req-claude-history",
        "claude",
        "claude-provider",
        "claude-haiku",
        120,
        30,
        "0.0125",
        chrono::NaiveDate::from_ymd_opt(2026, 2, 14)
            .expect("date")
            .and_hms_opt(2, 1, 3)
            .expect("time")
            .and_utc()
            .timestamp(),
    );

    let all_history_output = run_cli(
        temp.path(),
        &["--format", "json", "usage", "summary", "--app", "claude"],
    );
    assert!(
        all_history_output.status.success(),
        "stderr: {}",
        stderr_text(&all_history_output)
    );
    let all_history: Value = serde_json::from_slice(&all_history_output.stdout)
        .expect("usage summary should return json");
    assert_eq!(
        all_history.get("totalRequests").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        all_history.get("totalTokens").and_then(Value::as_u64),
        Some(150)
    );

    let recent_only_output = run_cli(
        temp.path(),
        &[
            "--format", "json", "usage", "summary", "--app", "claude", "--days", "7",
        ],
    );
    assert!(
        recent_only_output.status.success(),
        "stderr: {}",
        stderr_text(&recent_only_output)
    );
    let recent_only: Value = serde_json::from_slice(&recent_only_output.stdout)
        .expect("usage summary should return json");
    assert_eq!(
        recent_only.get("totalRequests").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        recent_only.get("totalTokens").and_then(Value::as_u64),
        Some(0)
    );
}

#[test]
#[serial]
fn usage_logs_export_and_invalid_date_paths_work() {
    let temp = tempdir().expect("tempdir");
    let export_file = temp.path().join("usage.csv");

    insert_usage_log(
        temp.path(),
        "req-claude-a",
        "claude",
        "claude-provider",
        "claude-sonnet",
        100,
        50,
        "0.01",
        chrono::NaiveDate::from_ymd_opt(2026, 3, 5)
            .expect("date")
            .and_hms_opt(8, 0, 0)
            .expect("time")
            .and_utc()
            .timestamp_millis(),
    );
    insert_usage_log(
        temp.path(),
        "req-claude-b",
        "claude",
        "claude-provider",
        "claude-haiku",
        40,
        10,
        "0.005",
        chrono::NaiveDate::from_ymd_opt(2026, 3, 6)
            .expect("date")
            .and_hms_opt(9, 0, 0)
            .expect("time")
            .and_utc()
            .timestamp_millis(),
    );
    insert_usage_log(
        temp.path(),
        "req-codex-a",
        "codex",
        "codex-provider",
        "gpt-5",
        200,
        100,
        "0.02",
        chrono::NaiveDate::from_ymd_opt(2026, 3, 5)
            .expect("date")
            .and_hms_opt(10, 0, 0)
            .expect("time")
            .and_utc()
            .timestamp_millis(),
    );

    let logs_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "usage",
            "logs",
            "--app",
            "claude",
            "--from",
            "2026-03-05",
            "--to",
            "2026-03-05",
        ],
    );
    assert!(
        logs_output.status.success(),
        "stderr: {}",
        stderr_text(&logs_output)
    );
    let logs: Value =
        serde_json::from_slice(&logs_output.stdout).expect("usage logs should return json");
    let items = logs.as_array().expect("logs should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("model").and_then(Value::as_str),
        Some("claude-sonnet")
    );

    let export_output = run_cli(
        temp.path(),
        &[
            "usage",
            "export",
            "--app",
            "claude",
            "--output",
            export_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        export_output.status.success(),
        "stderr: {}",
        stderr_text(&export_output)
    );
    let csv = fs::read_to_string(&export_file).expect("read csv");
    assert!(csv.contains("claude-sonnet"));
    assert!(csv.contains("claude-haiku"));
    assert!(!csv.contains("gpt-5"));

    let invalid_date = run_cli(
        temp.path(),
        &["usage", "logs", "--app", "claude", "--from", "bad-date"],
    );
    assert!(!invalid_date.status.success(), "command should fail");
    assert!(stderr_text(&invalid_date).contains("Expected format: YYYY-MM-DD"));
}

#[test]
#[serial]
fn skill_enable_disable_and_uninstall_round_trip() {
    let temp = tempdir().expect("tempdir");
    seed_installed_skill(temp.path(), "local:demo-skill", "demo-skill");

    let list_output = run_cli(temp.path(), &["--format", "json", "skill", "list"]);
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );
    let skills: Value =
        serde_json::from_slice(&list_output.stdout).expect("skill list should return json");
    assert_eq!(skills.as_array().map(Vec::len), Some(1));

    let enable_output = run_cli(
        temp.path(),
        &["skill", "enable", "local:demo-skill", "--app", "claude"],
    );
    assert!(
        enable_output.status.success(),
        "stderr: {}",
        stderr_text(&enable_output)
    );
    assert!(exists_or_symlink(&claude_skill_dir(
        temp.path(),
        "demo-skill"
    )));

    let disable_output = run_cli(
        temp.path(),
        &["skill", "disable", "local:demo-skill", "--app", "claude"],
    );
    assert!(
        disable_output.status.success(),
        "stderr: {}",
        stderr_text(&disable_output)
    );
    assert!(!exists_or_symlink(&claude_skill_dir(
        temp.path(),
        "demo-skill"
    )));

    let uninstall_without_yes = run_cli(temp.path(), &["skill", "uninstall", "local:demo-skill"]);
    assert!(
        !uninstall_without_yes.status.success(),
        "command should fail"
    );
    assert!(stderr_text(&uninstall_without_yes).contains("Re-run with --yes"));

    let uninstall_with_yes = run_cli(
        temp.path(),
        &["skill", "uninstall", "local:demo-skill", "--yes"],
    );
    assert!(
        uninstall_with_yes.status.success(),
        "stderr: {}",
        stderr_text(&uninstall_with_yes)
    );

    let list_after_uninstall = run_cli(temp.path(), &["--format", "json", "skill", "list"]);
    let skills_after_uninstall: Value = serde_json::from_slice(&list_after_uninstall.stdout)
        .expect("skill list should return json");
    assert_eq!(skills_after_uninstall, Value::Array(vec![]));
}

#[test]
#[serial]
fn skill_unmanaged_scan_import_and_repo_round_trip() {
    let temp = tempdir().expect("tempdir");
    seed_unmanaged_skill(
        temp.path(),
        ".codex/skills/unmanaged-skill",
        "Unmanaged Skill",
        "from live config",
    );

    let scan_output = run_cli(
        temp.path(),
        &["--format", "json", "skill", "unmanaged", "scan"],
    );
    assert!(
        scan_output.status.success(),
        "stderr: {}",
        stderr_text(&scan_output)
    );
    let unmanaged: Value =
        serde_json::from_slice(&scan_output.stdout).expect("unmanaged scan should return json");
    let entries = unmanaged
        .as_array()
        .expect("scan result should be an array");
    assert!(entries.iter().any(|item| {
        item.get("directory").and_then(Value::as_str) == Some("unmanaged-skill")
            && item
                .get("foundIn")
                .and_then(Value::as_array)
                .is_some_and(|labels| labels.iter().any(|label| label == "codex"))
    }));

    let import_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "skill",
            "unmanaged",
            "import",
            "unmanaged-skill",
        ],
    );
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );
    let imported: Value =
        serde_json::from_slice(&import_output.stdout).expect("import should return json");
    let imported_items = imported
        .as_array()
        .expect("import result should be an array");
    assert_eq!(imported_items.len(), 1);
    assert_eq!(
        imported_items[0].get("id").and_then(Value::as_str),
        Some("local:unmanaged-skill")
    );
    assert!(skill_ssot_dir(temp.path(), "unmanaged-skill").exists());

    let repo_list_output = run_cli(temp.path(), &["--format", "json", "skill", "repo", "list"]);
    assert!(
        repo_list_output.status.success(),
        "stderr: {}",
        stderr_text(&repo_list_output)
    );
    let default_repos: Value =
        serde_json::from_slice(&repo_list_output.stdout).expect("repo list should return json");
    assert!(
        default_repos
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "default skill repos should be materialized for repo commands"
    );

    let repo_add_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "skill",
            "repo",
            "add",
            "https://github.com/example/demo.git",
            "--branch",
            "develop",
        ],
    );
    assert!(
        repo_add_output.status.success(),
        "stderr: {}",
        stderr_text(&repo_add_output)
    );
    let added_repo: Value =
        serde_json::from_slice(&repo_add_output.stdout).expect("repo add should return json");
    assert_eq!(
        added_repo.get("owner").and_then(Value::as_str),
        Some("example")
    );
    assert_eq!(added_repo.get("name").and_then(Value::as_str), Some("demo"));
    assert_eq!(
        added_repo.get("branch").and_then(Value::as_str),
        Some("develop")
    );

    let repo_remove_output = run_cli(temp.path(), &["skill", "repo", "remove", "example/demo"]);
    assert!(
        repo_remove_output.status.success(),
        "stderr: {}",
        stderr_text(&repo_remove_output)
    );

    let repo_list_after_remove =
        run_cli(temp.path(), &["--format", "json", "skill", "repo", "list"]);
    let repos_after_remove: Value = serde_json::from_slice(&repo_list_after_remove.stdout)
        .expect("repo list should return json");
    assert!(
        repos_after_remove
            .as_array()
            .is_some_and(|items| items.iter().all(|item| {
                item.get("owner").and_then(Value::as_str) != Some("example")
                    || item.get("name").and_then(Value::as_str) != Some("demo")
            })),
        "removed repo should not remain in repo list"
    );
}

#[test]
#[serial]
fn skill_zip_install_imports_archive_and_syncs_to_app_dir() {
    let temp = tempdir().expect("tempdir");
    let zip_path = temp.path().join("zip-skill.zip");
    create_skill_zip(&zip_path, "zip-skill", "Zip Skill", "from zip");

    let install_output = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "skill",
            "zip-install",
            "--file",
            zip_path.to_str().expect("utf-8 path"),
            "--app",
            "claude",
        ],
    );
    assert!(
        install_output.status.success(),
        "stderr: {}",
        stderr_text(&install_output)
    );

    let installed: Value =
        serde_json::from_slice(&install_output.stdout).expect("zip install should return json");
    let items = installed
        .as_array()
        .expect("zip install should return an array");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("id").and_then(Value::as_str),
        Some("local:zip-skill")
    );
    assert!(skill_ssot_dir(temp.path(), "zip-skill")
        .join("SKILL.md")
        .exists());
    assert!(exists_or_symlink(&claude_skill_dir(
        temp.path(),
        "zip-skill"
    )));
}

#[test]
#[serial]
fn import_deeplink_provider_populates_provider_list() {
    let temp = tempdir().expect("tempdir");
    let deeplink =
        "ccswitch://provider?name=Router&baseUrl=https%3A%2F%2Fapi.example.com&apiKey=sk-demo&app=claude";

    let import_output = run_cli(temp.path(), &["import-deeplink", deeplink]);
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let list_output = run_cli(
        temp.path(),
        &["--format", "json", "provider", "list", "--app", "claude"],
    );
    assert!(
        list_output.status.success(),
        "stderr: {}",
        stderr_text(&list_output)
    );

    let value: Value =
        serde_json::from_slice(&list_output.stdout).expect("provider list should return json");
    let has_router = value
        .as_object()
        .expect("provider list should be an object")
        .values()
        .any(|provider| provider.get("name").and_then(Value::as_str) == Some("Router"));
    assert!(
        has_router,
        "imported provider should exist in provider list"
    );
}

#[test]
#[serial]
fn deeplink_parse_merge_and_preview_expose_structured_requests() {
    let temp = tempdir().expect("tempdir");
    let config_json = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-ant-xxx","ANTHROPIC_BASE_URL":"https://api.anthropic.com/v1","ANTHROPIC_MODEL":"claude-sonnet-4.5"}}"#;
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());
    let config_b64 = config_b64
        .replace('+', "%2B")
        .replace('/', "%2F")
        .replace('=', "%3D");
    let deeplink = format!(
        "ccswitch://v1/import?resource=provider&app=claude&name=Test%20Provider&config={}&configFormat=json",
        config_b64
    );

    let parse_output = run_cli(
        temp.path(),
        &["--format", "json", "deeplink", "parse", deeplink.as_str()],
    );
    assert!(
        parse_output.status.success(),
        "stderr: {}",
        stderr_text(&parse_output)
    );
    let parsed: Value =
        serde_json::from_slice(&parse_output.stdout).expect("deeplink parse should return json");
    assert_eq!(
        parsed.get("resource").and_then(Value::as_str),
        Some("provider")
    );
    assert_eq!(parsed.get("app").and_then(Value::as_str), Some("claude"));
    assert_eq!(
        parsed.get("apiKey").and_then(Value::as_str),
        None,
        "raw parse should not auto-fill config-derived apiKey"
    );

    let merge_output = run_cli(
        temp.path(),
        &["--format", "json", "deeplink", "merge", deeplink.as_str()],
    );
    assert!(
        merge_output.status.success(),
        "stderr: {}",
        stderr_text(&merge_output)
    );
    let merged: Value =
        serde_json::from_slice(&merge_output.stdout).expect("deeplink merge should return json");
    assert_eq!(
        merged.get("apiKey").and_then(Value::as_str),
        Some("sk-ant-xxx")
    );
    assert_eq!(
        merged.get("endpoint").and_then(Value::as_str),
        Some("https://api.anthropic.com/v1")
    );

    let preview_output = run_cli(
        temp.path(),
        &["--format", "json", "deeplink", "preview", deeplink.as_str()],
    );
    assert!(
        preview_output.status.success(),
        "stderr: {}",
        stderr_text(&preview_output)
    );
    let preview: Value = serde_json::from_slice(&preview_output.stdout)
        .expect("deeplink preview should return json");
    assert_eq!(
        preview
            .get("parsed")
            .and_then(|item| item.get("name"))
            .and_then(Value::as_str),
        Some("Test Provider")
    );
    assert_eq!(
        preview
            .get("merged")
            .and_then(|item| item.get("model"))
            .and_then(Value::as_str),
        Some("claude-sonnet-4.5")
    );
}

#[test]
#[serial]
fn export_import_merge_preserves_existing_data() {
    let source = tempdir().expect("tempdir");
    let target = tempdir().expect("tempdir");
    let export_file = source.path().join("backup.json");
    let source_prompt = source.path().join("source-prompt.txt");
    let target_prompt = target.path().join("target-prompt.txt");
    fs::write(&source_prompt, "Keep it sharp.\n").expect("write source prompt");
    fs::write(&target_prompt, "Stay local.\n").expect("write target prompt");

    let source_set = run_cli(source.path(), &["config", "set", "language", "zh"]);
    assert!(
        source_set.status.success(),
        "stderr: {}",
        stderr_text(&source_set)
    );
    let source_add_prompt = run_cli(
        source.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "sharp",
            "--file",
            source_prompt.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        source_add_prompt.status.success(),
        "stderr: {}",
        stderr_text(&source_add_prompt)
    );

    let export_output = run_cli(
        source.path(),
        &[
            "export",
            "--output",
            export_file.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        export_output.status.success(),
        "stderr: {}",
        stderr_text(&export_output)
    );

    let target_set = run_cli(target.path(), &["config", "set", "language", "ja"]);
    assert!(
        target_set.status.success(),
        "stderr: {}",
        stderr_text(&target_set)
    );
    let target_add_prompt = run_cli(
        target.path(),
        &[
            "prompt",
            "add",
            "--app",
            "claude",
            "--id",
            "local",
            "--file",
            target_prompt.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        target_add_prompt.status.success(),
        "stderr: {}",
        stderr_text(&target_add_prompt)
    );

    let import_output = run_cli(
        target.path(),
        &[
            "import",
            "--input",
            export_file.to_str().expect("utf-8 path"),
            "--merge",
        ],
    );
    assert!(
        import_output.status.success(),
        "stderr: {}",
        stderr_text(&import_output)
    );

    let get_output = run_cli(
        target.path(),
        &["--format", "json", "config", "get", "language"],
    );
    assert!(
        get_output.status.success(),
        "stderr: {}",
        stderr_text(&get_output)
    );
    let language: Value =
        serde_json::from_slice(&get_output.stdout).expect("config get should return json");
    assert_eq!(language.get("language").and_then(Value::as_str), Some("zh"));

    let prompt_output = run_cli(
        target.path(),
        &["--format", "json", "prompt", "list", "--app", "claude"],
    );
    assert!(
        prompt_output.status.success(),
        "stderr: {}",
        stderr_text(&prompt_output)
    );
    let prompts: Value =
        serde_json::from_slice(&prompt_output.stdout).expect("prompt list should return json");
    assert_eq!(
        prompts
            .get("sharp")
            .and_then(|item| item.get("content"))
            .and_then(Value::as_str),
        Some("Keep it sharp.\n")
    );
    assert_eq!(
        prompts
            .get("local")
            .and_then(|item| item.get("content"))
            .and_then(Value::as_str),
        Some("Stay local.\n")
    );
}

#[test]
#[serial]
fn invalid_app_error_is_consistent_across_commands() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(temp.path(), &["provider", "list", "--app", "bad-app"]);
    assert!(!output.status.success(), "command should fail");

    let stderr = stderr_text(&output);
    assert!(stderr.contains("Invalid app type: bad-app"));
    assert!(stderr.contains("claude, codex, gemini, opencode, openclaw"));
}

#[test]
#[serial]
fn proxy_commands_reuse_the_same_invalid_app_error() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(
        temp.path(),
        &["proxy", "takeover", "enable", "--app", "bad-app"],
    );
    assert!(!output.status.success(), "command should fail");

    let stderr = stderr_text(&output);
    assert!(stderr.contains("Invalid app type: bad-app"));
    assert!(stderr.contains("claude, codex, gemini, opencode, openclaw"));
}

#[test]
#[serial]
fn verbose_mode_emits_command_context_on_stderr() {
    let temp = tempdir().expect("tempdir");

    let output = run_cli(
        temp.path(),
        &["--verbose", "--format", "json", "config", "path"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));

    let stderr = stderr_text(&output);
    assert!(stderr.contains("Executing config command"));

    let value: Value =
        serde_json::from_slice(&output.stdout).expect("config path should still return json");
    assert!(value.get("configDir").is_some());
}

#[test]
#[serial]
fn auto_launch_commands_round_trip_with_test_state_file() {
    let temp = tempdir().expect("tempdir");
    let state_file = temp.path().join("auto-launch.state");
    let state_file_str = state_file.display().to_string();

    let status = run_cli_with_env(
        temp.path(),
        &["--format", "json", "auto-launch", "status"],
        &[(
            "CC_SWITCH_TEST_AUTO_LAUNCH_STATE_FILE",
            state_file_str.as_str(),
        )],
    );
    assert!(status.status.success(), "stderr: {}", stderr_text(&status));
    let status_json: Value =
        serde_json::from_slice(&status.stdout).expect("auto-launch status should return json");
    assert_eq!(status_json["enabled"], false);
    assert_eq!(status_json["launchOnStartup"], false);

    let enable = run_cli_with_env(
        temp.path(),
        &["--format", "json", "auto-launch", "enable"],
        &[(
            "CC_SWITCH_TEST_AUTO_LAUNCH_STATE_FILE",
            state_file_str.as_str(),
        )],
    );
    assert!(enable.status.success(), "stderr: {}", stderr_text(&enable));
    let enable_json: Value =
        serde_json::from_slice(&enable.stdout).expect("enable should return json");
    assert_eq!(enable_json["enabled"], true);
    assert_eq!(enable_json["launchOnStartup"], true);

    let disable = run_cli_with_env(
        temp.path(),
        &["--format", "json", "auto-launch", "disable"],
        &[(
            "CC_SWITCH_TEST_AUTO_LAUNCH_STATE_FILE",
            state_file_str.as_str(),
        )],
    );
    assert!(
        disable.status.success(),
        "stderr: {}",
        stderr_text(&disable)
    );
    let disable_json: Value =
        serde_json::from_slice(&disable.stdout).expect("disable should return json");
    assert_eq!(disable_json["enabled"], false);
    assert_eq!(disable_json["launchOnStartup"], false);
}

#[test]
#[serial]
fn portable_mode_and_about_commands_report_runtime_metadata() {
    let temp = tempdir().expect("tempdir");
    let portable_root = temp.path().join("portable-app");
    fs::create_dir_all(&portable_root).expect("create portable dir");
    fs::write(portable_root.join("portable.ini"), "").expect("write portable marker");
    let fake_exe = portable_root.join("cc-switch");
    fs::write(&fake_exe, "").expect("write fake exe");
    let fake_exe_str = fake_exe.display().to_string();

    let portable = run_cli_with_env(
        temp.path(),
        &["--format", "json", "portable-mode"],
        &[("CC_SWITCH_TEST_CURRENT_EXE", fake_exe_str.as_str())],
    );
    assert!(
        portable.status.success(),
        "stderr: {}",
        stderr_text(&portable)
    );
    let portable_json: Value =
        serde_json::from_slice(&portable.stdout).expect("portable-mode should return json");
    assert_eq!(portable_json["portableMode"], true);

    let about = run_cli_with_env(
        temp.path(),
        &["--format", "json", "about"],
        &[("CC_SWITCH_TEST_CURRENT_EXE", fake_exe_str.as_str())],
    );
    assert!(about.status.success(), "stderr: {}", stderr_text(&about));
    let about_json: Value = serde_json::from_slice(&about.stdout).expect("about json");
    assert_eq!(about_json["name"], "CC Switch");
    assert_eq!(about_json["portableMode"], true);
    assert_eq!(about_json["version"], env!("CARGO_PKG_VERSION"));
    assert!(about_json["currentReleaseNotesUrl"].as_str().is_some_and(
        |value| value.ends_with(&format!("/releases/tag/v{}", env!("CARGO_PKG_VERSION")))
    ));
}

#[test]
#[serial]
fn tool_versions_can_report_local_and_latest_versions() {
    let temp = tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin dir");
    #[cfg(unix)]
    write_executable(&bin_dir.join("claude"), "#!/bin/sh\necho 'claude 1.2.3'\n");

    let npm_base = spawn_routing_json_server(
        vec![(
            "/@anthropic-ai/claude-code".to_string(),
            r#"{"dist-tags":{"latest":"9.9.9"}}"#.to_string(),
        )],
        1,
    );
    let path_value = prepend_path(&bin_dir);

    let output = run_cli_with_env(
        temp.path(),
        &[
            "--format",
            "json",
            "tool-versions",
            "--tool",
            "claude",
            "--latest",
        ],
        &[
            ("PATH", path_value.as_str()),
            ("CC_SWITCH_TEST_NPM_REGISTRY_BASE_URL", npm_base.as_str()),
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));
    let versions: Value =
        serde_json::from_slice(&output.stdout).expect("tool-versions should return json");
    let claude = versions
        .as_array()
        .and_then(|items| items.first())
        .expect("expected one tool version");
    assert_eq!(claude["name"], "claude");
    assert_eq!(claude["version"], "1.2.3");
    assert_eq!(claude["latestVersion"], "9.9.9");
}

#[test]
#[serial]
fn update_and_release_notes_commands_report_expected_urls() {
    let temp = tempdir().expect("tempdir");
    let github_base = spawn_routing_json_server(
        vec![(
            "/repos/farion1231/cc-switch/releases/latest".to_string(),
            r#"{"tag_name":"v99.1.0"}"#.to_string(),
        )],
        1,
    );

    let update = run_cli_with_env(
        temp.path(),
        &["--format", "json", "update", "check"],
        &[("CC_SWITCH_TEST_GITHUB_API_BASE_URL", github_base.as_str())],
    );
    assert!(update.status.success(), "stderr: {}", stderr_text(&update));
    let update_json: Value = serde_json::from_slice(&update.stdout).expect("update json");
    assert_eq!(update_json["latestVersion"], "99.1.0");
    assert_eq!(update_json["hasUpdate"], true);
    assert!(update_json["releaseNotesUrl"]
        .as_str()
        .is_some_and(|value| value.ends_with("/releases/tag/v99.1.0")));

    let current_release_notes = run_cli(temp.path(), &["--format", "json", "release-notes"]);
    assert!(
        current_release_notes.status.success(),
        "stderr: {}",
        stderr_text(&current_release_notes)
    );
    let current_json: Value = serde_json::from_slice(&current_release_notes.stdout)
        .expect("release-notes should return json");
    assert!(current_json["url"].as_str().is_some_and(
        |value| value.ends_with(&format!("/releases/tag/v{}", env!("CARGO_PKG_VERSION")))
    ));

    let latest_release_notes = run_cli(
        temp.path(),
        &["--format", "json", "release-notes", "--latest"],
    );
    assert!(
        latest_release_notes.status.success(),
        "stderr: {}",
        stderr_text(&latest_release_notes)
    );
    let latest_json: Value = serde_json::from_slice(&latest_release_notes.stdout)
        .expect("latest release-notes should return json");
    assert!(latest_json["url"]
        .as_str()
        .is_some_and(|value| value.ends_with("/releases/latest")));
}

#[test]
#[serial]
fn completions_command_outputs_script_and_install_writes_target_file() {
    let temp = tempdir().expect("tempdir");

    let zsh = run_cli(temp.path(), &["completions", "zsh"]);
    assert!(zsh.status.success(), "stderr: {}", stderr_text(&zsh));
    let zsh_stdout = stdout_text(&zsh);
    assert!(zsh_stdout.contains("_cc-switch"));
    assert!(zsh_stdout.contains("provider"));

    let install_dir = temp.path().join("completions");
    let install_dir_str = install_dir.display().to_string();
    let installed = run_cli(
        temp.path(),
        &[
            "--format",
            "json",
            "install",
            "completions",
            "zsh",
            "--dir",
            install_dir_str.as_str(),
        ],
    );
    assert!(
        installed.status.success(),
        "stderr: {}",
        stderr_text(&installed)
    );
    let installed_json: Value =
        serde_json::from_slice(&installed.stdout).expect("install completions json");
    let installed_path = install_dir.join("_cc-switch");
    assert_eq!(installed_json["path"], installed_path.display().to_string());
    let content = fs::read_to_string(&installed_path).expect("installed completion file");
    assert!(content.contains("_cc-switch"));
}

#[test]
#[serial]
fn install_guide_and_update_guide_surface_actionable_commands() {
    let temp = tempdir().expect("tempdir");
    let github_base = spawn_routing_json_server(
        vec![(
            "/repos/farion1231/cc-switch/releases/latest".to_string(),
            r#"{"tag_name":"v99.1.0"}"#.to_string(),
        )],
        1,
    );

    let install = run_cli(
        temp.path(),
        &["--format", "json", "install", "guide", "--shell", "fish"],
    );
    assert!(
        install.status.success(),
        "stderr: {}",
        stderr_text(&install)
    );
    let install_json: Value = serde_json::from_slice(&install.stdout).expect("install guide json");
    assert_eq!(install_json["recommendedMethod"], "cargo-git");
    assert!(install_json["methods"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item["id"] == "cargo-git"
                && item["commands"].as_array().is_some_and(|commands| {
                    commands.iter().any(|value| {
                        value
                            .as_str()
                            .is_some_and(|text| text.contains("cargo install --git"))
                    })
                })
        })));
    assert_eq!(install_json["completionHints"][0]["shell"], "fish");

    let update = run_cli_with_env(
        temp.path(),
        &["--format", "json", "update", "guide"],
        &[("CC_SWITCH_TEST_GITHUB_API_BASE_URL", github_base.as_str())],
    );
    assert!(update.status.success(), "stderr: {}", stderr_text(&update));
    let update_json: Value = serde_json::from_slice(&update.stdout).expect("update guide json");
    assert_eq!(update_json["latestVersion"], "99.1.0");
    assert_eq!(update_json["hasUpdate"], true);
    assert!(update_json["steps"]
        .as_array()
        .is_some_and(|items| items.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|text| text.contains("git -C") || text.contains("cargo install --git"))
        })));
}

#[test]
#[serial]
fn doctor_reports_runtime_and_selected_app_state() {
    let temp = tempdir().expect("tempdir");
    seed_provider(
        temp.path(),
        "claude",
        Provider::with_id(
            "doctor-provider".to_string(),
            "Doctor Provider".to_string(),
            serde_json::json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token-a",
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            None,
        ),
    );
    with_seeded_state(temp.path(), |_| {
        cc_switch_core::settings::set_current_provider(
            &cc_switch_core::AppType::Claude,
            Some("doctor-provider"),
        )
        .expect("set current provider");
    });

    let output = run_cli(
        temp.path(),
        &["--format", "json", "doctor", "--app", "claude"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));
    let doctor_json: Value = serde_json::from_slice(&output.stdout).expect("doctor json");
    assert!(doctor_json["runtime"]["databasePath"]
        .as_str()
        .is_some_and(|value| value.ends_with(".cc-switch/cc-switch.db")));
    assert_eq!(doctor_json["apps"][0]["app"], "claude");
    assert_eq!(doctor_json["apps"][0]["currentProvider"], "doctor-provider");
    assert_eq!(doctor_json["apps"][0]["providerCount"], 1);
    assert!(doctor_json["apps"][0]["livePaths"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["label"] == "prompt")));
}

#[cfg(unix)]
#[test]
#[serial]
fn doctor_falls_back_to_read_only_database_when_writes_are_unavailable() {
    let temp = tempdir().expect("tempdir");
    seed_provider(
        temp.path(),
        "claude",
        Provider::with_id(
            "readonly-provider".to_string(),
            "Readonly Provider".to_string(),
            serde_json::json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token-b",
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            None,
        ),
    );
    with_seeded_state(temp.path(), |_| {
        cc_switch_core::settings::set_current_provider(
            &cc_switch_core::AppType::Claude,
            Some("readonly-provider"),
        )
        .expect("set current provider");
    });

    let db_path = database_path(temp.path());
    let config_dir = temp.path().join(".cc-switch");
    let original_db_permissions = fs::metadata(&db_path)
        .expect("database metadata")
        .permissions();
    let original_dir_permissions = fs::metadata(&config_dir)
        .expect("config dir metadata")
        .permissions();

    fs::set_permissions(&db_path, fs::Permissions::from_mode(0o444)).expect("readonly db");
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o555)).expect("readonly dir");

    let output = run_cli(
        temp.path(),
        &["--format", "json", "doctor", "--app", "claude"],
    );

    fs::set_permissions(&config_dir, original_dir_permissions).expect("restore dir permissions");
    fs::set_permissions(&db_path, original_db_permissions).expect("restore db permissions");

    assert!(output.status.success(), "stderr: {}", stderr_text(&output));
    let doctor_json: Value = serde_json::from_slice(&output.stdout).expect("doctor json");
    assert_eq!(doctor_json["apps"][0]["app"], "claude");
    assert_eq!(
        doctor_json["apps"][0]["currentProvider"],
        "readonly-provider"
    );
    assert_eq!(doctor_json["apps"][0]["providerCount"], 1);
}
