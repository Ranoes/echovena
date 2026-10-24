use crate::core::app::App;
use crate::lyrics::{KaraokeTimeline, VocalRole};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, Paragraph},
    Frame,
};

/// Computes character-slice counts for sub-word progress highlighting
pub fn compute_sub_word_slices<'a>(word: &'a str, progress: f64) -> (&'a str, &'a str) {
    if progress <= 0.0 {
        return ("", word);
    }
    if progress >= 1.0 {
        return (word, "");
    }

    let char_indices: Vec<(usize, char)> = word.char_indices().collect();
    if char_indices.is_empty() {
        return ("", "");
    }

    let sung_char_count = (char_indices.len() as f64 * progress).round() as usize;
    if sung_char_count >= char_indices.len() {
        return (word, "");
    }
    if sung_char_count == 0 {
        return ("", word);
    }

    let split_byte = char_indices[sung_char_count].0;
    (&word[..split_byte], &word[split_byte..])
}

/// Detects if current playback is in an instrumental break (> 5000ms gap)
pub fn detect_instrumental_break(
    timeline: &KaraokeTimeline,
    current_time_ms: i64,
) -> Option<(f64, f64)> {
    if timeline.lines.is_empty() {
        return None;
    }

    // Gap before first line
    let first_line = &timeline.lines[0];
    if current_time_ms < first_line.start_time_ms {
        let gap = first_line.start_time_ms - current_time_ms;
        if first_line.start_time_ms >= 5000 && gap > 0 {
            let progress = (current_time_ms as f64 / first_line.start_time_ms as f64).clamp(0.0, 1.0);
            let remaining_secs = gap as f64 / 1000.0;
            return Some((remaining_secs, progress));
        }
    }

    // Gap between lines
    for i in 0..timeline.lines.len() - 1 {
        let curr_end = timeline.lines[i].end_time_ms;
        let next_start = timeline.lines[i + 1].start_time_ms;
        let gap = next_start - curr_end;

        if gap >= 5000 && current_time_ms >= curr_end && current_time_ms < next_start {
            let elapsed_gap = current_time_ms - curr_end;
            let progress = (elapsed_gap as f64 / gap as f64).clamp(0.0, 1.0);
            let remaining_secs = (next_start - current_time_ms) as f64 / 1000.0;
            return Some((remaining_secs, progress));
        }
    }

    None
}

pub fn draw_karaoke_stage_view(f: &mut Frame<'_>, app: &App) {
    if !app.view.karaoke_stage_visible {
        return;
    }

    let area = f.area();
    f.render_widget(Clear, area);

    let offset_ms = app.view.lyrics_view.timing_offset_ms;
    let offset_str = format!("{:+.1}s ({:+}ms)", offset_ms as f64 / 1000.0, offset_ms);

    let romanization_str = if app.view.karaoke_show_romanization {
        "ON [Key T]"
    } else {
        "OFF [Key T]"
    };

    let title_line = format!(
        " EchoVena — KARAOKE STAGE (60 FPS) | Offset: {} | Romanization: {} | [Esc/K] Exit ",
        offset_str, romanization_str
    );

    let stage_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title_line,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(stage_block, area);

    let inner_area = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };

    if inner_area.height < 5 || inner_area.width < 20 {
        return;
    }

    let timeline = match &app.view.active_karaoke_timeline {
        Some(t) if !t.lines.is_empty() => t,
        _ => {
            let waiting_text = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "♪  EchoVena Live Karaoke Stage  ♪",
                    Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Waiting for word-synced lyrics stream from relay...",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Supported: Apple Music TTML, Musixmatch RichSync, Spotify Color Lyrics, LRCLIB .elrc, NetEase .yrc",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let waiting_para = Paragraph::new(waiting_text).alignment(Alignment::Center);
            f.render_widget(waiting_para, inner_area);
            return;
        }
    };

    // Calculate current interpolated playback time T_now in milliseconds
    let current_time_ms = (app.song_progress_ms as i64) + (offset_ms as i64);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Instrumental Break Banner or Gap gauge
            Constraint::Min(4),    // Lyrics Lines
            Constraint::Length(2), // Controls footer
        ])
        .split(inner_area);

    // 1. Instrumental Break Countdown Bar
    if let Some((remaining_sec, progress)) = detect_instrumental_break(timeline, current_time_ms) {
        let label = format!("♪ ♫ [Instrumental Break — {:04.1}s] ♫ ♪", remaining_sec);
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::NONE))
            .gauge_style(
                Style::default()
                    .fg(Color::LightMagenta)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .percent((progress * 100.0) as u16)
            .label(label);
        f.render_widget(gauge, chunks[0]);
    }

    // 2. Lyrics Stage Rendering (Active line centered with 2 lines before & 2 lines after)
    let active_line_idx = timeline.active_line_index(current_time_ms).unwrap_or(0);

    let start_idx = active_line_idx.saturating_sub(2);
    let end_idx = (active_line_idx + 3).min(timeline.lines.len());

    let mut rendered_lines = Vec::new();

    for idx in start_idx..end_idx {
        let line = &timeline.lines[idx];
        let is_active = idx == active_line_idx;

        let role_style = match line.vocal_role {
            VocalRole::Lead1 => Style::default().fg(Color::Cyan),
            VocalRole::Lead2 => Style::default().fg(Color::LightMagenta),
            VocalRole::Background => Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        };

        let alignment = match line.vocal_role {
            VocalRole::Lead1 => Alignment::Left,
            VocalRole::Lead2 => Alignment::Right,
            VocalRole::Background => Alignment::Center,
        };

        if is_active {
            // Render word-by-word with live sub-word character slice sweep
            let mut spans = Vec::new();

            for word in &line.words {
                if current_time_ms >= word.end_time_ms {
                    // Sung word
                    spans.push(Span::styled(
                        &word.text,
                        role_style.add_modifier(Modifier::BOLD).fg(Color::White),
                    ));
                } else if current_time_ms <= word.start_time_ms {
                    // Unsung word
                    spans.push(Span::styled(
                        &word.text,
                        Style::default().fg(Color::DarkGray),
                    ));
                } else {
                    // Active word undergoing character-slice sweep
                    let progress = word.progress(current_time_ms);
                    let (sung, unsung) = compute_sub_word_slices(&word.text, progress);

                    spans.push(Span::styled(
                        sung,
                        role_style
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::UNDERLINED)
                            .fg(Color::Yellow),
                    ));
                    spans.push(Span::styled(
                        unsung,
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }

            rendered_lines.push(Line::from(spans).alignment(alignment));

            // Optional Romanization or Translation line
            if app.view.karaoke_show_romanization {
                if let Some(ref rom) = line.romanization.as_ref().or(line.translation.as_ref()) {
                    rendered_lines.push(
                        Line::from(Span::styled(
                            format!("  ↳ {}", rom),
                            Style::default()
                                .fg(Color::LightYellow)
                                .add_modifier(Modifier::ITALIC),
                        ))
                        .alignment(alignment),
                    );
                }
            }
        } else if idx < active_line_idx {
            // Past line
            rendered_lines.push(
                Line::from(Span::styled(&line.text, Style::default().fg(Color::DarkGray)))
                    .alignment(alignment),
            );
        } else {
            // Future upcoming line
            rendered_lines.push(
                Line::from(Span::styled(&line.text, Style::default().fg(Color::Gray)))
                    .alignment(alignment),
            );
        }

        rendered_lines.push(Line::from("")); // Spacing
    }

    let lyrics_para = Paragraph::new(rendered_lines);
    f.render_widget(lyrics_para, chunks[1]);

    // 3. Footer controls
    let footer_text = Line::from(vec![
        Span::styled("[K]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" Toggle Stage   ", Style::default().fg(Color::White)),
        Span::styled("[ [ ]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Offset -100ms   ", Style::default().fg(Color::White)),
        Span::styled("[ ] ]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Offset +100ms   ", Style::default().fg(Color::White)),
        Span::styled("[T]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(" Toggle Romanization", Style::default().fg(Color::White)),
    ]);
    let footer_para = Paragraph::new(footer_text).alignment(Alignment::Center);
    f.render_widget(footer_para, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::{KaraokeLine, SyncType};

    #[test]
    fn test_compute_sub_word_slices() {
        let (sung, unsung) = compute_sub_word_slices("Never", 0.0);
        assert_eq!(sung, "");
        assert_eq!(unsung, "Never");

        let (sung, unsung) = compute_sub_word_slices("Never", 1.0);
        assert_eq!(sung, "Never");
        assert_eq!(unsung, "");

        let (sung, unsung) = compute_sub_word_slices("Never", 0.5);
        assert_eq!(sung, "Nev");
        assert_eq!(unsung, "er");
    }

    #[test]
    fn test_detect_instrumental_break() {
        let timeline = KaraokeTimeline {
            provider: "test".to_string(),
            sync_type: SyncType::Syllable,
            lines: vec![
                KaraokeLine {
                    text: "Line 1".to_string(),
                    start_time_ms: 10_000,
                    end_time_ms: 15_000,
                    words: vec![],
                    vocal_role: VocalRole::Lead1,
                    romanization: None,
                    translation: None,
                },
                KaraokeLine {
                    text: "Line 2".to_string(),
                    start_time_ms: 25_000, // 10s gap
                    end_time_ms: 30_000,
                    words: vec![],
                    vocal_role: VocalRole::Lead1,
                    romanization: None,
                    translation: None,
                },
            ],
            raw_elrc: None,
        };

        // Before first line (starts at 10s > 5s threshold)
        let brk = detect_instrumental_break(&timeline, 2000);
        assert!(brk.is_some());
        let (rem, _) = brk.unwrap();
        assert!((rem - 8.0).abs() < 0.1);

        // In 10s gap between line 1 (ends 15s) and line 2 (starts 25s)
        let brk2 = detect_instrumental_break(&timeline, 18_000);
        assert!(brk2.is_some());
        let (rem2, prog2) = brk2.unwrap();
        assert!((rem2 - 7.0).abs() < 0.1);
        assert!((prog2 - 0.3).abs() < 0.05);

        // During line 1
        assert!(detect_instrumental_break(&timeline, 12_000).is_none());
    }
}
