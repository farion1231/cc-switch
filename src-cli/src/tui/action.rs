use cc_switch_lib::{AppType, Provider};
use indexmap::IndexMap;

pub enum Action {
    Quit,
    FocusLeft,
    FocusRight,
    Up,
    Down,
    Select,
    Back,
    ShowHelp,
    // Async callbacks
    ProvidersLoaded(AppType, IndexMap<String, Provider>, String),
    Error(String),
}
