// Compiled by the connectivity tests so stdio checks need no Node/Python runtime.
use std::{
    io::{self, BufRead, Write},
    net::TcpListener,
    time::Duration,
};

fn main() {
    let marker = std::env::var("MCP_TEST_MARKER").unwrap();
    let args: Vec<_> = std::env::args().collect();
    let child = args.iter().any(|arg| arg == "--child");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let marker_path = if child {
        format!("{marker}.child")
    } else {
        marker.clone()
    };
    std::fs::write(marker_path, listener.local_addr().unwrap().to_string()).unwrap();
    if child {
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    assert_eq!(
        std::env::current_dir().unwrap().canonicalize().unwrap(),
        std::path::Path::new(&marker)
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
    );
    let _child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--child")
        .spawn()
        .unwrap();
    while !std::path::Path::new(&format!("{marker}.child")).exists() {
        std::thread::sleep(Duration::from_millis(10));
    }
    if args.iter().any(|arg| arg == "--hang") {
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    assert!(args
        .iter()
        .any(|arg| arg == "argument with spaces & symbols"));
    if let Ok(delay) = std::env::var("MCP_TEST_STARTUP_DELAY_MS") {
        std::thread::sleep(Duration::from_millis(delay.parse().unwrap()));
    }
    let mut lines = io::stdin().lock().lines();
    let request = lines.next().unwrap().unwrap();
    assert!(request.contains("\"method\":\"initialize\""));
    let id = request
        .split("\"id\":")
        .nth(1)
        .unwrap()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap();
    println!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{{}},\"serverInfo\":{{\"name\":\"test\",\"version\":\"1\"}}}}}}");
    io::stdout().flush().unwrap();
    let notification = lines.next().unwrap().unwrap();
    assert!(notification.contains("notifications/initialized"));
    std::fs::write(format!("{marker}.initialized"), "ok").unwrap();
    // Exit when the probe closes stdin, just like a well-behaved stdio server.
    for _ in lines {}
}
