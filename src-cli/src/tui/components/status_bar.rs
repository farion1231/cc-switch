use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, area: Rect) {
    let p = Paragraph::new("j/k:navigate  Enter:switch  h/l:focus  ?:help  q:quit")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(p, area);
}
