//! Terminal UI (TUI) for interactive provider management.

mod app;
mod event;
mod form;
mod input;
mod theme;
mod ui;

use std::io::stdout;
use std::time::Duration;

use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::store::AppState;

use app::App;
use event::{Event, EventHandler};

pub fn run_tui(state: &AppState) -> Result<(), String> {
    let mut terminal_guard = TerminalGuard::new()?;
    let mut app = App::new(state)?;
    let mut events = EventHandler::new(Duration::from_millis(200));

    while !app.should_quit() {
        terminal_guard
            .terminal
            .draw(|frame| ui::render(frame, &app))
            .map_err(|e| format!("Failed to draw UI: {e}"))?;

        match events.next()? {
            Event::Tick => {
                app.on_tick();
            }
            Event::Resize(_, _) => {
                terminal_guard
                    .terminal
                    .clear()
                    .map_err(|e| format!("Failed to clear terminal: {e}"))?;
            }
            Event::Key(key) => {
                app.handle_key(state, key)?;
            }
        }
    }

    Ok(())
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self, String> {
        enable_raw_mode().map_err(|e| format!("Failed to enable raw mode: {e}"))?;
        execute!(stdout(), EnterAlternateScreen)
            .map_err(|e| format!("Failed to enter alternate screen: {e}"))?;

        let backend = CrosstermBackend::new(stdout());
        let mut terminal =
            Terminal::new(backend).map_err(|e| format!("Terminal init error: {e}"))?;
        terminal
            .clear()
            .map_err(|e| format!("Failed to clear terminal: {e}"))?;

        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
