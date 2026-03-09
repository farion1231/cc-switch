use std::fs;
use std::io::Write;

use cc_switch_lib::bridges::skill as skill_bridge;
use cc_switch_lib::AppType;
use super::support::{ensure_test_home, reset_test_fs, test_mutex};

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
        "cc-switch-skill-baseline-{}.zip",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    let file = fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("SKILL.md", options).expect("start skill file");
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

#[test]
fn skill_baseline_import_toggle_and_zip_install_are_stable() {
    let _guard = test_mutex()
        .lock()
        .unwrap_or_else(|err| err.into_inner());

    reset_test_fs();
    let _home = ensure_test_home();
    seed_unmanaged_skill("demo-skill");

    let unmanaged = skill_bridge::scan_unmanaged_skills().expect("scan unmanaged");
    assert_eq!(unmanaged.len(), 1);
    assert_eq!(unmanaged[0].directory, "demo-skill");
    assert_eq!(unmanaged[0].found_in, vec!["claude".to_string()]);

    let imported = skill_bridge::import_skills_from_apps(vec!["demo-skill".to_string()])
        .expect("import skills");
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].id, "local:demo-skill");
    assert!(imported[0].apps.claude);

    let ssot_dir = cc_switch_core::SkillService::get_ssot_dir()
        .expect("ssot dir")
        .join("demo-skill");
    let app_dir = cc_switch_core::SkillService::get_app_skills_dir(&cc_switch_core::AppType::Claude)
        .expect("claude app dir")
        .join("demo-skill");
    assert!(exists_or_symlink(&ssot_dir));
    assert!(exists_or_symlink(&app_dir));

    skill_bridge::toggle_skill_app("local:demo-skill", AppType::Claude, false)
        .expect("disable skill for claude");
    let installed = skill_bridge::get_installed_skills().expect("get installed skills");
    assert_eq!(installed.len(), 1);
    assert!(!installed[0].apps.claude);
    assert!(exists_or_symlink(&ssot_dir));
    assert!(!exists_or_symlink(&app_dir));

    let zip_path = create_skill_zip();
    let zipped =
        skill_bridge::install_skills_from_zip(zip_path.to_string_lossy().as_ref(), AppType::Claude)
            .expect("install from zip");
    assert_eq!(zipped.len(), 1);
    assert!(zipped[0].apps.claude);
    assert!(exists_or_symlink(
        &cc_switch_core::SkillService::get_ssot_dir()
            .expect("ssot dir")
            .join(&zipped[0].directory)
    ));
    let _ = fs::remove_file(zip_path);
}
