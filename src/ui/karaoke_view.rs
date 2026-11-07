use crate::core::app::App;
use crate::lyrics::KaraokeTimeline;
use crate::tui::theme::ThemeExt;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

/// Computes character-slice counts for sub-word progress highlighting
pub fn compute_sub_word_slices(word: &str, progress: f64) -> (&str, &str) {
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

fn get_now_playing_info(app: &App) -> (String, String, String, u64, u64, bool) {
    if let Some(snapshot) = crate::infra::media_metadata::current_playback_snapshot(app) {
        let artist = snapshot.primary_artist();
        let title = snapshot.metadata.title.clone();
        let album = snapshot.metadata.album.clone();
        let pos_ms = snapshot.progress_ms as u64;
        let dur_ms = snapshot.metadata.duration_ms as u64;
        let is_playing = snapshot.is_playing;
        return (title, artist, album, pos_ms, dur_ms, is_playing);
    }

    #[cfg(feature = "youtube")]
    if let Some(yt) = app.youtube_playback.as_ref() {
        if let Some(track) = yt.current() {
            let pos_ms = yt.player.position().as_millis() as u64;
            let dur_ms = track.duration_ms.max(1);
            let artist = if track.artists.is_empty() {
                "Unknown Artist".to_string()
            } else {
                track.artists.join(", ")
            };
            let album = if track.album.is_empty() {
                "YouTube".to_string()
            } else {
                track.album.clone()
            };
            let is_playing = !yt.player.is_paused();
            return (track.name.clone(), artist, album, pos_ms, dur_ms, is_playing);
        }
    }

    if let Some(lossless) = app.lossless_playback.as_ref() {
        if let Some(ref t) = lossless.current_track {
            let dur_ms = t.duration_ms.max(1) as u64;
            return (
                t.clean_title.clone(),
                t.artist.clone(),
                t.album.clone().unwrap_or_else(|| "EchoVena Archive".to_string()),
                app.song_progress_ms as u64,
                dur_ms,
                lossless.is_playing,
            );
        }
    }

    ("No track playing".to_string(), "EchoVena".to_string(), "".to_string(), 0, 1, false)
}

fn get_single_next_up_track(app: &App) -> Option<(String, String, u64)> {
    // 1. Native queue (manually queued tracks)
    if let Some(track) = app.native_queue.first() {
        let artist = if track.artists.is_empty() {
            "Unknown Artist".to_string()
        } else {
            track.artists.join(", ")
        };
        return Some((track.name.clone(), artist, track.duration_ms / 1000));
    }

    // 2. YouTube playback playlist upcoming
    #[cfg(feature = "youtube")]
    if let Some(yt) = app.youtube_playback.as_ref() {
        if let Some(track) = yt.tracks.get(yt.index + 1) {
            let artist = if track.artists.is_empty() {
                "Unknown Artist".to_string()
            } else {
                track.artists.join(", ")
            };
            return Some((track.name.clone(), artist, track.duration_ms / 1000));
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

    // Split screen: Left = Lyrics Stage (~55%), Right = Now Playing Dashboard (~45%)
    let (lyrics_area, right_area) = if area.width >= 90 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);
        (cols[0], cols[1])
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        (rows[0], rows[1])
    };

    // =========================================================================
    // 1. RIGHT COLUMN: Now Playing (Art + Info + Clean Playbar) & Up Next
    // =========================================================================
    let (title, artist, album, progress_ms, duration_ms, is_playing) = get_now_playing_info(app);
    let next_up = get_single_next_up_track(app);

    let right_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(12),   // Top: Art + Info + Playbar
            Constraint::Length(3), // Bottom: Tiny Up Next Box (1 next track)
        ])
        .split(right_area);

    let theme = &app.user_config.theme;

    // 1A. Now Playing Box
    f.render_widget(Clear, right_sections[0]);
    let np_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.header.into()))
        .style(theme.base_style())
        .title(Span::styled(" Now Playing ", Style::default().fg(theme.header.into()).add_modifier(Modifier::BOLD)));
    let np_inner = np_block.inner(right_sections[0]);
    f.render_widget(np_block, right_sections[0]);

    let np_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),    // Artwork
            Constraint::Length(2), // Title & Artist/Album
            Constraint::Length(2), // Clean Playbar without hotkey text
        ])
        .split(np_inner);

    // Render Album Artwork
    let bounds = np_layout[0];
    f.render_widget(Clear, bounds);
    let available_image_size = Rect::new(
        0,
        0,
        bounds.width.saturating_sub(2),
        bounds.height.saturating_sub(1),
    );

    #[cfg(feature = "cover-art")]
    {
        if app.cover_art.available() {
            let fitted_size = crate::tui::cover_art::fullscreen_size_for(available_image_size, &app.cover_art)
                .unwrap_or_else(|| {
                    let art_h = bounds.height.saturating_sub(1);
                    let art_w = (art_h * 2).min(bounds.width.saturating_sub(2));
                    Rect::new(0, 0, art_w, art_h)
                });
            let art_rect = crate::tui::layout::center_rect_within(bounds, fitted_size);
            crate::tui::cover_art::render_fullscreen(f, art_rect, &app.cover_art, &app.user_config);
        } else {
            let art_h = bounds.height.saturating_sub(1).min(6);
            let art_w = (art_h * 2).min(bounds.width.saturating_sub(2));
            let art_rect = crate::tui::layout::center_rect_within(bounds, Rect::new(0, 0, art_w, art_h));
            let placeholder = vec![
                Line::from(""),
                Line::from(Span::styled("┌───────────────────┐", Style::default().fg(theme.inactive.into()))),
                Line::from(Span::styled("│   ♫  EchoVena  ♫  │", Style::default().fg(theme.selected.into()))),
                Line::from(Span::styled("│   ●───────────●   │", Style::default().fg(theme.hint.into()))),
                Line::from(Span::styled("│     ALBUM ART     │", Style::default().fg(theme.text.into()))),
                Line::from(Span::styled("└───────────────────┘", Style::default().fg(theme.inactive.into()))),
            ];
            f.render_widget(Paragraph::new(placeholder).alignment(Alignment::Center), art_rect);
        }
    }
    #[cfg(not(feature = "cover-art"))]
    {
        let art_h = bounds.height.saturating_sub(1).min(6);
        let art_w = (art_h * 2).min(bounds.width.saturating_sub(2));
        let art_rect = crate::tui::layout::center_rect_within(bounds, Rect::new(0, 0, art_w, art_h));
        let placeholder = vec![
            Line::from(""),
            Line::from(Span::styled("♪ ♫ [EchoVena HD Art] ♫ ♪", Style::default().fg(theme.selected.into()))),
        ];
        f.render_widget(Paragraph::new(placeholder).alignment(Alignment::Center), art_rect);
    }

    // Song Info (Title, Artist, Album)
    let album_display = if album.is_empty() { String::new() } else { format!(" — {}", album) };
    let info_text = vec![
        Line::from(Span::styled(&title, Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(format!("{}{}", artist, album_display), Style::default().fg(theme.hint.into()))),
    ];
    f.render_widget(Paragraph::new(info_text).alignment(Alignment::Center), np_layout[1]);

    // Clean Playbar (Without hotkeys)
    let cur_min = progress_ms / 60_000;
    let cur_sec = (progress_ms / 1_000) % 60;
    let tot_min = duration_ms / 60_000;
    let tot_sec = (duration_ms / 1_000) % 60;
    let rem_ms = duration_ms.saturating_sub(progress_ms);
    let rem_min = rem_ms / 60_000;
    let rem_sec = (rem_ms / 1_000) % 60;

    let progress_ratio = (progress_ms as f64 / duration_ms.max(1) as f64).clamp(0.0, 1.0);
    let bar_width = (np_layout[2].width as usize).saturating_sub(24).max(6);
    let filled = (bar_width as f64 * progress_ratio).round() as usize;
    let empty = bar_width.saturating_sub(filled);

    let filled_str = if filled > 0 { "━".repeat(filled - 1) } else { String::new() };
    let empty_str = "─".repeat(empty);

    let progress_line = format!(
        "{:02}:{:02} {}●{} {:02}:{:02} (-{:02}:{:02})",
        cur_min, cur_sec,
        filled_str,
        empty_str,
        tot_min, tot_sec,
        rem_min, rem_sec,
    );

    let state_str = if is_playing { "▶ Playing" } else { "⏸ Paused" };
    let status_line = format!("{}  •  Opus 160k  •  Vol {}%", state_str, app.desired_volume());

    let playbar_widget = Paragraph::new(vec![
        Line::from(Span::styled(progress_line, Style::default().fg(theme.playbar_progress.into()))),
        Line::from(Span::styled(status_line, Style::default().fg(theme.inactive.into()))),
    ])
    .alignment(Alignment::Center);
    f.render_widget(playbar_widget, np_layout[2]);

    // 1B. Tiny Up Next Box (Only 1 next track)
    f.render_widget(Clear, right_sections[1]);
    let next_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.hovered.into()))
        .style(theme.base_style())
        .title(Span::styled(" Up Next ", Style::default().fg(theme.hovered.into()).add_modifier(Modifier::BOLD)));
    let next_inner = next_block.inner(right_sections[1]);
    f.render_widget(next_block, right_sections[1]);

    let next_line = if let Some((n_title, n_artist, n_dur)) = next_up {
        let mins = n_dur / 60;
        let secs = n_dur % 60;
        Line::from(vec![
            Span::styled(" ⏭  ", Style::default().fg(theme.hovered.into()).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} — {} ", n_title, n_artist), Style::default().fg(theme.text.into())),
            Span::styled(format!("[{:02}:{:02}]", mins, secs), Style::default().fg(theme.inactive.into())),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ⏭  ", Style::default().fg(theme.inactive.into())),
            Span::styled("[Auto-Play Radio] Discovering similar track...", Style::default().fg(theme.inactive.into()).add_modifier(Modifier::ITALIC)),
        ])
    };
    f.render_widget(Paragraph::new(next_line).alignment(Alignment::Left), next_inner);

    // =========================================================================
    // 2. LEFT COLUMN: Lyrics & Karaoke Stage (Shared with Dedicated Lyrics View )
    // =========================================================================
    f.render_widget(Clear, lyrics_area);
    crate::tui::ui::lyrics::draw_lyrics(f, app, lyrics_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::{KaraokeLine, SyncType, VocalRole};

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
