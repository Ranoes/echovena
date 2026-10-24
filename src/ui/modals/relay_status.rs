use crate::config::ProviderRegistry;
use crate::core::app::App;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Row, Table},
    Frame,
};

/// Helper to center a rect with given width and height percentages
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn draw_relay_status_modal(f: &mut Frame<'_>, app: &App) {
    if !app.view.relay_status_visible {
        return;
    }

    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);

    let main_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " EchoVena — Provider & Lossless Relay Health (Press 'R' or 'Esc' to close) ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
            Constraint::Length(3), // Telemetry summary banner
            Constraint::Length(8), // Audio Providers table
            Constraint::Min(8),    // Lyric Providers table
            Constraint::Length(1), // Footer hotkeys
        ])
        .split(inner_area);

    let registry = ProviderRegistry::load_or_create_default().unwrap_or_default();

    // 1. Telemetry Summary Banner
    let summary_lines = vec![
        Line::from(vec![
            Span::styled(" [Lossless Engine: Active] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(" [Concurrency: Semaphore(1)] ", Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(format!(" [Timeout: {}ms] ", registry.waterfall_timeout_ms), Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled(" [Queue: Idle / Position: #0] ", Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::styled(format!(" Quality: {} ", app.user_config.lossless_relay.prefer_quality), Style::default().fg(Color::Cyan)),
            Span::raw(" | "),
            Span::styled(format!(" Atmos Excluded: {} ", app.user_config.lossless_relay.exclude_dolby_atmos), Style::default().fg(Color::DarkGray)),
            Span::raw(" | "),
            Span::styled(format!(" Pre-resolve: {}s ", app.user_config.lossless_relay.pre_resolve_next_track_seconds), Style::default().fg(Color::DarkGray)),
            Span::raw(" | "),
            Span::styled(format!(" Auth Mode: {} ", app.user_config.lossless_relay.auth_mode), Style::default().fg(Color::DarkGray)),
        ]),
    ];
    let summary_para = Paragraph::new(summary_lines).block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(summary_para, chunks[0]);

    // 2. Audio Providers Table
    let audio_header = Row::new(vec![
        "Pri",
        "ID",
        "Provider Type",
        "Status",
        "Formats",
        "Target Endpoint",
    ])
    .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let mut audio_rows = Vec::new();
    for p in &registry.audio_providers {
        let status_span = if p.enabled {
            Span::styled("Active", Style::default().fg(Color::Green))
        } else {
            Span::styled("Disabled", Style::default().fg(Color::DarkGray))
        };
        let endpoint = p
            .base_url
            .as_deref()
            .or(p.playback_url.as_deref())
            .unwrap_or("builtin CLI");
        let formats = if p.supported_formats.is_empty() {
            "-".to_string()
        } else {
            p.supported_formats.join(", ")
        };

        audio_rows.push(Row::new(vec![
            Span::raw(p.priority.to_string()),
            Span::styled(&p.id, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&p.provider_type),
            status_span,
            Span::raw(formats),
            Span::styled(endpoint, Style::default().fg(Color::LightCyan)),
        ]));
    }

    let audio_table = Table::new(
        audio_rows,
        [
            Constraint::Length(4),
            Constraint::Length(22),
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(24),
            Constraint::Min(20),
        ],
    )
    .header(audio_header)
    .block(
        Block::default()
            .title(Span::styled(" Lossless Audio Relays (Priority Waterfall) ", Style::default().add_modifier(Modifier::BOLD)))
            .borders(Borders::BOTTOM),
    );
    f.render_widget(audio_table, chunks[1]);

    // 3. Lyric Providers Table
    let lyric_header = Row::new(vec!["Pri", "ID", "Provider Type", "Status", "Base URL"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let mut lyric_rows = Vec::new();
    for p in &registry.lyric_providers {
        let status_span = if p.enabled {
            Span::styled("Active", Style::default().fg(Color::Green))
        } else {
            Span::styled("Disabled", Style::default().fg(Color::DarkGray))
        };

        lyric_rows.push(Row::new(vec![
            Span::raw(p.priority.to_string()),
            Span::styled(&p.id, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&p.provider_type),
            status_span,
            Span::styled(&p.base_url, Style::default().fg(Color::LightCyan)),
        ]));
    }

    let lyric_table = Table::new(
        lyric_rows,
        [
            Constraint::Length(4),
            Constraint::Length(32),
            Constraint::Length(24),
            Constraint::Length(10),
            Constraint::Min(25),
        ],
    )
    .header(lyric_header)
    .block(
        Block::default()
            .title(Span::styled(" Multi-Source Karaoke & Lyrics Waterfall ", Style::default().add_modifier(Modifier::BOLD))),
    );
    f.render_widget(lyric_table, chunks[2]);

    // 4. Footer Hotkeys
    let footer_text = Line::from(vec![
        Span::styled(" [R] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Toggle Health Modal  "),
        Span::styled(" [Esc] / [q] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Close Modal  "),
        Span::styled(" [providers.json] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("Dynamic hot-swap enabled"),
    ]);
    let footer_para = Paragraph::new(footer_text).alignment(Alignment::Center);
    f.render_widget(footer_para, chunks[3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_centered_rect_math() {
        let parent = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let centered = centered_rect(80, 60, parent);
        assert_eq!(centered.width, 80);
        assert_eq!(centered.height, 60);
        assert_eq!(centered.x, 10);
        assert_eq!(centered.y, 20);
    }
}
