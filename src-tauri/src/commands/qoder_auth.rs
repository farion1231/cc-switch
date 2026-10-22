//! Use Qoder's non-interactive `login` / `status --output json` account commands.
//! The native client owns the device flow, credential storage and token refresh.
use crate::{
    app_config::AppType,
    qoder_accounts::{self, AccountProfile, AccountStore, QoderAccount},
    qoder_config,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::HashMap, path::Path, process::Stdio, sync::Arc, time::Duration};
use tauri::State;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::{oneshot, watch, Mutex},
    time::{timeout, Instant},
};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QoderAuthStatus {
    pub installed: bool,
    pub logged_in: bool,
    pub username: Option<String>,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub auth_source: Option<String>,
    pub organization_id: Option<String>,
    pub accounts: Vec<QoderAccount>,
    pub current_account_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QoderLoginProgress {
    pub auth_url: Option<String>,
    pub complete: bool,
    pub error: Option<String>,
    pub account_id: Option<String>,
}

struct LoginFlow {
    app: AppType,
    cancel: oneshot::Sender<()>,
    cancelled: Arc<AtomicBool>,
    progress: watch::Receiver<QoderLoginProgress>,
    task: tokio::task::JoinHandle<()>,
    expires_at: Instant,
}

#[derive(Default)]
pub struct QoderAuthState {
    flows: Mutex<HashMap<String, LoginFlow>>,
    accounts: Arc<Mutex<()>>,
}

fn edition(app: &str) -> Result<AppType, String> {
    match app {
        "qoder" => Ok(AppType::Qoder),
        "qoder-cn" => Ok(AppType::QoderCn),
        _ => Err("Select a Qoder edition".into()),
    }
}

async fn command_at(
    app: &AppType,
    args: Vec<&'static str>,
    root: &Path,
    isolated: bool,
) -> Result<Command, String> {
    let tool = qoder_config::command(app);
    let mut native =
        tokio::task::spawn_blocking(move || super::misc::qoder_account_command(tool, &args))
            .await
            .map_err(|error| error.to_string())??;
    let variable = if *app == AppType::QoderCn {
        "QODERCN_CONFIG_DIR"
    } else {
        "QODER_CONFIG_DIR"
    };
    native
        .env_remove("QODER_SDK_AUTH_PAYLOAD_FILE")
        .env_remove("QODER_SDK_AUTH_CONFIG_DIR");
    if isolated {
        for prefix in ["QODER", "QODERCN"] {
            for suffix in [
                "PERSONAL_ACCESS_TOKEN",
                "JOB_TOKEN",
                "SERVICE_ACCOUNT_KEY",
                "SDK_ACCESS_TOKEN",
            ] {
                native.env_remove(format!("{prefix}_{suffix}"));
            }
        }
    }
    native
        .env(variable, root)
        // Qoder documents this headless-browser value. The application opens
        // the emitted URL once, and can offer it again if opening fails.
        .env("BROWSER", "www-browser")
        .env("NO_COLOR", "1")
        .current_dir(crate::config::get_home_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut command = Command::from(native);
    command.kill_on_drop(true);
    Ok(command)
}

fn parse_status(source: &[u8]) -> Result<QoderAuthStatus, String> {
    #[derive(Deserialize)]
    struct NativeStatus {
        logged_in: bool,
        username: Option<String>,
        email: Option<String>,
        plan: Option<String>,
        auth_source: Option<String>,
        org_id: Option<String>,
    }
    let status: NativeStatus = serde_json::from_slice(source)
        .map_err(|_| "Cannot read Qoder account status. Update Qoder CLI and retry".to_string())?;
    Ok(QoderAuthStatus {
        installed: true,
        logged_in: status.logged_in,
        username: status.username,
        email: status.email,
        plan: status.plan,
        auth_source: status.auth_source,
        organization_id: status.org_id,
        accounts: Vec::new(),
        current_account_id: None,
    })
}

async fn native_status(
    app: &AppType,
    root: &Path,
    isolated: bool,
) -> Result<QoderAuthStatus, String> {
    let mut command = match command_at(app, vec!["status", "--output", "json"], root, isolated)
        .await
    {
        Ok(command) => command,
        Err(error) if error.ends_with("is not installed") => return Ok(QoderAuthStatus::default()),
        Err(error) => return Err(error),
    };
    let output = timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| "Qoder account status timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("Cannot read Qoder account status. Retry or update Qoder CLI".into());
    }
    parse_status(&output.stdout)
}

impl QoderAuthStatus {
    fn profile(&self) -> AccountProfile {
        AccountProfile {
            username: self.username.clone(),
            email: self.email.clone(),
            plan: self.plan.clone(),
            organization_id: self.organization_id.clone(),
        }
    }
}

async fn account_status(app: &AppType) -> Result<QoderAuthStatus, String> {
    let root = qoder_accounts::absolute_root(app)?;
    let before = qoder_accounts::capture(&root, app)?;
    let mut status = native_status(app, &root, false).await?;
    let current = qoder_accounts::capture(&root, app)?;
    let scope = qoder_accounts::scope(app, &root);
    let mut store = AccountStore::load()?;
    if status.logged_in && status.auth_source.as_deref() != Some("external") {
        if let Some(current) = current.as_ref() {
            store.import(&scope, current.clone(), status.profile(), before.as_ref());
            store.save()?;
        }
    }
    status.accounts = store.accounts(
        &scope,
        if status.logged_in && status.auth_source.as_deref() != Some("external") {
            current.as_ref()
        } else {
            None
        },
    );
    status.current_account_id = status
        .accounts
        .iter()
        .find(|account| account.is_current)
        .map(|account| account.id.clone());
    Ok(status)
}

#[tauri::command]
pub async fn get_qoder_auth_status(
    state: State<'_, QoderAuthState>,
    app: String,
) -> Result<QoderAuthStatus, String> {
    let app = edition(&app)?;
    let _guard = state.accounts.lock().await;
    account_status(&app).await
}

#[tauri::command]
pub async fn use_qoder_account(
    state: State<'_, QoderAuthState>,
    app: String,
    account_id: String,
) -> Result<QoderAuthStatus, String> {
    let app = edition(&app)?;
    let _guard = state.accounts.lock().await;
    // Preserve the latest native login, including refreshed credentials, before switching.
    let live = account_status(&app).await?;
    if live.auth_source.as_deref() == Some("external") {
        return Err("Qoder is using an environment credential. Clear its token environment variable before switching saved accounts".into());
    }
    let root = qoder_accounts::absolute_root(&app)?;
    let scope = qoder_accounts::scope(&app, &root);
    let mut store = AccountStore::load()?;
    let selected = store.credential(&scope, &account_id)?;
    let previous = qoder_accounts::capture(&root, &app)?;
    if previous.as_ref() != Some(&selected) {
        qoder_accounts::replace(&root, &app, previous.as_ref(), Some(&selected))?;
    }
    let status = native_status(&app, &root, false).await;
    let current = qoder_accounts::capture(&root, &app)?;
    let status = match status {
        Ok(status) if status.logged_in => status,
        result => {
            if current.as_ref() == Some(&selected) || current.is_none() {
                qoder_accounts::replace(&root, &app, current.as_ref(), previous.as_ref())?;
            }
            return Err(result
                .err()
                .unwrap_or_else(|| "This Qoder login has expired. Add the account again".into()));
        }
    };
    if let Some(current) = current.as_ref() {
        store.import(&scope, current.clone(), status.profile(), Some(&selected));
    }
    if let Err(error) = store.save() {
        if current.as_ref() == Some(&selected) {
            qoder_accounts::replace(&root, &app, current.as_ref(), previous.as_ref())?;
        } else {
            // A native refresh may rotate the token. Keep its only durable copy
            // instead of replacing it with a credential that can no longer renew.
            return Err(format!("Qoder switched accounts, but CC Switch could not save the refreshed login ({error}). The previous account is still saved in the account list"));
        }
        return Err(error);
    }
    if let Err(error) = crate::qoder_models::set_model(&app, "auto") {
        qoder_accounts::replace(&root, &app, current.as_ref(), previous.as_ref())?;
        return Err(error.to_string());
    }
    account_status(&app).await
}

#[tauri::command]
pub async fn delete_qoder_account(
    state: State<'_, QoderAuthState>,
    app: String,
    account_id: String,
) -> Result<QoderAuthStatus, String> {
    let app = edition(&app)?;
    let _guard = state.accounts.lock().await;
    account_status(&app).await?;
    let root = qoder_accounts::absolute_root(&app)?;
    let scope = qoder_accounts::scope(&app, &root);
    let mut store = AccountStore::load()?;
    let selected = store.credential(&scope, &account_id)?;
    let previous = qoder_accounts::capture(&root, &app)?;
    let active = previous.as_ref() == Some(&selected);
    if active {
        qoder_accounts::replace(&root, &app, previous.as_ref(), None)?;
    }
    store.remove(&scope, &account_id)?;
    if let Err(error) = store.save() {
        if active {
            qoder_accounts::replace(&root, &app, None, previous.as_ref())?;
        }
        return Err(error);
    }
    account_status(&app).await
}

fn login_url(app: &AppType, line: &str) -> Option<String> {
    let url = url::Url::parse(line.trim()).ok()?;
    let host = url.host_str()?;
    let domains: &[&str] = if *app == AppType::QoderCn {
        &["qoder.com.cn", "qoder.cn"]
    } else {
        &["qoder.com", "qoder.sh"]
    };
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && domains
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}"))))
    .then(|| url.into())
}

async fn stop_child(child: &mut Child) {
    if let Some(pid) = child.id() {
        #[cfg(target_os = "windows")]
        {
            let mut kill = Command::new("taskkill");
            kill.args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let _ = timeout(Duration::from_secs(5), kill.status()).await;
        }
        #[cfg(not(target_os = "windows"))]
        // SAFETY: account commands use their own process group, created before spawn.
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn run_login(
    app: AppType,
    mut cancel: oneshot::Receiver<()>,
    progress: &watch::Sender<QoderLoginProgress>,
    accounts: Arc<Mutex<()>>,
    cancelled_flag: Arc<AtomicBool>,
) -> Result<String, String> {
    let root = qoder_accounts::absolute_root(&app)?;
    let staging = qoder_accounts::prepare_login(&root)?;
    qoder_config::ensure_privacy_at(staging.path()).map_err(|e| e.to_string())?;
    let mut command = tokio::select! {
        _ = &mut cancel => return Err("Qoder login cancelled".into()),
        command = command_at(&app, vec!["login"], staging.path(), true) => command?,
    };
    let privacy_app = app.clone();
    tokio::task::spawn_blocking(move || qoder_config::ensure_privacy(&privacy_app))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    if cancel.try_recv().is_ok() {
        return Err("Qoder login cancelled".into());
    }
    let child = command.spawn().map_err(|error| error.to_string())?;
    monitor_login(app.clone(), child, cancel, progress, LOGIN_TIMEOUT).await?;
    let status = native_status(&app, staging.path(), true).await?;
    if cancelled_flag.load(Ordering::Acquire) {
        return Err("Qoder login cancelled".into());
    }
    if !status.logged_in {
        return Err("Qoder sign-in could not be confirmed. Retry".into());
    }
    let credential = qoder_accounts::capture(staging.path(), &app)?
        .ok_or("Qoder did not save the new account credentials")?;
    let _guard = accounts.lock().await;
    if cancelled_flag.load(Ordering::Acquire) {
        return Err("Qoder login cancelled".into());
    }
    let mut store = AccountStore::load()?;
    let id = store.import(
        &qoder_accounts::scope(&app, &root),
        credential,
        status.profile(),
        None,
    );
    store.save()?;
    Ok(id)
}

async fn monitor_login(
    app: AppType,
    mut child: Child,
    mut cancel: oneshot::Receiver<()>,
    progress: &watch::Sender<QoderLoginProgress>,
    limit: Duration,
) -> Result<(), String> {
    let mut lines =
        BufReader::new(child.stdout.take().ok_or("Missing Qoder login output")?).lines();
    // Never forward raw account output or credentials to the frontend/logs.
    let stderr = child.stderr.take().unwrap();
    let drain = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut stderr.take(64 * 1024), &mut tokio::io::sink()).await;
    });
    let deadline = tokio::time::sleep(limit);
    tokio::pin!(deadline);
    let mut reading = true;
    let mut size = 0;
    let result = loop {
        tokio::select! {
            _ = &mut cancel => break Err("Qoder login cancelled".into()),
            _ = &mut deadline => break Err("Qoder login timed out. Please retry".into()),
            line = lines.next_line(), if reading => {
                match line {
                    Ok(Some(line)) => {
                        size += line.len();
                        if size > 64 * 1024 { break Err("Unexpected Qoder login output".into()); }
                        if let Some(auth_url) = login_url(&app, &line) {
                            progress.send_modify(|value| value.auth_url = Some(auth_url));
                        }
                    }
                    Ok(None) => reading = false,
                    Err(_) => break Err("Cannot read Qoder login response".into()),
                }
            }
            status = child.wait() => break match status {
                Ok(status) if status.success() => Ok(()),
                _ => Err("Qoder browser login failed. Retry or update Qoder CLI".into()),
            },
        }
    };
    if result.is_err() {
        stop_child(&mut child).await;
    }
    drain.abort();
    result
}

#[tauri::command]
pub async fn start_qoder_login(
    state: State<'_, QoderAuthState>,
    app: String,
) -> Result<String, String> {
    let app = edition(&app)?;
    let mut flows = state.flows.lock().await;
    flows.retain(|_, flow| {
        let keep = flow.expires_at > Instant::now();
        if !keep {
            flow.cancelled.store(true, Ordering::Release);
        }
        keep
    });
    if flows
        .values()
        .any(|flow| flow.app == app && !flow.task.is_finished())
    {
        return Err("A Qoder login is already in progress for this edition".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (cancel, cancelled) = oneshot::channel();
    let cancelled_flag = Arc::new(AtomicBool::new(false));
    let task_cancelled = cancelled_flag.clone();
    let (progress, receiver) = watch::channel(QoderLoginProgress::default());
    let login_app = app.clone();
    let accounts = state.accounts.clone();
    let task = tokio::spawn(async move {
        let result = run_login(login_app, cancelled, &progress, accounts, task_cancelled).await;
        progress.send_modify(|value| {
            value.complete = result.is_ok();
            match result {
                Ok(id) => value.account_id = Some(id),
                Err(error) => value.error = Some(error),
            }
        });
    });
    flows.insert(
        id.clone(),
        LoginFlow {
            app,
            cancel,
            cancelled: cancelled_flag,
            progress: receiver,
            task,
            expires_at: Instant::now() + LOGIN_TIMEOUT + Duration::from_secs(60),
        },
    );
    Ok(id)
}

#[tauri::command]
pub async fn poll_qoder_login(
    state: State<'_, QoderAuthState>,
    app: String,
    flow_id: String,
) -> Result<QoderLoginProgress, String> {
    let app = edition(&app)?;
    let flows = state.flows.lock().await;
    let flow = flows
        .get(&flow_id)
        .filter(|flow| flow.app == app)
        .ok_or("Qoder login expired or was cancelled")?;
    let result = flow.progress.borrow().clone();
    Ok(result)
}

#[tauri::command]
pub async fn cancel_qoder_login(
    state: State<'_, QoderAuthState>,
    app: String,
    flow_id: String,
) -> Result<(), String> {
    let app = edition(&app)?;
    let mut flows = state.flows.lock().await;
    if flows.get(&flow_id).is_some_and(|flow| flow.app != app) {
        return Err("Qoder login belongs to a different edition".into());
    }
    if let Some(flow) = flows.remove(&flow_id) {
        // Keep the edition reserved until the original child is reaped.
        flow.cancelled.store(true, Ordering::Release);
        let _ = flow.cancel.send(());
        let _ = flow.task.await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qoder_login_child_fixture() {
        let Ok(mode) = std::env::var("CC_SWITCH_QODER_LOGIN_FIXTURE") else {
            return;
        };
        println!("https://qoder.com.cn/login?code=fixture");
        if mode == "wait" {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    fn login_fixture(mode: &str) -> Child {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "commands::qoder_auth::tests::qoder_login_child_fixture",
                "--nocapture",
            ])
            .env("CC_SWITCH_QODER_LOGIN_FIXTURE", mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true);
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000);
        #[cfg(not(target_os = "windows"))]
        command.process_group(0);
        command.spawn().unwrap()
    }

    #[tokio::test]
    async fn qoder_login_cancellation_reaps_the_background_child() {
        let child = login_fixture("wait");
        let (cancel, cancelled) = oneshot::channel();
        let (sender, mut receiver) = watch::channel(QoderLoginProgress::default());
        let task = tokio::spawn(async move {
            monitor_login(
                AppType::QoderCn,
                child,
                cancelled,
                &sender,
                Duration::from_secs(30),
            )
            .await
        });
        timeout(Duration::from_secs(10), receiver.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            receiver.borrow().auth_url.as_deref(),
            Some("https://qoder.com.cn/login?code=fixture")
        );
        cancel.send(()).unwrap();
        let error = timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.contains("cancelled"));
    }

    #[tokio::test]
    async fn qoder_login_timeout_reaps_the_background_child() {
        let child = login_fixture("wait");
        let (_cancel, cancelled) = oneshot::channel();
        let (sender, _) = watch::channel(QoderLoginProgress::default());
        let error = monitor_login(
            AppType::QoderCn,
            child,
            cancelled,
            &sender,
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(error.contains("timed out"));
    }

    #[tokio::test]
    async fn qoder_login_accepts_native_success_without_a_browser_url() {
        let child = login_fixture("success");
        let (_cancel, cancelled) = oneshot::channel();
        let (sender, _) = watch::channel(QoderLoginProgress::default());
        monitor_login(
            AppType::QoderCn,
            child,
            cancelled,
            &sender,
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    }

    #[test]
    fn qoder_login_urls_keep_editions_separate() {
        assert!(login_url(&AppType::Qoder, "  https://qoder.com/device?code=fixture  ").is_some());
        assert!(login_url(
            &AppType::QoderCn,
            "https://auth.qoder.com.cn/login?code=fixture"
        )
        .is_some());
        for url in [
            "http://qoder.com/device",
            "https://qoder.com.evil.test/login",
            "https://user:pass@qoder.com/login",
            "https://evil.test/qoder.com",
            "https://qoder.com:8443/login",
        ] {
            assert!(login_url(&AppType::Qoder, url).is_none());
        }
        assert!(login_url(&AppType::Qoder, "https://qoder.com.cn/login").is_none());
        assert!(login_url(&AppType::QoderCn, "https://qoder.com/login").is_none());
    }

    #[test]
    fn qoder_status_exposes_identity_without_credentials() {
        let status = parse_status(br#"{"logged_in":true,"username":"Fixture","email":"fixture@example.test","plan":"Pro","access_token":"must-not-leak"}"#).unwrap();
        let source = serde_json::to_string(&status).unwrap();
        assert!(status.installed && status.logged_in);
        assert_eq!(status.email.as_deref(), Some("fixture@example.test"));
        assert!(!source.contains("must-not-leak"));
        assert!(!source.contains("access_token"));
        assert!(!parse_status(br#"{"logged_in":false}"#).unwrap().logged_in);
        assert!(parse_status(b"invalid output").is_err());
    }
}
