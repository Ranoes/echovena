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

pub fn draw_url_import_modal(f: &mut Frame<'_>, app: &App) {
    if !app.view.url_import_visible {
        return;
    }

    let theme = &app.user_config.theme;
    let area = centered_rect(70, 48, f.area());
    f.render_widget(Clear, area);

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.selected.into()))
        .style(theme.base_style())
        .title(Span::styled(
            " EchoVena — URL Import Engine (Spotify & YouTube) [Key U] ",
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
            Constraint::Length(5), // Supported URLs guidance
            Constraint::Length(3), // Input box
            Constraint::Length(3), // Status / feedback message
            Constraint::Min(1),    // Shortcut hints
        ])
        .split(inner_area);

    // 1. Guidance Section
    let guidance_text = vec![
        Line::from(vec![
            Span::styled("Enter or paste a Spotify or YouTube link to import into EchoVena:", Style::default().fg(theme.text.into())),
        ]),
        Line::from(vec![
            Span::styled("• Spotify:  ", Style::default().fg(theme.hovered.into()).add_modifier(Modifier::BOLD)),
            Span::styled("https://open.spotify.com/playlist/...  or  spotify:playlist:...", Style::default().fg(theme.inactive.into())),
        ]),
        Line::from(vec![
            Span::styled("• YouTube:  ", Style::default().fg(theme.error_text.into()).add_modifier(Modifier::BOLD)),
            Span::styled("https://music.youtube.com/playlist?list=...  or  youtu.be/...", Style::default().fg(theme.inactive.into())),
        ]),
    ];
    let guidance_para = Paragraph::new(guidance_text).alignment(Alignment::Left);
    f.render_widget(guidance_para, chunks[0]);

    // 2. Input Box
    let input_str: String = app.view.url_import_input.iter().collect();
    let display_str = if input_str.is_empty() {
        Span::styled(
            "Paste URL here (e.g. https://open.spotify.com/playlist/...)...",
            Style::default().fg(theme.inactive.into()),
        )
    } else {
        Span::styled(&input_str, Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD))
    };

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if app.view.url_import_in_progress {
            Style::default().fg(theme.inactive.into())
        } else {
            Style::default().fg(theme.header.into())
        })
        .title(Span::styled(" Playlist / Track URL ", Style::default().fg(theme.header.into())));

    let input_para = Paragraph::new(Line::from(vec![display_str]))
        .block(input_block)
        .alignment(Alignment::Left);
    f.render_widget(input_para, chunks[1]);

    // 3. Status Message
    let status_line = if app.view.url_import_in_progress {
        Line::from(vec![
            Span::styled("⏳ ", Style::default().fg(theme.hint.into())),
            Span::styled("Importing tracks and launching background lossless resolution...", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
        ])
    } else if let Some(ref status) = app.view.url_import_status {
        let is_err = status.to_lowercase().contains("error") || status.to_lowercase().contains("failed") || status.to_lowercase().contains("invalid");
        let color: ratatui::style::Color = if is_err { theme.error_text.into() } else { theme.hovered.into() };
        Line::from(vec![
            Span::styled(if is_err { "❌ " } else { "✔ " }, Style::default().fg(color)),
            Span::styled(status.as_str(), Style::default().fg(color)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Ready. Press [Enter] to import.", Style::default().fg(theme.inactive.into())),
        ])
    };

    let status_para = Paragraph::new(status_line).alignment(Alignment::Left);
    f.render_widget(status_para, chunks[2]);

    // 4. Footer shortcuts
    let shortcuts = Line::from(vec![
        Span::styled("[Enter]", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Import   ", Style::default().fg(theme.text.into())),
        Span::styled("[Ctrl+U]", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Clear   ", Style::default().fg(theme.text.into())),
        Span::styled("[Esc / q]", Style::default().fg(theme.header.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Close", Style::default().fg(theme.text.into())),
    ]);
    let footer_para = Paragraph::new(shortcuts).alignment(Alignment::Center);
    f.render_widget(footer_para, chunks[3]);
}
