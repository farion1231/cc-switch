//! ccswitch —— CC-Switch 的命令行前端
//!
//! 与 GUI **同源**：直接复用 `cc_switch_lib` 的 service 层（不是复刻逻辑）。
//!
//! 安全设计：
//!   1. 任何**写操作**前先探测 GUI 是否在运行，在跑就拒绝（除非 --force），避免两个进程抢配置
//!   2. 写操作前自动备份 cc-switch.db 到 `<配置目录>/cli-backups/`（可用 `CCSWITCH_BACKUP_DIR` 覆盖）
//!   3. 只走官方 service 层，不直接改 DB、不手写配置文件的字段级合并
//!   4. --dry-run 可预览
//!
//! 用法：
//!   ccswitch doctor
//!   ccswitch apps
//!   ccswitch providers [--app X]
//!   ccswitch switch --app X --id ID [--force] [--dry-run]
//!   ccswitch mcp [--app X]
//!   ccswitch mcp-set --app X --id SERVER --on|--off [--force]
//!   ccswitch skills [--app X]

use std::process::Command;
use std::sync::Arc;

use cc_switch_lib::app_config::AppType;
use cc_switch_lib::config::get_app_config_dir;
use cc_switch_lib::database::Database;
use cc_switch_lib::services::{McpService, ProviderService, SkillService};
use cc_switch_lib::store::AppState;

/// 备份目录：默认 `<配置目录>/cli-backups/`，可用环境变量 `CCSWITCH_BACKUP_DIR` 覆盖。
fn backup_dir() -> std::path::PathBuf {
    if let Ok(custom) = std::env::var("CCSWITCH_BACKUP_DIR") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return std::path::PathBuf::from(trimmed);
        }
    }
    get_app_config_dir().join("cli-backups")
}

fn apps() -> Vec<(&'static str, AppType)> {
    vec![
        ("claude", AppType::Claude),
        ("codex", AppType::Codex),
        ("gemini", AppType::Gemini),
        ("grokbuild", AppType::GrokBuild),
        ("opencode", AppType::OpenCode),
        ("openclaw", AppType::OpenClaw),
        ("hermes", AppType::Hermes),
    ]
}

fn all_apps() -> Vec<AppType> {
    vec![
        AppType::Claude,
        AppType::Codex,
        AppType::Gemini,
        AppType::GrokBuild,
        AppType::OpenCode,
        AppType::OpenClaw,
        AppType::Hermes,
    ]
}

fn parse_app(s: &str) -> Option<AppType> {
    let s = s.trim().to_lowercase();
    apps().into_iter().find(|(k, _)| *k == s).map(|(_, v)| v)
}

fn gui_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq cc-switch.exe", "/NH"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("cc-switch.exe")
        })
        .unwrap_or(false)
}

fn db_path() -> std::path::PathBuf {
    get_app_config_dir().join("cc-switch.db")
}

fn backup_db() -> Option<String> {
    let src = db_path();
    if !src.exists() {
        return None;
    }
    let dir = backup_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let dst = dir.join(format!("cc-switch-{stamp}.db"));
    std::fs::copy(&src, &dst).ok()?;
    Some(dst.display().to_string())
}

/// 写操作前的地雷检测。返回 true = 可以写
fn write_preflight(force: bool, dry: bool) -> bool {
    if dry {
        return true;
    }
    if gui_running() && !force {
        eprintln!("✗ CC-Switch GUI 正在运行 —— 现在写会两个进程抢配置，已拒绝。");
        eprintln!("  先退出 CC-Switch（托盘图标 → 退出），或加 --force 强行继续。");
        return false;
    }
    match backup_db() {
        Some(p) => println!("· 已备份数据库 -> {p}"),
        None => {
            eprintln!(
                "! 数据库备份失败（{}），为安全起见已中止。",
                db_path().display()
            );
            return false;
        }
    }
    true
}

fn need(v: Option<String>, what: &str) -> String {
    match v {
        Some(x) => x,
        None => {
            eprintln!("✗ 缺少参数 {what}");
            std::process::exit(2);
        }
    }
}

// ---------------------------------------------------------------- doctor
fn cmd_doctor(state: Option<&AppState>) {
    println!(
        "ccswitch (Rust, 与 CC-Switch 同源)  v{}",
        env!("CARGO_PKG_VERSION")
    );
    println!("config dir   = {}", get_app_config_dir().display());
    let dbp = db_path();
    println!(
        "db           = {} ({})",
        dbp.display(),
        if dbp.exists() {
            format!(
                "{} KB",
                std::fs::metadata(&dbp).map(|m| m.len() / 1024).unwrap_or(0)
            )
        } else {
            "不存在".into()
        }
    );
    println!(
        "GUI          = {}",
        if gui_running() {
            "运行中"
        } else {
            "未运行"
        }
    );
    println!("备份目录     = {}", backup_dir().display());
    match state {
        Some(st) => {
            let mut total = 0usize;
            for at in all_apps() {
                if let Ok(m) = ProviderService::list(st, at.clone()) {
                    total += m.len();
                }
            }
            println!("供应商总数   = {total}");
            if let Ok(sv) = McpService::get_all_servers(st) {
                println!("MCP 服务器   = {}", sv.len());
            }
            if let Ok(sk) = SkillService::get_all_installed(&st.db) {
                println!("技能         = {}", sk.len());
            }
        }
        None => println!("! 数据库打不开，上面信息不完整"),
    }
}

// ---------------------------------------------------------------- apps
fn cmd_apps(state: &AppState) {
    println!("{:<10} {:<16} {:>8}  当前生效", "key", "应用", "供应商");
    for (k, at) in apps() {
        let n = ProviderService::list(state, at.clone())
            .map(|m| m.len())
            .unwrap_or(0);
        let cur = ProviderService::current(state, at.clone()).unwrap_or_default();
        println!(
            "{:<10} {:<16} {:>8}  {}",
            k,
            at.as_str(),
            n,
            if cur.is_empty() { "-".into() } else { cur }
        );
    }
}

// ---------------------------------------------------------------- providers
fn cmd_providers(state: &AppState, app: Option<String>) {
    let targets: Vec<(String, AppType)> = match app {
        Some(ref a) => vec![(a.clone(), parse_app(a).unwrap())],
        None => apps()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    };
    for (k, at) in targets {
        let cur = ProviderService::current(state, at.clone()).unwrap_or_default();
        match ProviderService::list(state, at.clone()) {
            Ok(m) => {
                println!(
                    "\n== {} ({}) — {} 个，当前 = {}",
                    at.as_str(),
                    k,
                    m.len(),
                    if cur.is_empty() { "-" } else { &cur }
                );
                for (id, p) in m.iter() {
                    let mark = if *id == cur { "★" } else { " " };
                    println!(
                        "  {} {:<34} id={:<46} {}",
                        mark,
                        p.name,
                        id,
                        p.category.clone().unwrap_or_default()
                    );
                }
            }
            Err(e) => println!("\n== {} ERR: {e}", at.as_str()),
        }
    }
}

// ---------------------------------------------------------------- switch
fn cmd_switch(state: &AppState, app: Option<String>, id: Option<String>, force: bool, dry: bool) {
    let k = need(app, "--app");
    let at = match parse_app(&k) {
        Some(a) => a,
        None => {
            eprintln!("✗ 未知应用 {k}");
            std::process::exit(2)
        }
    };
    let pid = need(id, "--id");

    let list = match ProviderService::list(state, at.clone()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("✗ 读供应商失败: {e}");
            std::process::exit(1)
        }
    };
    let target = match list.get(&pid) {
        Some(p) => p,
        None => {
            eprintln!("✗ {} 下没有 id = {pid} 的供应商。现有：", at.as_str());
            for (i, p) in list.iter() {
                eprintln!("    {:<46} {}", i, p.name);
            }
            std::process::exit(2);
        }
    };
    let cur = ProviderService::current(state, at.clone()).unwrap_or_default();
    println!(
        "切换 {} : {}  →  {}",
        at.as_str(),
        if cur.is_empty() {
            "(无)".into()
        } else {
            list.get(&cur)
                .map(|p| p.name.clone())
                .unwrap_or(cur.clone())
        },
        target.name
    );

    if dry {
        println!("(--dry-run，未执行)");
        return;
    }
    if !write_preflight(force, dry) {
        std::process::exit(3);
    }

    match ProviderService::switch(state, at.clone(), &pid) {
        Ok(r) => {
            println!("✓ 已切换");
            for w in r.warnings {
                println!("  ! {w}");
            }
        }
        Err(e) => {
            eprintln!("✗ 切换失败: {e}");
            std::process::exit(1)
        }
    }
}

// ---------------------------------------------------------------- mcp
fn cmd_mcp(state: &AppState, app: Option<String>) {
    let sv = match McpService::get_all_servers(state) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ {e}");
            return;
        }
    };
    let sel: Option<AppType> = app.as_deref().and_then(parse_app);
    print!("{:<44}", "MCP");
    let cols = [
        "claude",
        "codex",
        "gemini",
        "grokbuild",
        "opencode",
        "hermes",
    ];
    for c in cols {
        print!(" {:<9}", c);
    }
    println!();
    for (id, s) in sv.iter() {
        if let Some(ref only) = sel {
            if !s.apps.is_enabled_for(only) {
                continue;
            }
        }
        print!(
            "{:<44}",
            format!(
                "{}",
                if s.name.len() > 42 {
                    &s.name[..42]
                } else {
                    &s.name
                }
            )
        );
        for c in cols {
            let on = match c {
                "claude" => s.apps.claude,
                "codex" => s.apps.codex,
                "gemini" => s.apps.gemini,
                "grokbuild" => s.apps.grokbuild,
                "opencode" => s.apps.opencode,
                _ => s.apps.hermes,
            };
            print!(" {:<9}", if on { "✓" } else { "·" });
        }
        println!("   id={id}");
    }
}

fn cmd_mcp_set(
    state: &AppState,
    app: Option<String>,
    id: Option<String>,
    on: bool,
    force: bool,
    dry: bool,
) {
    let k = need(app, "--app");
    let at = match parse_app(&k) {
        Some(a) => a,
        None => {
            eprintln!("✗ 未知应用 {k}");
            std::process::exit(2)
        }
    };
    let sid = need(id, "--id");
    println!(
        "{} MCP {sid} 对 {} 的开关 → {}",
        if on { "开启" } else { "关闭" },
        at.as_str(),
        on
    );
    if dry {
        println!("(--dry-run，未执行)");
        return;
    }
    if !write_preflight(force, dry) {
        std::process::exit(3);
    }
    match McpService::toggle_app(state, &sid, at, on) {
        Ok(_) => println!("✓ 完成（已同步到该应用的 MCP 配置）"),
        Err(e) => {
            eprintln!("✗ {e}");
            std::process::exit(1)
        }
    }
}

// ---------------------------------------------------------------- skills
fn cmd_skills(state: &AppState, app: Option<String>) {
    let sk = match SkillService::get_all_installed(&state.db) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ {e}");
            return;
        }
    };
    let sel: Option<AppType> = app.as_deref().and_then(parse_app);
    let mut n = 0;
    for s in sk.iter() {
        if let Some(ref only) = sel {
            if !s.apps.is_enabled_for(only) {
                continue;
            }
        }
        n += 1;
        println!("  {:<44} {}", s.name, s.id);
    }
    println!("  共 {n} 个");
}

// ---------------------------------------------------------------- main
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "help" | "--help" | "-h") {
        println!("ccswitch — CC-Switch 命令行前端（与 GUI 同源）\n");
        println!("  doctor                                 体检");
        println!("  apps                                   应用与供应商概览");
        println!("  providers [--app X]                    供应商列表");
        println!("  switch --app X --id ID [--force] [--dry-run]   切换供应商");
        println!("  mcp [--app X]                          MCP 与开关矩阵");
        println!("  mcp-set --app X --id SERVER --on|--off  开关 MCP");
        println!("  skills [--app X]                       技能库");
        println!("\n环境: CC_SWITCH_TEST_HOME=<dir> 可把配置目录整体重定向（沙箱调试用）");
        return;
    }
    let get = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let force = has("--force");
    let dry = has("--dry-run");

    let cmd = args[0].as_str();
    let need_db = cmd != "help";

    let db = if need_db {
        match Database::init() {
            Ok(d) => Some(Arc::new(d)),
            Err(e) => {
                eprintln!("✗ 打开数据库失败: {e}");
                None
            }
        }
    } else {
        None
    };
    let state = db.clone().map(AppState::new);

    match cmd {
        "doctor" => cmd_doctor(state.as_ref()),
        "apps" => {
            if let Some(s) = state.as_ref() {
                cmd_apps(s);
            }
        }
        "providers" => {
            if let Some(s) = state.as_ref() {
                cmd_providers(s, get("--app"));
            }
        }
        "switch" => {
            if let Some(s) = state.as_ref() {
                cmd_switch(s, get("--app"), get("--id"), force, dry);
            }
        }
        "mcp" => {
            if let Some(s) = state.as_ref() {
                cmd_mcp(s, get("--app"));
            }
        }
        "mcp-set" => {
            if let Some(s) = state.as_ref() {
                cmd_mcp_set(s, get("--app"), get("--id"), has("--on"), force, dry);
            }
        }
        "skills" => {
            if let Some(s) = state.as_ref() {
                cmd_skills(s, get("--app"));
            }
        }
        other => eprintln!("✗ 未知命令 {other}（跑 ccswitch 看帮助）"),
    }
}
