use crate::core::app::App;
use crate::tui::theme::ThemeExt;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Renders the LosslessRelay telemetry status bar:
/// `[Origin: Spotify -> Relay: Qobuz]  [FLAC 24-bit / 96.0 kHz | 2304 kbps]  [Lyrics: Syllable Karaoke]`
pub fn render_status_bar(f: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.user_config.theme;

    let (origin_badge, codec_badge, lyrics_badge) = if let Some(lossless) = &app.lossless_playback {
        (
            lossless.origin_badge(),
            lossless.codec_badge(),
            lossless.lyrics_badge(),
        )
    } else {
        (
            "[Origin: Spotify -> Relay: Qobuz]".to_string(),
            "[FLAC 24-bit / 96.0 kHz | 2304 kbps]".to_string(),
            "[Lyrics: Syllable Karaoke]".to_string(),
        )
    };

    let spans = vec![
        Span::styled(
            origin_badge,
            Style::default()
                .fg(theme.selected.into())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            codec_badge,
            Style::default()
                .fg(theme.hovered.into())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            lyrics_badge,
            Style::default()
                .fg(theme.hint.into())
                .add_modifier(Modifier::BOLD),
        ),
    ];

    let paragraph = Paragraph::new(Line::from(spans))
        .style(theme.base_style())
        .block(
            Block::default()
                .borders(Borders::NONE),
        );

    f.render_widget(paragraph, area);
}
