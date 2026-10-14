// Each policy test runs in a child process so fake proxy environment variables
// and the global client cannot affect other tests or the user's application.
#[path = "../src/proxy/http_client.rs"]
#[allow(dead_code)]
mod http_client;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

fn serve(status: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0; 2048];
            let _ = stream.read(&mut bytes);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{address}")
}

#[test]
fn outbound_policy_ignores_inherited_proxy_only_when_disabled() {
    let proxy = serve("502 Bad Gateway");
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.args([
        "--exact",
        "outbound_policy_child",
        "--ignored",
        "--nocapture",
    ]);
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "REQUEST_METHOD",
    ] {
        child.env_remove(key);
    }
    let output = child
        .env("HTTP_PROXY", &proxy)
        .env("HTTPS_PROXY", &proxy)
        .env("NO_PROXY", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "invoked only by the isolated parent test"]
fn outbound_policy_child() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let origin = serve("401 Unauthorized");
        let explicit = serve("503 Service Unavailable");
        http_client::init_with_policy(None, true).unwrap();
        assert_eq!(
            http_client::get()
                .get(&origin)
                .send()
                .await
                .unwrap()
                .status(),
            502
        );

        // Rebuild while the inherited proxy is still present: disabling must bypass it.
        http_client::set_follow_system_proxy(false).unwrap();
        assert_eq!(http_client::proxy_policy(), (None, false));
        assert_eq!(
            http_client::get()
                .get(&origin)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );

        // An explicitly saved proxy always takes priority over the follow switch.
        http_client::apply_proxy(Some(&explicit)).unwrap();
        assert_eq!(
            http_client::get()
                .get(&origin)
                .send()
                .await
                .unwrap()
                .status(),
            503
        );
        http_client::set_follow_system_proxy(true).unwrap();
        assert_eq!(
            http_client::get()
                .get(&origin)
                .send()
                .await
                .unwrap()
                .status(),
            503
        );
        http_client::set_follow_system_proxy(false).unwrap();
        http_client::apply_proxy(None).unwrap();
        assert_eq!(
            http_client::get()
                .get(&origin)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );

        // Restart initialization honors the saved choice, and invalid changes are atomic.
        http_client::init_with_policy(None, false).unwrap();
        assert!(http_client::apply_proxy(Some("invalid://proxy")).is_err());
        assert_eq!(http_client::proxy_policy(), (None, false));
        assert_eq!(
            http_client::get()
                .get(&origin)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );

        http_client::set_follow_system_proxy(true).unwrap();
        assert_eq!(
            http_client::get()
                .get(&origin)
                .send()
                .await
                .unwrap()
                .status(),
            502
        );
        http_client::set_follow_system_proxy(false).unwrap();
        assert_eq!(
            http_client::get()
                .get(&origin)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    });
}
