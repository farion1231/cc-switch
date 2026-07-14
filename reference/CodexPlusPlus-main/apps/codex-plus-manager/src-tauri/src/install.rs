pub use codex_plus_core::install::{
    EntryPointState, InstallActionResult, InstallOptions, ShortcutState, inspect_entrypoints,
};

pub fn install_entrypoints() -> InstallActionResult {
    codex_plus_core::install::install_entrypoints(&InstallOptions::default())
}

pub fn uninstall_entrypoints(options: InstallOptions) -> InstallActionResult {
    codex_plus_core::install::uninstall_entrypoints(&options)
}

pub fn repair_shortcuts() -> InstallActionResult {
    codex_plus_core::install::repair_entrypoints(&InstallOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_entrypoints_reports_two_entrypoints() {
        let state = inspect_entrypoints();

        assert!(matches!(state.silent_shortcut.installed, true | false));
        assert!(matches!(state.management_shortcut.installed, true | false));
    }
}
