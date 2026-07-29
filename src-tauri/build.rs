use std::path::{Path, PathBuf};

const AGENT_INPUTS: [(&str, &str); 2] = [
    (
        "CC_SWITCH_AGENT_X86_64_PATH",
        "cc-switch-agent-linux-x86_64",
    ),
    (
        "CC_SWITCH_AGENT_AARCH64_PATH",
        "cc-switch-agent-linux-aarch64",
    ),
];

fn main() {
    prepare_embedded_agents();
    tauri_build::build();

    // Windows 测试二进制没有标准 Tauri manifest，需要显式嵌入 Common Controls v6；主程序
    // 仍由 Tauri 自己处理 manifest，因此 bins 使用 /MANIFEST:NO 防止重复资源。
    #[cfg(target_os = "windows")]
    {
        let manifest_path =
            PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"))
                .join("common-controls.manifest");
        let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg={manifest_arg}");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    }
}

fn prepare_embedded_agents() {
    let output_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("missing OUT_DIR"));
    for (environment_name, output_name) in AGENT_INPUTS {
        println!("cargo:rerun-if-env-changed={environment_name}");
        let output_path = output_dir.join(output_name);
        match std::env::var_os(environment_name) {
            Some(input_path) => copy_required_agent(environment_name, &input_path, &output_path),
            None => {
                // 本地桌面开发不强制安装 Linux 交叉工具链；空文件只保留编译边界，连接时会
                // 返回 AGENT_EMBEDDED_ARTIFACT_MISSING。发布 workflow 必须设置两个变量。
                std::fs::write(&output_path, []).expect("write empty development Agent entry");
            }
        }
    }
}

fn copy_required_agent(environment_name: &str, input_path: &std::ffi::OsStr, output_path: &Path) {
    let input_path = PathBuf::from(input_path);
    println!("cargo:rerun-if-changed={}", input_path.display());
    let metadata = std::fs::metadata(&input_path).unwrap_or_else(|error| {
        panic!(
            "{environment_name} points to unreadable Agent {}: {error}",
            input_path.display()
        )
    });
    assert!(
        metadata.is_file() && metadata.len() > 0,
        "{environment_name} must point to a non-empty Agent file: {}",
        input_path.display()
    );
    std::fs::copy(&input_path, output_path).unwrap_or_else(|error| {
        panic!(
            "failed to stage embedded Agent {} -> {}: {error}",
            input_path.display(),
            output_path.display()
        )
    });
}
