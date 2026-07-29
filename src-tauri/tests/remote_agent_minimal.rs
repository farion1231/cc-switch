use std::collections::{HashMap, HashSet};
use std::fs;
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

#[test]
fn cross_workflows_isolate_host_build_artifacts_per_architecture() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri 应位于仓库根目录下")
        .to_path_buf();

    // cross 镜像的宿主 GLIBC 版本可能不同；若共用 target，前一镜像生成的 build script
    // 会被后一镜像复用并在启动阶段失败，因此 CI 与发布流程都必须按架构隔离缓存根目录。
    for workflow in ["ci.yml", "release.yml"] {
        let path = workspace_root
            .join(".github")
            .join("workflows")
            .join(workflow);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取 {} 失败: {error}", path.display()));

        assert!(
            source.contains("CARGO_TARGET_DIR=src-tauri/target/cross-x86_64"),
            "{workflow} 必须为 x86_64 cross 构建设置独立 target 目录"
        );
        assert!(
            source.contains("CARGO_TARGET_DIR=src-tauri/target/cross-aarch64"),
            "{workflow} 必须为 aarch64 cross 构建设置独立 target 目录"
        );
        assert!(
            source.contains(
                "X86=src-tauri/target/cross-x86_64/x86_64-unknown-linux-musl/release/cc-switch-agent"
            ),
            "{workflow} 必须从隔离后的 x86_64 目录收集 Agent"
        );
        assert!(
            source.contains(
                "ARM=src-tauri/target/cross-aarch64/aarch64-unknown-linux-musl/release/cc-switch-agent"
            ),
            "{workflow} 必须从隔离后的 aarch64 目录收集 Agent"
        );
    }
}

#[test]
fn cross_workflows_accept_and_log_static_pie_artifacts() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri 应位于仓库根目录下")
        .to_path_buf();

    // musl 目标可能生成 static PIE；file 对此输出 `static-pie linked`，不能只接受传统
    // `statically linked` 文案。保留元数据日志可让后续镜像差异直接出现在 Actions 中。
    for workflow in ["ci.yml", "release.yml"] {
        let path = workspace_root
            .join(".github")
            .join("workflows")
            .join(workflow);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取 {} 失败: {error}", path.display()));

        assert!(
            source.contains("STATIC_LINK_PATTERN='statically linked|static-pie linked'"),
            "{workflow} 必须接受普通静态链接与 static PIE"
        );
        assert!(
            source.contains("X86_FILE=$(file \"$X86\")")
                && source.contains("ARM_FILE=$(file \"$ARM\")"),
            "{workflow} 必须保存两个架构的 file 元数据"
        );
        assert!(
            source.contains("printf '%s\\n%s\\n' \"$X86_FILE\" \"$ARM_FILE\""),
            "{workflow} 必须把 Agent 文件类型写入 CI 日志"
        );
        assert_eq!(
            source.matches("grep -Eq \"$STATIC_LINK_PATTERN\"").count(),
            2,
            "{workflow} 必须分别验证两个架构的静态链接类型"
        );
    }
}

#[test]
fn cross_workflows_inspect_elf_headers_instead_of_ldd_exit_status() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri 应位于仓库根目录下")
        .to_path_buf();

    // ldd 对 static PIE 可能返回成功，返回码不能证明存在动态加载器。直接检查 ELF 的
    // INTERP 与 NEEDED 项既适用于 static PIE，也能在 x86_64 主机上验证 aarch64 产物。
    for workflow in ["ci.yml", "release.yml"] {
        let path = workspace_root
            .join(".github")
            .join("workflows")
            .join(workflow);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取 {} 失败: {error}", path.display()));

        assert!(
            source.contains("verify_static_elf() {"),
            "{workflow} 必须提供结构化 ELF 静态检查"
        );
        assert!(
            source.contains("program_headers=$(readelf -l \"$binary\")")
                && source.contains("dynamic_section=$(readelf -d \"$binary\")"),
            "{workflow} 必须读取程序头和动态节"
        );
        assert!(
            source.contains("grep -F 'INTERP' <<<\"$program_headers\"")
                && source.contains("grep -F '(NEEDED)' <<<\"$dynamic_section\""),
            "{workflow} 必须拒绝解释器与共享库依赖"
        );
        assert_eq!(
            source.matches("verify_static_elf \"$").count(),
            2,
            "{workflow} 必须检查两个架构的 Agent"
        );
        assert!(
            !source.contains("if ldd \"$X86\""),
            "{workflow} 不得再用 ldd 返回码判断 static PIE"
        );
    }
}
