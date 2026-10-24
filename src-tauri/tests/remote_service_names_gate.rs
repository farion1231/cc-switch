//! Executable gate for the **local service names** (MagicDNS / `.local`) that
//! the remote server answers to.
//!
//! The failure this pins is a real one, not a hypothetical. A peer that
//! reaches this machine over its Tailscale MagicDNS name
//! (`mymac.tail1234.ts.net`) or over a `.local` name sends a `Host` / `Origin`
//! that is none of the loopback authorities and none of the LAN IPv4
//! addresses — the two sources the allow-lists used to have. The request then
//! dies at the Host gate (a bare `GET /` is already 403) even though the peer
//! *is* the machine being administered, and `tailscale serve` — which keeps the
//! original `Host` — never gets as far as the `LocalProxy` cell.
//!
//! The names have to come from **this** machine, so they cannot be typed into
//! a user whitelist: the MagicDNS name changes with the tailnet, and the
//! `.local` name is whatever the OS calls this box. They are therefore
//! discovered at runtime, and both lists must be fed from that one discovery
//! — a `Host` that is accepted while the matching `Origin` is not is the
//! classic split, and it is silent on both sides.
//!
//! Three claims, in the order they fail:
//!
//! 1. `service_names_feed_both_lists` — in production source, both
//!    `allowed_hosts` and `allowed_origins` call `service_names()`.
//! 2. `tailscale_dns_name_is_parsed` — the MagicDNS name is taken from
//!    `tailscale status --json`'s `Self.DNSName` and the trailing dot is
//!    stripped, and the local host name contributes both its bare form and the
//!    `.local` form.
//! 3. `names_reach_both_lists` — the discovered names reach **both** lists, in
//!    every form a browser actually sends.
//!
//! The gate is falsifiable: claim 1 runs the same detector over a copy of the
//! production source with the call deleted, and claim 2 asserts the negative
//! form (the name with its trailing dot is *not* in the list).
//!
//! The MagicDNS name is injected through `CC_SWITCH_TAILSCALE_BIN` — the
//! override `whois::resolve_bin()` already consults first, and the one ops
//! uses to point at a specific `tailscale` binary. A fake one prints the JSON
//! below and nothing else, so the case does not depend on whether the machine
//! running the test happens to have Tailscale installed.
//!
//! Source paths are anchored with `CARGO_MANIFEST_DIR`, never cwd-relative: the
//! cwd of a test binary depends on the harness that launched it.

use std::path::{Path, PathBuf};

use cc_switch_lib::{allowed_hosts, allowed_origins, RemoteConfig};

/// 假的 `tailscale` 打印出来的 `status --json`。
///
/// 键名与大小写照抄 CLI 的真实输出（`Self.DNSName`），尾点也照抄 ——
/// `tailscale status --json` 给的就是 `mymac.tail1234.ts.net.`，而浏览器
/// 发来的 `Host` 不带尾点。这条差一个字符的缝就是本文件要钉的东西。
const FAKE_STATUS_JSON: &str = r#"{"Self":{"DNSName":"mymac.tail1234.ts.net."}}"#;

/// 假 tailscale 报告的 MagicDNS 名（去尾点后）。
const FAKE_DNS_NAME: &str = "mymac.tail1234.ts.net";

/// 覆盖变量名：与 `whois::BIN_OVERRIDE_ENV` 同一个。
///
/// 这里是字面量而不是引用 —— `whois` 是 crate 私有模块，集成测试够不着；
/// 这个变量名是应用对外的既有约定（运维文档里就写着它），照抄不是私造。
const TAILSCALE_BIN_ENV: &str = "CC_SWITCH_TAILSCALE_BIN";

/// 造一个只会吐上面那段 JSON 的假 `tailscale`。
///
/// `ip -4` 也答一句：生产代码在非回环档位下会问 Tailscale 的地址，那条命令
/// 落到同一个假二进制上时不能回一段 JSON（否则会被当成 IP 字面量写进名单）。
fn fake_tailscale_bin() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cc-switch-service-names-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建不出临时目录");

    #[cfg(windows)]
    let path = dir.join("tailscale.cmd");
    #[cfg(not(windows))]
    let path = dir.join("tailscale");

    #[cfg(windows)]
    let script = format!(
        "@echo off\r\nif \"%1\"==\"ip\" (echo 192.0.2.10) else (echo {})\r\n",
        FAKE_STATUS_JSON
    );
    #[cfg(not(windows))]
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = \"ip\" ]; then echo 192.0.2.10; else echo '{}'; fi\n",
        FAKE_STATUS_JSON
    );
    std::fs::write(&path, script).expect("写不进假 tailscale");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .expect("读不到假 tailscale")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("改不了假 tailscale 的权限");
    }

    path
}

/// 把假 tailscale 装进环境（每个进程只装一次）。
///
/// 为什么要 `Once`：生产代码把服务名缓存进 `OnceLock`，而缓存是**进程级**的
/// —— 同一个测试进程里，谁先碰到生产代码谁就决定了缓存内容。所以假二进制
/// 必须在任何一次生产调用之前装好；`Once::call_once` 保证「装」只发生一次，
/// 且发生在所有生产调用之前。
fn install_fake_tailscale() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::env::set_var(TAILSCALE_BIN_ENV, fake_tailscale_bin());
    });
}

/// `remote/mod.rs` 的路径，按 `CARGO_MANIFEST_DIR` 锚定。
fn remote_mod_source() -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("remote")
        .join("mod.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()))
}

/// 剥掉整行的行尾注释（`//` / `///`），让断言读的是代码而不是说明。
///
/// 这份 gate 要在注释里点名 `service_names` —— 不剥注释的话，注释本身就会把
/// detector 喂绿。
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| {
            let code = match line.find("//") {
                Some(at) => &line[..at],
                None => line,
            };
            code.trim_end()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `fn <name>(...)` 的函数体（到下一个顶格 `}` 为止）。
fn fn_body(src: &str, name: &str) -> String {
    let stripped = strip_line_comments(src);
    let needle = format!("fn {name}(");
    let start = stripped
        .find(&needle)
        .unwrap_or_else(|| panic!("remote/mod.rs 里必须有 {needle}"));
    let rest = &stripped[start..];
    let end = rest[1..]
        .find("\n}")
        .map(|rel| rel + 1)
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

/// 生产代码的形状判定：这个函数体里有没有取本机服务名。
fn feeds_service_names(body: &str) -> bool {
    body.contains("service_names()")
}

/// 两份名单都从 `service_names()` 取本机服务名。
#[test]
fn service_names_feed_both_lists() {
    let real = remote_mod_source();

    let hosts_body = fn_body(&real, "allowed_hosts");
    assert!(
        feeds_service_names(&hosts_body),
        "allowed_hosts 必须经 service_names() 取本机服务名：MagicDNS 名与 .local \
         名的 Host 不在名单里时，对端连 GET / 都会被 Host 闸门挡成 403"
    );

    let origins_body = fn_body(&real, "allowed_origins");
    assert!(
        feeds_service_names(&origins_body),
        "allowed_origins 必须经 service_names() 取本机服务名：Host 认、Origin 不认 \
         的那个分叉是最难查的一类（CORS 放行、中间件 403）"
    );

    // 负控：把调用删掉，detector 必须能红。
    let broken = real.replace("service_names()", "Vec::new()");
    assert_ne!(
        broken, real,
        "负控失效：源码里找不到 `service_names()` 这段文本，detector 会对着同一份 \
         内容跑两遍，两次都绿"
    );
    assert!(
        !feeds_service_names(&fn_body(&broken, "allowed_hosts")),
        "负控必须能红：删掉 service_names() 调用后 detector 仍判绿，\
         说明它检不出真正承重的那一半"
    );
    assert!(
        !feeds_service_names(&fn_body(&broken, "allowed_origins")),
        "负控必须能红：删掉 service_names() 调用后 detector 仍判绿，\
         说明它检不出真正承重的那一半"
    );
}

/// MagicDNS 名去掉尾点，本机主机名同时贡献裸名与 `.local` 形式。
#[test]
fn tailscale_dns_name_is_parsed() {
    install_fake_tailscale();

    let cfg = RemoteConfig {
        port: 4000,
        ..Default::default()
    };
    let hosts = allowed_hosts(&cfg);

    assert!(
        hosts.contains(&format!("{FAKE_DNS_NAME}:{}", cfg.port)),
        "MagicDNS 名去尾点后必须进名单，实际 {hosts:?}：tailscale status --json 的 \
         Self.DNSName 带尾点，而浏览器发来的 Host 不带"
    );
    assert!(
        !hosts.contains(&format!("{FAKE_DNS_NAME}.:{}", cfg.port)),
        "尾点必须去掉，否则 Host 永远匹配不上：{hosts:?}"
    );
    assert!(
        !hosts
            .iter()
            .any(|h| h.contains(FAKE_DNS_NAME) && h.ends_with(".:4000")),
        "带尾点的形态不得留在名单里：{hosts:?}"
    );

    // 本机主机名那一份：裸名与 `.local` 形式都要在。
    let hostname = hostname_of_this_machine();
    if let Some(host) = hostname {
        let host = host.to_ascii_lowercase();
        assert!(
            hosts.contains(&format!("{host}:{}", cfg.port)),
            "本机主机名 {host:?} 必须在名单里，实际 {hosts:?}"
        );
        if !host.ends_with(".local") {
            assert!(
                hosts.contains(&format!("{host}.local:{}", cfg.port)),
                "`.local` 形式必须在名单里，实际 {hosts:?}：mDNS / Bonjour 解析出来的 \
                 `.local` 名是最常见的本机访问方式"
            );
        }
    }
}

/// 本机发现的那些名字，真的落进两份名单的每一种形态。
///
/// 名单是「名字」与「端口 / scheme」的组合，浏览器发过来的形态有好几种：
/// 带端口的 `Host`（`myhost:4000`）、默认端口省略的 `Host`（`myhost`）、
/// 带端口的 `Origin`、以及 `tailscale serve` 在 443 终止 TLS 时那种不带端口的
/// `Origin`（`https://myhost`）。少一种的形态就是「那一条路径上打不开」。
#[test]
fn names_reach_both_lists() {
    install_fake_tailscale();

    let cfg = RemoteConfig {
        port: 4000,
        ..Default::default()
    };
    let hosts = allowed_hosts(&cfg);
    let origins = allowed_origins(&cfg, true);

    // 本机这一轮真正发现的名字：假 tailscale 报的那一个，加上主机名那一份。
    let mut discovered = vec![FAKE_DNS_NAME.to_string()];
    if let Some(host) = hostname_of_this_machine() {
        let host = host.to_ascii_lowercase();
        if !host.is_empty() {
            discovered.push(host.clone());
            if !host.ends_with(".local") {
                discovered.push(format!("{host}.local"));
            }
        }
    }

    for name in discovered {
        for authority in [format!("{name}:{}", cfg.port), name.clone()] {
            assert!(
                hosts.contains(&authority),
                "{authority:?} 必须进 allowed_hosts，实际 {hosts:?}"
            );
        }
        for scheme in ["http", "https"] {
            for origin in [
                format!("{scheme}://{name}:{}", cfg.port),
                format!("{scheme}://{name}"),
            ] {
                assert!(
                    origins
                        .iter()
                        .any(|o| o.to_str().ok() == Some(origin.as_str())),
                    "{origin:?} 必须进 allowed_origins，实际 {origins:?}"
                );
            }
        }
    }

    // 名单里不得出现通配：`*` 就是「任意主机」的另一种拼写。
    for entry in hosts.iter() {
        assert!(
            !entry.contains('*'),
            "名单里不得有通配条目：{entry:?}（`*` 就是「任意主机」）"
        );
    }
    for entry in origins.iter().filter_map(|o| o.to_str().ok()) {
        assert!(
            !entry.contains('*'),
            "名单里不得有通配条目：{entry:?}（`*` 就是「任意主机」）"
        );
    }
}

/// 本机的主机名 —— 用来断言「主机名那一份也在名单里」。
///
/// 取不到就返回 `None`：那台机器上这一格本来就该是空的，测试不该因此失败。
fn hostname_of_this_machine() -> Option<String> {
    #[cfg(windows)]
    {
        std::env::var("COMPUTERNAME").ok()
    }
    #[cfg(not(windows))]
    {
        let output = std::process::Command::new("hostname").output().ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
