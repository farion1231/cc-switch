#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    for arg in std::env::args() {
        if arg.starts_with("codexplusplus://") {
            match codex_plus_core::provider_import::save_pending_provider_import_from_url(&arg) {
                Ok(request) => {
                    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
                        "manager.provider_import_url.pending",
                        serde_json::json!({
                            "name": request.name,
                            "baseUrl": request.base_url
                        }),
                    );
                    focus_existing_manager_window();
                }
                Err(error) => {
                    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
                        "manager.provider_import_url.failed",
                        serde_json::json!({
                            "error": error.to_string()
                        }),
                    );
                }
            }
        }
    }
    if std::env::args().any(|arg| arg == "--show-update") {
        unsafe {
            std::env::set_var("CODEX_PLUS_SHOW_UPDATE", "1");
        }
    }
    codex_plus_manager_lib::run();
}

#[cfg(windows)]
fn focus_existing_manager_window() {
    let current_process_id = std::process::id();
    for process in codex_plus_core::windows_enumerate_processes() {
        if process.process_id == current_process_id {
            continue;
        }
        if process
            .exe_file
            .eq_ignore_ascii_case("codex-plus-plus-manager.exe")
        {
            let _ = codex_plus_core::windows_activate_process_window(process.process_id);
            break;
        }
    }
}

#[cfg(not(windows))]
fn focus_existing_manager_window() {}
