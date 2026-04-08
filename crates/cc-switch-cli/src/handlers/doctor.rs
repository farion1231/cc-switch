use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use cc_switch_core::{AppState, AppType, DoctorService};

pub async fn handle(
    apps: Vec<String>,
    latest: bool,
    check_updates: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let apps = if apps.is_empty() {
        None
    } else {
        Some(
            apps.into_iter()
                .map(|app| parse_app_type(&app))
                .collect::<anyhow::Result<Vec<AppType>>>()?,
        )
    };
    let report = DoctorService::inspect(&state.db, apps, latest, check_updates).await?;
    printer.print_value(&report)
}
