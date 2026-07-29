use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

#[test]
fn standalone_agent_dependency_graph_excludes_desktop_gui_stack() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("cc-switch-agent")
        .join("Cargo.toml");
    assert!(
        manifest.is_file(),
        "独立 Agent manifest 不存在: {}",
        manifest.display()
    );

    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(&manifest)
        .output()
        .expect("运行 cargo metadata");
    assert!(
        output.status.success(),
        "读取 Agent 依赖图失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Value = serde_json::from_slice(&output.stdout).expect("解析 cargo metadata");
    let packages = metadata["packages"]
        .as_array()
        .expect("metadata packages")
        .iter()
        .filter_map(|package| {
            Some((
                package["id"].as_str()?.to_string(),
                package["name"].as_str()?.to_string(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let root_id = packages
        .iter()
        .find_map(|(id, name)| (name == "cc-switch-agent").then(|| id.clone()))
        .expect("metadata 中缺少 cc-switch-agent package");

    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .expect("metadata resolve nodes")
        .iter()
        .filter_map(|node| {
            Some((
                node["id"].as_str()?.to_string(),
                node["dependencies"]
                    .as_array()?
                    .iter()
                    .filter_map(|dependency| dependency.as_str().map(str::to_string))
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<HashMap<_, _>>();

    // 必须检查完整传递依赖闭包；只检查 Agent manifest 会漏掉 core 间接引入的桌面框架。
    let mut pending = vec![root_id];
    let mut visited = HashSet::new();
    while let Some(package_id) = pending.pop() {
        if !visited.insert(package_id.clone()) {
            continue;
        }
        if let Some(dependencies) = nodes.get(&package_id) {
            pending.extend(dependencies.iter().cloned());
        }
    }

    let banned_fragments = ["tauri", "gtk", "webkit", "wry", "muda"];
    let mut banned = visited
        .iter()
        .filter_map(|id| packages.get(id))
        .filter(|name| {
            let name = name.to_ascii_lowercase();
            banned_fragments
                .iter()
                .any(|fragment| name.contains(fragment))
        })
        .cloned()
        .collect::<Vec<_>>();
    banned.sort();
    banned.dedup();

    assert!(
        banned.is_empty(),
        "最小 Agent 依赖了桌面 GUI 栈: {}",
        banned.join(", ")
    );
}
