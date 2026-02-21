use crate::tui::action::Action;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

pub fn spawn_event_reader(tx: mpsc::UnboundedSender<Action>) {
    std::thread::spawn(move || loop {
        if let Ok(Event::Key(key)) = event::read() {
            if let Some(action) = map_key(key) {
                if tx.send(action).is_err() {
                    break;
                }
            }
        }
    });
}

fn map_key(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::Up),
        KeyCode::Char('h') | KeyCode::Left => Some(Action::FocusLeft),
        KeyCode::Char('l') | KeyCode::Right => Some(Action::FocusRight),
        KeyCode::Enter => Some(Action::Select),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('?') => Some(Action::ShowHelp),
        _ => None,
    }
}
