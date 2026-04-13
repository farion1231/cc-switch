#![allow(non_snake_case)]

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledEditor {
    pub id: String,
    pub name: String,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exe_path: Option<String>,
    /// How it was detected ("registry" | "path" | "unknown")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
struct EditorDefinition {
    id: &'static str,
    name: &'static str,
    /// Case-insensitive "contains" match against DisplayName in Windows Uninstall registry.
    display_name_keywords: &'static [&'static str],
    /// Candidate executable names under InstallLocation/common install dirs.
    exe_names: &'static [&'static str],
    /// Extra common relative install dirs appended to known roots.
    extra_rel_dirs: &'static [&'static str],
}

fn editor_definitions() -> Vec<EditorDefinition> {
    vec![
        EditorDefinition {
            id: "qoder",
            name: "Qoder",
            display_name_keywords: &["qoder"],
            exe_names: &["Qoder.exe", "qoder.exe"],
            extra_rel_dirs: &["Qoder", "Alibaba\\Qoder"],
        },
        EditorDefinition {
            id: "trae",
            name: "Trae",
            display_name_keywords: &["trae"],
            exe_names: &["Trae.exe", "trae.exe"],
            extra_rel_dirs: &["Trae", "ByteDance\\Trae"],
        },
        EditorDefinition {
            id: "codebuddy",
            name: "CodeBuddy",
            display_name_keywords: &["codebuddy"],
            exe_names: &[
                "CodeBuddy.exe",
                "codebuddy.exe",
                // Some Electron apps keep a "Code.exe"-like naming; keep a couple of fallbacks.
                "CodeBuddy Code.exe",
                "CodeBuddyIDE.exe",
            ],
            extra_rel_dirs: &["CodeBuddy", "Tencent\\CodeBuddy"],
        },
    ]
}

#[tauri::command]
pub async fn list_installed_editors() -> Result<Vec<InstalledEditor>, String> {
    Ok(detect_installed_editors())
}

#[cfg(not(target_os = "windows"))]
fn detect_installed_editors() -> Vec<InstalledEditor> {
    // For now, only Windows auto-detection is implemented (user requested Windows).
    Vec::new()
}

#[cfg(target_os = "windows")]
fn detect_installed_editors() -> Vec<InstalledEditor> {
    editor_definitions()
        .into_iter()
        .map(|def| {
            let (path, source) = detect_single_editor(&def);
            InstalledEditor {
                id: def.id.to_string(),
                name: def.name.to_string(),
                installed: path.is_some(),
                exe_path: path.map(|p| p.to_string_lossy().to_string()),
                source,
            }
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn detect_single_editor(def: &EditorDefinition) -> (Option<std::path::PathBuf>, Option<String>) {
    if let Some(path) = find_editor_in_registry(def) {
        return (Some(path), Some("registry".to_string()));
    }
    if let Some(path) = find_editor_in_common_paths(def) {
        return (Some(path), Some("path".to_string()));
    }
    (None, None)
}

#[cfg(target_os = "windows")]
fn find_editor_in_registry(def: &EditorDefinition) -> Option<std::path::PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let hives = [
        RegKey::predef(HKEY_CURRENT_USER),
        RegKey::predef(HKEY_LOCAL_MACHINE),
    ];

    // Prefer user installs; then machine; also include 32-bit view.
    let uninstall_paths = [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];

    for hive in &hives {
        for uninstall_path in &uninstall_paths {
            let Ok(uninstall) = hive.open_subkey(uninstall_path) else {
                continue;
            };
            for subkey_name in uninstall.enum_keys().filter_map(Result::ok) {
                let Ok(subkey) = uninstall.open_subkey(&subkey_name) else {
                    continue;
                };
                let display_name: String = match subkey.get_value("DisplayName") {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if !keyword_match(&display_name, def.display_name_keywords) {
                    continue;
                }

                // 1) DisplayIcon is often the real exe (sometimes with ",0")
                if let Ok(display_icon) = subkey.get_value::<String, _>("DisplayIcon") {
                    if let Some(exe) = parse_display_icon_to_exe(&display_icon) {
                        if exe.exists() {
                            return Some(exe);
                        }
                    }
                }

                // 2) InstallLocation + exe candidates
                if let Ok(install_location) = subkey.get_value::<String, _>("InstallLocation") {
                    let install_location = install_location.trim().trim_matches('"');
                    if !install_location.is_empty() {
                        let base = std::path::PathBuf::from(install_location);
                        for exe in def.exe_names {
                            let candidate = base.join(exe);
                            if candidate.exists() {
                                return Some(candidate);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn find_editor_in_common_paths(def: &EditorDefinition) -> Option<std::path::PathBuf> {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();

    for key in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(val) = std::env::var_os(key) {
            roots.push(std::path::PathBuf::from(val));
        }
    }

    // Common user-installs: %LOCALAPPDATA%\Programs\...
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(std::path::PathBuf::from(local).join("Programs"));
    }

    // Try each root with known rel dirs and exe names
    for root in roots {
        // 1) root + extra_rel_dir + exe
        for rel in def.extra_rel_dirs {
            for exe in def.exe_names {
                let candidate = root.join(rel).join(exe);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        // 2) root + exe (rare, but keep a direct fallback)
        for exe in def.exe_names {
            let candidate = root.join(exe);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn keyword_match(value: &str, keywords: &[&str]) -> bool {
    let lower = value.to_lowercase();
    keywords
        .iter()
        .any(|k| lower.contains(&k.to_lowercase()))
}

#[cfg(target_os = "windows")]
fn parse_display_icon_to_exe(raw: &str) -> Option<std::path::PathBuf> {
    // Typical formats:
    // - "C:\Path\App.exe,0"
    // - C:\Path\App.exe
    // - "C:\Path\App.exe" --some-flag (rare)
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }

    // Strip icon index: ",0"
    let without_index = trimmed
        .split(',')
        .next()
        .unwrap_or(trimmed)
        .trim()
        .trim_matches('"');

    // If there are args, try to keep only up to ".exe"
    let lower = without_index.to_lowercase();
    if let Some(pos) = lower.find(".exe") {
        let path = &without_index[..pos + 4];
        return Some(std::path::PathBuf::from(path));
    }

    None
}

