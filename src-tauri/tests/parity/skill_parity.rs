use std::fs;
use std::io::Write;

use cc_switch_lib::bridges::skill as skill_bridge;
use cc_switch_lib::AppType;
use serde_json::json;

use super::support::{create_empty_core_state, ensure_test_home, reset_test_fs, test_mutex};

fn exists_or_symlink(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn seed_unmanaged_skill(directory: &str) {
    let dir = cc_switch_core::SkillService::get_app_skills_dir(&cc_switch_core::AppType::Claude)
        .expect("claude skills dir")
        .join(directory);
    fs::create_dir_all(&dir).expect("create unmanaged skill dir");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: Demo Skill\ndescription: local import\n---\nbody\n",
    )
    .expect("write skill");
}

fn create_skill_zip() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "cc-switch-skill-{}.zip",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    let file = fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("SKILL.md", options)
        .expect("start skill file");
    writer
        .write_all(b"---\nname: Zip Skill\ndescription: from zip\n---\n")
        .expect("write skill");
    writer
        .start_file("README.md", options)
        .expect("start readme");
    writer.write_all(b"zip body").expect("write readme");
    writer.finish().expect("finish zip");
    path
}

fn import_snapshot() -> serde_json::Value {
    let skills = skill_bridge::get_installed_skills().expect("get installed skills");
    let ssot = cc_switch_core::SkillService::get_ssot_dir()
        .expect("ssot dir")
        .join("demo-skill");
    let app = cc_switch_core::SkillService::get_app_skills_dir(&cc_switch_core::AppType::Claude)
        .expect("claude skills dir")
        .join("demo-skill");
    json!({
        "skills": normalize_skills(serde_json::to_value(skills).expect("skills json")),
        "ssotExists": exists_or_symlink(&ssot),
        "appExists": exists_or_symlink(&app),
    })
}

fn normalize_skills(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(skills) = value.as_array_mut() {
        for skill in skills {
            if let Some(apps) = skill.get_mut("apps").and_then(|apps| apps.as_object_mut()) {
                apps.remove("openclaw");
            }
        }
    }
    value
}

#[test]
fn skill_parity_import_from_apps_matches_core() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    seed_unmanaged_skill("demo-skill");
    skill_bridge::import_skills_from_apps(vec!["demo-skill".to_string()]).expect("bridge import");
    let tauri = import_snapshot();

    reset_test_fs();
    let _home = ensure_test_home();
    seed_unmanaged_skill("demo-skill");
    let state = create_empty_core_state();
    cc_switch_core::SkillService::import_from_apps(&state.db, vec!["demo-skill".to_string()])
        .expect("core import");
    let core_skills = state
        .db
        .get_all_installed_skills()
        .expect("core installed skills");
    let ssot = cc_switch_core::SkillService::get_ssot_dir()
        .expect("ssot dir")
        .join("demo-skill");
    let app = cc_switch_core::SkillService::get_app_skills_dir(&cc_switch_core::AppType::Claude)
        .expect("claude skills dir")
        .join("demo-skill");
    let core = json!({
        "skills": normalize_skills(serde_json::to_value(core_skills.into_values().collect::<Vec<_>>()).expect("core skills json")),
        "ssotExists": exists_or_symlink(&ssot),
        "appExists": exists_or_symlink(&app),
    });

    assert_eq!(tauri, core);
}

#[test]
fn skill_parity_zip_install_matches_core() {
    let _guard = test_mutex().lock().unwrap_or_else(|err| err.into_inner());

    let zip_path = create_skill_zip();

    reset_test_fs();
    let _home = ensure_test_home();
    let bridge_installed =
        skill_bridge::install_skills_from_zip(zip_path.to_string_lossy().as_ref(), AppType::Claude)
            .expect("bridge zip install");
    let bridge_directory = bridge_installed[0].directory.clone();
    let tauri = json!({
        "skills": normalize_skills(serde_json::to_value(skill_bridge::get_installed_skills().expect("bridge installed skills")).expect("bridge skills json")),
        "ssotExists": exists_or_symlink(
            &cc_switch_core::SkillService::get_ssot_dir()
                .expect("ssot dir")
                .join(&bridge_directory),
        ),
        "appExists": exists_or_symlink(
            &cc_switch_core::SkillService::get_app_skills_dir(&cc_switch_core::AppType::Claude)
                .expect("claude app dir")
                .join(&bridge_directory),
        ),
    });

    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_empty_core_state();
    let core_installed = cc_switch_core::SkillService::install_from_zip(
        &state.db,
        &zip_path,
        &cc_switch_core::AppType::Claude,
    )
    .expect("core zip install");
    let core_directory = core_installed[0].directory.clone();
    let core = json!({
        "skills": normalize_skills(serde_json::to_value(state.db.get_all_installed_skills().expect("core skills").into_values().collect::<Vec<_>>()).expect("core skills json")),
        "ssotExists": exists_or_symlink(
            &cc_switch_core::SkillService::get_ssot_dir()
                .expect("ssot dir")
                .join(&core_directory),
        ),
        "appExists": exists_or_symlink(
            &cc_switch_core::SkillService::get_app_skills_dir(&cc_switch_core::AppType::Claude)
                .expect("claude app dir")
                .join(&core_directory),
        ),
    });

    let _ = fs::remove_file(&zip_path);
    assert_eq!(tauri, core);
}
