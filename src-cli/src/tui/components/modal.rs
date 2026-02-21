use crate::tui::app::{App, Modal};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub fn render(f: &mut Frame, app: &App) {
    match &app.modal {
        Modal::None => {}
        Modal::Help => render_help(f),
        Modal::Error(msg) => render_error(f, msg),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(v[1])[1]
}

fn render_help(f: &mut Frame) {
    let area = centered_rect(60, 60, f.area());
    f.render_widget(Clear, area);

    let text = vec![
        Line::styled("Keyboard Shortcuts", Style::default().bold()),
        Line::raw(""),
        Line::raw("j/k or ↑/↓    Navigate list"),
        Line::raw("h/l or ←/→    Switch panel focus"),
        Line::raw("Enter         Confirm action"),
        Line::raw("Tab/Shift+Tab Switch view"),
        Line::raw("1/2/3         Jump to view"),
        Line::raw("Esc           Back / Close modal"),
        Line::raw("?             Show this help"),
        Line::raw("q / Ctrl+c    Quit"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Help ");
    let p = Paragraph::new(text).block(block);
    f.render_widget(p, area);
}

fn render_error(f: &mut Frame, msg: &str) {
    let area = centered_rect(50, 30, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" Error ");
    let p = Paragraph::new(msg.to_string())
        .block(block)
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}
