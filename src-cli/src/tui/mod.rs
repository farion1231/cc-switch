pub mod action;
pub mod app;
pub mod components;
pub mod event;
pub mod ui;

use anyhow::Result;
use app::App;
use cc_switch_lib::AppState;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tokio::sync::mpsc;

pub async fn run(state: AppState) -> Result<()> {
    // Panic hook: restore terminal even on crash
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let mut terminal = init()?;
    let (tx, mut rx) = mpsc::unbounded_channel();
    event::spawn_event_reader(tx.clone());
    let mut app = App::new(state, tx);
    app.load_current_app();

    while app.running {
        terminal.draw(|f| ui::render(f, &app))?;
        if let Some(action) = rx.recv().await {
            app.dispatch(action);
        }
    }

    restore(&mut terminal)
}

fn init() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
