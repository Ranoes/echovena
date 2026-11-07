use crate::core::app::{App, SyncAuthModalType};
use crate::tui::theme::ThemeExt;
use crate::ui::modals::relay_status::centered_rect;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

pub fn draw_sync_auth_modal(f: &mut Frame<'_>, app: &App) {
    let modal_type = match app.view.sync_auth_modal {
        Some(m) => m,
        None => return,
    };

    let theme = &app.user_config.theme;
    let area = centered_rect(72, 38, f.area());
    f.render_widget(Clear, area);

    let (border_color, title_text): (ratatui::style::Color, &str) = match modal_type {
        SyncAuthModalType::SpotifySpDc => (
            theme.selected.into(),
            " 🔐 Spotify sp_dc Authentication Cookie [Key C] ",
        ),
        SyncAuthModalType::YouTubeCookies => (
            theme.error_border.into(),
            " 🔐 YouTube Cookies Configuration [Key Y] ",
        ),
    };

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(theme.base_style())
        .title(Span::styled(
            title_text,
            Style::default().fg(border_color).add_modifier(Modifier::BOLD),
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
            Constraint::Length(4), // Instructions
            Constraint::Length(3), // Input box
            Constraint::Length(2), // Status feedback
            Constraint::Min(1),    // Shortcut hints
        ])
        .split(inner_area);

    // 1. Guidance Section
    let guidance_lines = match modal_type {
        SyncAuthModalType::SpotifySpDc => vec![
            Line::from(Span::styled(
                "Enter your personal Spotify 'sp_dc' cookie for cloud playlist sync:",
                Style::default().fg(theme.text.into()).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "How to find: Open open.spotify.com -> DevTools (F12) -> Application -> Cookies -> 'sp_dc'",
                Style::default().fg(theme.inactive.into()),
            )),
        ],
        SyncAuthModalType::YouTubeCookies => vec![
            Line::from(Span::styled(
                "Enter YouTube cookies text or full path to cookies.txt:",
                Style::default().fg(theme.text.into()).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "How to export: Use browser extension 'Get cookies.txt LOCALLY' on youtube.com",
                Style::default().fg(theme.inactive.into()),
            )),
        ],
    };
    f.render_widget(Paragraph::new(guidance_lines), chunks[0]);

    // 2. Input Box
    let input_str: String = app.view.sync_auth_input.iter().collect();
    let display_str = if input_str.is_empty() {
        Span::styled(
            "Paste cookie or path here (press 'p' or Ctrl+v to paste)...",
            Style::default().fg(theme.inactive.into()),
        )
    } else {
        // Mask sp_dc cookie partially for privacy if long
        let display_val = if modal_type == SyncAuthModalType::SpotifySpDc && input_str.len() > 16 {
            format!("{}...{} ({} chars)", &input_str[..8], &input_str[input_str.len() - 4..], input_str.len())
        } else {
            input_str.clone()
        };
        Span::styled(
            display_val,
            Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD),
        )
    };

    let input_box = Paragraph::new(Line::from(vec![
        Span::raw(" > "),
        display_str,
        Span::styled("█", Style::default().fg(theme.text.into())),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(input_box, chunks[1]);

    // 3. Status feedback
    let feedback_line = Line::from(vec![
        Span::styled("Stored securely in encrypted local vault / providers.json", Style::default().fg(theme.inactive.into())),
    ]);
    f.render_widget(Paragraph::new(feedback_line), chunks[2]);

    // 4. Action hints
    let hints = Line::from(vec![
        Span::styled("[Enter]", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Save & Connect   ", Style::default().fg(theme.text.into())),
        Span::styled("[p / Ctrl+v]", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Paste   ", Style::default().fg(theme.text.into())),
        Span::styled("[Esc]", Style::default().fg(theme.error_text.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" Cancel", Style::default().fg(theme.text.into())),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[3]);
}
