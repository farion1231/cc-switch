use crate::tui::app::{App, Focus};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Detail;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let Some((_id, p)) = app.selected_provider_entry() else {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Detail ");
        f.render_widget(block, area);
        return;
    };

    // Split: fixed info top, JSON preview bottom
    let chunks = Layout::vertical([
        Constraint::Length(info_height(p)),
        Constraint::Fill(1),
    ])
    .split(area);

    // --- Top: structured fields ---
    let mut lines = vec![
        field_line("Name", &p.name),
        field_line("ID", &p.id),
    ];
    if let Some(url) = &p.website_url {
        lines.push(field_line("URL", url));
    }
    if let Some(cat) = &p.category {
        lines.push(field_line("Category", cat));
    }
    if let Some(notes) = &p.notes {
        lines.push(field_line("Notes", notes));
    }
    if p.in_failover_queue {
        lines.push(field_line("Failover", "Yes"));
    }

    let info_block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Info ");
    let info = Paragraph::new(lines).block(info_block);
    f.render_widget(info, chunks[0]);

    // --- Bottom: JSON preview with scroll ---
    let json = serde_json::to_string_pretty(&p.settings_config).unwrap_or_default();
    let json_block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Settings JSON ");
    let json_para = Paragraph::new(json)
        .block(json_block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(json_para, chunks[1]);
}

fn field_line<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(Color::Yellow)),
        Span::raw(value),
    ])
}

fn info_height(p: &cc_switch_lib::Provider) -> u16 {
    let mut h: u16 = 2; // Name + ID
    if p.website_url.is_some() { h += 1; }
    if p.category.is_some() { h += 1; }
    if p.notes.is_some() { h += 1; }
    if p.in_failover_queue { h += 1; }
    h + 2 // +2 for border
}
