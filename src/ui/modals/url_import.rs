use crate::core::app::App;
use crate::ui::modals::relay_status::centered_rect;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

pub fn draw_url_import_modal(f: &mut Frame<'_>, app: &App) {
    if !app.view.url_import_visible {
        return;
    }

    let area = centered_rect(70, 48, f.area());
    f.render_widget(Clear, area);

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::LightGreen))
        .title(Span::styled(
            " EchoVena — URL Import Engine (Spotify & YouTube) [Key U] ",
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
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
            Span::styled("Enter or paste a Spotify or YouTube link to import into EchoVena:", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("• Spotify:  ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("https://open.spotify.com/playlist/...  or  spotify:playlist:...", Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("• YouTube:  ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled("https://music.youtube.com/playlist?list=...  or  youtu.be/...", Style::default().fg(Color::Gray)),
        ]),
    ];
    let guidance_para = Paragraph::new(guidance_text).alignment(Alignment::Left);
    f.render_widget(guidance_para, chunks[0]);

    // 2. Input Box
    let input_str: String = app.view.url_import_input.iter().collect();
    let display_str = if input_str.is_empty() {
        Span::styled(
            "Paste URL here (e.g. https://open.spotify.com/playlist/...)...",
            Style::default().fg(Color::DarkGray),
        )
    } else {
        Span::styled(&input_str, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    };

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if app.view.url_import_in_progress {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Cyan)
        })
        .title(Span::styled(" Playlist / Track URL ", Style::default().fg(Color::Cyan)));

    let input_para = Paragraph::new(Line::from(vec![display_str]))
        .block(input_block)
        .alignment(Alignment::Left);
    f.render_widget(input_para, chunks[1]);

    // 3. Status Message
    let status_line = if app.view.url_import_in_progress {
        Line::from(vec![
            Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
            Span::styled("Importing tracks and launching background lossless resolution...", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ])
    } else if let Some(ref status) = app.view.url_import_status {
        let is_err = status.to_lowercase().contains("error") || status.to_lowercase().contains("failed") || status.to_lowercase().contains("invalid");
        let color = if is_err { Color::LightRed } else { Color::LightGreen };
        Line::from(vec![
            Span::styled(if is_err { "❌ " } else { "✔ " }, Style::default().fg(color)),
            Span::styled(status.as_str(), Style::default().fg(color)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Ready. Press [Enter] to import.", Style::default().fg(Color::DarkGray)),
        ])
    };

    let status_para = Paragraph::new(status_line).alignment(Alignment::Left);
    f.render_widget(status_para, chunks[2]);

    // 4. Footer shortcuts
    let shortcuts = Line::from(vec![
        Span::styled("[Enter]", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        Span::styled(" Import   ", Style::default().fg(Color::White)),
        Span::styled("[Ctrl+U]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" Clear   ", Style::default().fg(Color::White)),
        Span::styled("[Esc / q]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Close", Style::default().fg(Color::White)),
    ]);
    let footer_para = Paragraph::new(shortcuts).alignment(Alignment::Center);
    f.render_widget(footer_para, chunks[3]);
}
