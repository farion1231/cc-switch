//! Keep Linux's exported menu tree alive across refreshes. Replacing it makes
//! AppIndicators cancel its two-stage layout/property fetch, which can leave
//! partially populated nodes in the extension's cache.

use super::TrayEntry;

#[derive(Debug, PartialEq)]
enum ItemKey<'a> {
    Item(&'a str),
    Check(&'a str),
    Submenu(&'a str),
    Separator,
}

fn entry_key(entry: &TrayEntry) -> ItemKey<'_> {
    match entry {
        TrayEntry::Item { id, .. } => ItemKey::Item(id),
        TrayEntry::Check { id, .. } => ItemKey::Check(id),
        TrayEntry::Submenu { id, .. } => ItemKey::Submenu(id),
        TrayEntry::Separator => ItemKey::Separator,
    }
}

/// Map new positions to surviving old positions, keeping a longest common
/// subsequence. Removing/reinserting a GTK submenu also recreates its children,
/// so preserve existing order wherever possible, including anonymous separators.
fn retained_indices<T: PartialEq>(old: &[T], new: &[T]) -> Vec<Option<usize>> {
    // Usage, language and selection changes normally only change properties.
    if old == new {
        return (0..old.len()).map(Some).collect();
    }
    let mut lengths = vec![vec![0; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lengths[i][j] = if old[i] == new[j] {
                1 + lengths[i + 1][j + 1]
            } else {
                lengths[i + 1][j].max(lengths[i][j + 1])
            };
        }
    }
    let mut retained = vec![None; new.len()];
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            retained[j] = Some(i);
            i += 1;
            j += 1;
        } else if lengths[i + 1][j] >= lengths[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    retained
}

#[cfg(target_os = "linux")]
pub(super) use native::{refresh, LinuxTrayMenu};

#[cfg(any(target_os = "linux", all(test, target_os = "windows")))]
mod native {
    use super::*;
    use crate::error::AppError;
    #[cfg(target_os = "linux")]
    use crate::store::AppState;
    use crate::tray::{attach_entry, menu_error};
    #[cfg(target_os = "linux")]
    use crate::tray::{
        collect_model, linux_menu_entries, lock, update_problem_indicator, TrayTexts, LAST_STATUS,
        TRAY_ID,
    };
    use std::collections::HashMap;
    use tauri::menu::{Menu, MenuItemKind, PredefinedMenuItem, Submenu};
    #[cfg(target_os = "linux")]
    use tauri::Manager;

    #[cfg(target_os = "linux")]
    pub(crate) struct LinuxTrayMenu(pub Menu<tauri::Wry>);

    enum Parent<'a, R: tauri::Runtime> {
        Menu(&'a Menu<R>),
        Submenu(&'a Submenu<R>),
    }

    impl<R: tauri::Runtime> Parent<'_, R> {
        fn items(&self) -> tauri::Result<Vec<MenuItemKind<R>>> {
            match self {
                Self::Menu(menu) => menu.items(),
                Self::Submenu(menu) => menu.items(),
            }
        }

        fn remove(&self, item: &MenuItemKind<R>) -> tauri::Result<()> {
            match self {
                Self::Menu(menu) => menu.remove(item),
                Self::Submenu(menu) => menu.remove(item),
            }
        }

        fn insert(&self, item: &MenuItemKind<R>, index: usize) -> tauri::Result<()> {
            match self {
                Self::Menu(menu) => menu.insert(item, index),
                Self::Submenu(menu) => menu.insert(item, index),
            }
        }
    }

    fn item_key<R: tauri::Runtime>(item: &MenuItemKind<R>) -> ItemKey<'_> {
        let id = item.id().as_ref();
        match item {
            MenuItemKind::MenuItem(_) => ItemKey::Item(id),
            MenuItemKind::Check(_) => ItemKey::Check(id),
            MenuItemKind::Submenu(_) => ItemKey::Submenu(id),
            // The tray model only creates these four kinds of items.
            MenuItemKind::Predefined(_) => ItemKey::Separator,
            MenuItemKind::Icon(_) => unreachable!("tray model has no icon items"),
        }
    }

    fn sync_children<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        parent: Parent<'_, R>,
        entries: &[TrayEntry],
    ) -> Result<(), AppError> {
        let items = parent.items().map_err(menu_error)?;
        let old: Vec<_> = items.iter().map(item_key).collect();
        let new: Vec<_> = entries.iter().map(entry_key).collect();
        let retained = retained_indices(&old, &new);
        let mut keep = vec![false; items.len()];
        for index in retained.iter().flatten() {
            keep[*index] = true;
        }
        for (item, keep) in items.iter().zip(keep).rev() {
            if !keep {
                parent.remove(item).map_err(menu_error)?;
            }
        }
        for (index, (entry, previous)) in entries.iter().zip(retained).enumerate() {
            if let Some(previous) = previous {
                update_entry(app, &items[previous], entry)?;
            } else {
                let item = match attach_entry(app, entry, &mut HashMap::new())? {
                    Some(item) => item,
                    None => MenuItemKind::Predefined(
                        PredefinedMenuItem::separator(app).map_err(menu_error)?,
                    ),
                };
                parent.insert(&item, index).map_err(menu_error)?;
            }
        }
        Ok(())
    }

    fn update_entry<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        item: &MenuItemKind<R>,
        entry: &TrayEntry,
    ) -> Result<(), AppError> {
        // Skip unchanged setters: they can emit D-Bus property notifications.
        // Read the native state so a partially failed refresh is retryable.
        macro_rules! update {
            ($item:expr, $get:ident, $set:ident, $value:expr) => {
                if $item.$get().map_err(menu_error)? != $value {
                    $item.$set($value).map_err(menu_error)?;
                }
            };
        }
        match (item, entry) {
            (MenuItemKind::MenuItem(item), TrayEntry::Item { text, enabled, .. }) => {
                update!(item, text, set_text, text.as_str());
                update!(item, is_enabled, set_enabled, *enabled);
            }
            (
                MenuItemKind::Check(item),
                TrayEntry::Check {
                    text,
                    enabled,
                    checked,
                    ..
                },
            ) => {
                update!(item, text, set_text, text.as_str());
                update!(item, is_enabled, set_enabled, *enabled);
                update!(item, is_checked, set_checked, *checked);
            }
            (MenuItemKind::Submenu(item), TrayEntry::Submenu { text, children, .. }) => {
                update!(item, text, set_text, text.as_str());
                sync_children(app, Parent::Submenu(item), children)?;
            }
            (MenuItemKind::Predefined(_), TrayEntry::Separator) => {}
            _ => unreachable!("only matching menu kinds are retained"),
        }
        Ok(())
    }

    /// Called only on the main thread, including snapshot collection. Concurrent
    /// refresh requests cannot interleave edits or apply an older snapshot last.
    #[cfg(target_os = "linux")]
    pub(crate) fn refresh(app: &tauri::AppHandle) -> Result<bool, AppError> {
        let (Some(state), Some(menu)) = (
            app.try_state::<AppState>(),
            app.try_state::<LinuxTrayMenu>(),
        ) else {
            return Ok(false);
        };
        if app.tray_by_id(TRAY_ID).is_none() {
            return Ok(false);
        }
        let texts = TrayTexts::current();
        let model = collect_model(state.inner(), &texts)?;
        let entries = linux_menu_entries(model.entries);
        sync_children(app, Parent::Menu(&menu.0), &entries)?;
        update_problem_indicator(app, &texts, &model.status.problems);
        *lock(&LAST_STATUS) = model.status;
        Ok(true)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        #[cfg_attr(
            target_os = "linux",
            ignore = "requires GTK display (run with xvfb-run)"
        )]
        fn native_refresh_keeps_handles_and_applies_properties_and_structure() {
            #[cfg(target_os = "linux")]
            gtk::init().unwrap();

            let app = tauri::test::mock_app();
            let app = app.handle();
            let menu = Menu::new(app).unwrap();
            let model = |title: &str, children| {
                vec![
                    TrayEntry::item("show_main", "Open"),
                    TrayEntry::Separator,
                    TrayEntry::Submenu {
                        id: "submenu_claude".into(),
                        text: title.into(),
                        app: None,
                        children,
                    },
                    TrayEntry::item("quit", "Quit"),
                ]
            };
            let entries = model(
                "Claude · A · $10",
                vec![
                    TrayEntry::check("a", "A", true, true),
                    TrayEntry::check("b", "B", true, false),
                ],
            );
            sync_children(app, Parent::Menu(&menu), &entries).unwrap();
            // Materialize the GTK widgets and AppIndicator's exported menu;
            // unattached muda menus only store Rust-side item state on Linux.
            #[cfg(target_os = "linux")]
            let _tray = tauri::tray::TrayIconBuilder::with_id("cc-switch-menu-sync-test")
                .menu(&menu)
                .build(app)
                .unwrap();
            let root = menu.items().unwrap();
            let submenu = root[2].as_submenu().unwrap();
            let children = submenu.items().unwrap();
            let provider_a = children[0].as_check_menuitem().unwrap();
            let provider_b = children[1].as_check_menuitem().unwrap();

            // Repeated usage/selection updates must mutate the original handles.
            for remaining in (0..100).rev() {
                let entries = model(
                    &format!("Claude · B · ${remaining}"),
                    vec![
                        TrayEntry::check("a", "A renamed", false, false),
                        TrayEntry::check("b", "B", true, true),
                    ],
                );
                sync_children(app, Parent::Menu(&menu), &entries).unwrap();
                assert_eq!(
                    submenu.text().unwrap(),
                    format!("Claude · B · ${remaining}")
                );
                assert_eq!(provider_a.text().unwrap(), "A renamed");
                assert!(!provider_a.is_enabled().unwrap());
                assert!(!provider_a.is_checked().unwrap());
                assert!(provider_b.is_checked().unwrap());
                assert_eq!(menu.items().unwrap()[1].id(), root[1].id());
            }

            // Insert/remove siblings and change a node's kind, retaining the app
            // submenu and the provider that remains in it.
            let mut entries = model(
                "Claude · B",
                vec![
                    TrayEntry::check("b", "B retained", true, true),
                    TrayEntry::item("a", "A needs routing"),
                ],
            );
            entries.insert(0, TrayEntry::item("problem", "Service down"));
            entries.insert(1, TrayEntry::Separator);
            sync_children(app, Parent::Menu(&menu), &entries).unwrap();
            assert_eq!(submenu.text().unwrap(), "Claude · B");
            assert_eq!(provider_b.text().unwrap(), "B retained");
            let children = submenu.items().unwrap();
            assert_eq!(children[0].id().as_ref(), "b");
            assert!(children[1].as_menuitem().is_some());
            assert_eq!(menu.items().unwrap()[3].id(), root[1].id());

            entries.drain(..2);
            sync_children(app, Parent::Menu(&menu), &entries).unwrap();
            let root_after = menu.items().unwrap();
            assert_eq!(root_after.len(), 4);
            // Mutating an old handle must still change the attached submenu.
            submenu.set_text("still attached").unwrap();
            assert_eq!(
                root_after[2].as_submenu().unwrap().text().unwrap(),
                "still attached"
            );

            // Refreshing again restores the model through the same handle.
            sync_children(app, Parent::Menu(&menu), &entries).unwrap();
            assert_eq!(submenu.text().unwrap(), "Claude · B");
            sync_children(app, Parent::Menu(&menu), &entries).unwrap();
            assert_eq!(submenu.text().unwrap(), "Claude · B");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retained(old: &[TrayEntry], new: &[TrayEntry]) -> Vec<Option<usize>> {
        retained_indices(
            &old.iter().map(entry_key).collect::<Vec<_>>(),
            &new.iter().map(entry_key).collect::<Vec<_>>(),
        )
    }

    fn submenu(text: &str, children: Vec<TrayEntry>) -> TrayEntry {
        TrayEntry::Submenu {
            id: "submenu_claude".into(),
            text: text.into(),
            app: None,
            children,
        }
    }

    #[test]
    fn property_refreshes_keep_all_nodes() {
        let old = vec![
            TrayEntry::item("show_main", "Open CC Switch"),
            TrayEntry::Separator,
            submenu("Claude Code · A · $10", vec![]),
            TrayEntry::check("lightweight_mode", "Lightweight mode", true, false),
        ];
        let new = vec![
            TrayEntry::label("show_main", "打开 CC Switch"),
            TrayEntry::Separator,
            submenu("Claude Code · B · $9", vec![]),
            TrayEntry::check("lightweight_mode", "轻量模式", false, true),
        ];
        assert_eq!(
            retained(&old, &new),
            vec![Some(0), Some(1), Some(2), Some(3)]
        );
        assert_eq!(retained(&new, &new), retained(&old, &new));
    }

    #[test]
    fn provider_switch_and_quota_change_keep_existing_children() {
        let old = vec![
            TrayEntry::check("prov:claude:a", "A", true, true),
            TrayEntry::check("prov:claude:b", "B", true, false),
            TrayEntry::item("nav:app:claude:quota", "$10 left"),
        ];
        let new = vec![
            TrayEntry::check("prov:claude:a", "A renamed", true, false),
            TrayEntry::check("prov:claude:b", "B", true, true),
            TrayEntry::item("nav:app:claude:quota", "$9 left"),
        ];
        assert_eq!(retained(&old, &new), vec![Some(0), Some(1), Some(2)]);
        assert_eq!(
            retained(&[submenu("A", old)], &[submenu("B", new)]),
            vec![Some(0)]
        );
    }

    #[test]
    fn problem_rows_can_appear_and_disappear_without_recreating_apps() {
        let old = vec![
            TrayEntry::item("show_main", "Open"),
            TrayEntry::Separator,
            submenu("Claude", vec![]),
            TrayEntry::Separator,
            TrayEntry::item("quit", "Quit"),
        ];
        let mut new = vec![
            TrayEntry::item("problem", "Service down"),
            TrayEntry::Separator,
        ];
        new.extend(old.clone());
        assert_eq!(
            retained(&old, &new),
            vec![None, None, Some(0), Some(1), Some(2), Some(3), Some(4)]
        );
        assert_eq!(
            retained(&new, &old),
            vec![Some(2), Some(3), Some(4), Some(5), Some(6)]
        );
    }

    #[test]
    fn additions_removals_and_reordering_preserve_surviving_siblings() {
        assert_eq!(
            retained_indices(&["a", "b", "c"], &["a", "c", "d"]),
            vec![Some(0), Some(2), None]
        );
        assert_eq!(
            retained_indices(&["a", "b", "c", "d"], &["d", "a", "b", "c"]),
            vec![None, Some(0), Some(1), Some(2)]
        );
        assert_eq!(retained_indices::<u8>(&[], &[1, 2]), vec![None, None]);
        assert!(retained_indices::<u8>(&[1, 2], &[]).is_empty());
    }

    #[test]
    fn changing_item_kind_requires_a_new_node_even_with_the_same_id() {
        let old = vec![TrayEntry::check("provider", "A", true, true)];
        let new = vec![TrayEntry::label("provider", "A unavailable")];
        assert_eq!(retained(&old, &new), vec![None]);
    }

    #[test]
    fn repeated_separators_are_retained_at_most_once_each() {
        let old = vec![TrayEntry::Separator; 2];
        let new = vec![TrayEntry::Separator; 3];
        assert_eq!(retained(&old, &new), vec![Some(0), Some(1), None]);
        assert_eq!(retained(&new, &old), vec![Some(0), Some(1)]);
    }
}
