use crate::core::app::App;
use crate::tui::theme::ThemeExt;
use crate::ui::modals::relay_status::centered_rect;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

pub fn draw_remap_track_modal(f: &mut Frame<'_>, app: &App) {
    if !app.view.remap_modal_visible {
        return;
    }

    let theme = &app.user_config.theme;
    let area = centered_rect(72, 45, f.area());
    f.render_widget(Clear, area);

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.selected.into()))
        .style(theme.base_style())
        .title(Span::styled(
            " EchoVena \u{2014} Manual YouTube Remap [Key M] ",
            Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(main_block, area);

    let inner_area = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Song title header
            Constraint::Length(3), // Guidance note
            Constraint::Length(3), // Input box
            Constraint::Min(1),    // Shortcut hints
        ])
        .split(inner_area);

    // 1. Song Title
    let song_name = app.view.remap_track_name.as_deref().unwrap_or("Unknown Song");
    let title_para = Paragraph::new(vec![
        Line::from(Span::styled("Target Song to Remap:", Style::default().fg(theme.inactive.into()))),
        Line::from(Span::styled(format!("\u{266a} {}", song_name), Style::default().fg(theme.text.into()).add_modifier(Modifier::BOLD))),
    ]);
    f.render_widget(title_para, chunks[0]);

    // 2. Guidance Note
    let guidance_para = Paragraph::new(vec![
        Line::from(Span::styled("Enter YouTube Video ID or URL (e.g. dQw4w9WgXcQ or youtu.be/...):", Style::default().fg(theme.hint.into()))),
        Line::from(Span::styled("Mapped video will be saved to local database and used for playback.", Style::default().fg(theme.inactive.into()))),
    ]);
    f.render_widget(guidance_para, chunks[1]);

    // 3. Input Box
    let input_text: String = app.view.remap_input.iter().collect();
    let input_para = Paragraph::new(Line::from(vec![
        Span::styled(format!("> {}_", input_text), Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.header.into()))
            .title(Span::styled(" YouTube Link / Video ID ", Style::default().fg(theme.header.into()))),
    );
    f.render_widget(input_para, chunks[2]);

    // 4. Hints Footer
    let hints_para = Paragraph::new(Line::from(vec![
        Span::styled("[Enter]", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Save & Map   ", Style::default().fg(theme.text.into())),
        Span::styled("[p]", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Paste Clipboard   ", Style::default().fg(theme.text.into())),
        Span::styled("[Esc]", Style::default().fg(theme.error_text.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Cancel", Style::default().fg(theme.text.into())),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(hints_para, chunks[3]);
}
