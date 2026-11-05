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

    let offset_ms = app.view.lyrics_view.timing_offset_ms;
    let offset_str = format!("{:+.1}s ({:+}ms)", offset_ms as f64 / 1000.0, offset_ms);

    let mode_str = match app.view.karaoke_display_mode {
        crate::core::app::LyricsDisplayMode::Original => "Orig",
        crate::core::app::LyricsDisplayMode::Romanized => "Rom",
        crate::core::app::LyricsDisplayMode::Both => "Both",
    };
    let provider_str = app
        .view
        .active_karaoke_timeline
        .as_ref()
        .map(|tl| tl.provider.as_str())
        .unwrap_or("Auto");

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
            Constraint::Min(14),   // Top: Art + Info + Playbar
            Constraint::Length(4), // Bottom: Tiny Up Next Box (1 next track)
        ])
        .split(right_area);

    // 1A. Now Playing Box
    let np_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(" Now Playing ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    let np_inner = np_block.inner(right_sections[0]);
    f.render_widget(np_block, right_sections[0]);

    let np_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),    // Artwork
            Constraint::Length(3), // Title & Artist/Album
            Constraint::Length(3), // Clean Playbar without hotkey text
        ])
        .split(np_inner);

    // Render Album Artwork
    let art_h = np_layout[0].height.saturating_sub(1);
    let art_w = (art_h * 2).min(np_layout[0].width.saturating_sub(2));
    let x_offset = np_layout[0].x + (np_layout[0].width.saturating_sub(art_w)) / 2;
    let art_rect = Rect {
        x: x_offset,
        y: np_layout[0].y,
        width: art_w,
        height: art_h,
    };

    #[cfg(feature = "cover-art")]
    {
        if app.cover_art.available() {
            crate::tui::cover_art::render_fullscreen(f, art_rect, &app.cover_art, &app.user_config);
        } else {
            let placeholder = vec![
                Line::from(""),
                Line::from(Span::styled("┌───────────────────┐", Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled("│   ♫  EchoVena  ♫  │", Style::default().fg(Color::Cyan))),
                Line::from(Span::styled("│   ●───────────●   │", Style::default().fg(Color::Yellow))),
                Line::from(Span::styled("│     ALBUM ART     │", Style::default().fg(Color::White))),
                Line::from(Span::styled("└───────────────────┘", Style::default().fg(Color::DarkGray))),
            ];
            f.render_widget(Paragraph::new(placeholder).alignment(Alignment::Center), art_rect);
        }
    }
    #[cfg(not(feature = "cover-art"))]
    {
        let placeholder = vec![
            Line::from(""),
            Line::from(Span::styled("♪ ♫ [EchoVena HD Art] ♫ ♪", Style::default().fg(Color::Cyan))),
        ];
        f.render_widget(Paragraph::new(placeholder).alignment(Alignment::Center), np_layout[0]);
    }

    // Song Info (Title, Artist, Album)
    let album_display = if album.is_empty() { String::new() } else { format!(" — {}", album) };
    let info_text = vec![
        Line::from(Span::styled(&title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(format!("{}{}", artist, album_display), Style::default().fg(Color::LightYellow))),
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
        Line::from(Span::styled(progress_line, Style::default().fg(Color::LightCyan))),
        Line::from(Span::styled(status_line, Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center);
    f.render_widget(playbar_widget, np_layout[2]);

    // 1B. Tiny Up Next Box (Only 1 next track)
    let next_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::LightGreen))
        .title(Span::styled(" Up Next ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)));
    let next_inner = next_block.inner(right_sections[1]);
    f.render_widget(next_block, right_sections[1]);

    let next_line = if let Some((n_title, n_artist, n_dur)) = next_up {
        let mins = n_dur / 60;
        let secs = n_dur % 60;
        Line::from(vec![
            Span::styled(" ⏭  ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} — {} ", n_title, n_artist), Style::default().fg(Color::White)),
            Span::styled(format!("[{:02}:{:02}]", mins, secs), Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ⏭  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Auto-Play Radio] Discovering similar track...", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
        ])
    };
    f.render_widget(Paragraph::new(next_line).alignment(Alignment::Left), next_inner);

    // =========================================================================
    // 2. LEFT COLUMN: Lyrics & Karaoke Stage
    // =========================================================================
    let lyrics_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            format!(" Lyrics & Karaoke │ {} │ Mode: {} │ Offset: {} ", provider_str, mode_str, offset_str),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let lyrics_inner = lyrics_block.inner(lyrics_area);
    f.render_widget(lyrics_block, lyrics_area);

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
            f.render_widget(waiting_para, lyrics_inner);
            return;
        }
    };

    // Calculate current interpolated playback time T_now in milliseconds
    #[cfg(feature = "youtube")]
    let current_time_ms = if let Some(yt) = app.youtube_playback.as_ref() {
        (yt.player.position().as_millis() as i64) + (offset_ms as i64)
    } else {
        (app.song_progress_ms as i64) + (offset_ms as i64)
    };
    #[cfg(not(feature = "youtube"))]
    let current_time_ms = (app.song_progress_ms as i64) + (offset_ms as i64);

    let break_info = detect_instrumental_break(timeline, current_time_ms);
    let break_height = if break_info.is_some() { 3 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(break_height), // Instrumental Break Banner or Gap gauge
            Constraint::Min(4),               // Lyrics Lines
            Constraint::Length(2),            // Controls footer
        ])
        .split(lyrics_inner);

    // 1. Instrumental Break Countdown Bar (only rendered when active)
    if let Some((remaining_sec, progress)) = break_info {
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

    // 2. Lyrics Stage Rendering (Centered in middle of view with auto-scroll)
    let active_line_idx = timeline.active_line_index(current_time_ms).unwrap_or(0);
    let available_lines = chunks[1].height as usize;
    let mid_offset = available_lines / 2;

    let mut rendered_lines = Vec::new();
    let mut active_row = 0;

    // Pad top so initial lines can rest centered in the middle of the screen
    for _ in 0..mid_offset {
        rendered_lines.push(Line::from(""));
    }

    for (idx, line) in timeline.lines.iter().enumerate() {
        let is_active = idx == active_line_idx;
        if is_active {
            active_row = rendered_lines.len();
        }

        let role_style = match line.vocal_role {
            VocalRole::Lead1 => Style::default().fg(Color::Cyan),
            VocalRole::Lead2 => Style::default().fg(Color::LightMagenta),
            VocalRole::Background => Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        };

        let alignment = app.view.karaoke_alignment.to_ratatui();
        let display_mode = app.view.karaoke_display_mode;

        if is_active {
            match display_mode {
                crate::core::app::LyricsDisplayMode::Original => {
                    let mut spans = Vec::new();
                    if line.words.is_empty() {
                        spans.push(Span::styled(
                            &line.text,
                            role_style.add_modifier(Modifier::BOLD).fg(Color::Yellow),
                        ));
                    } else {
                        for word in &line.words {
                            if current_time_ms >= word.end_time_ms {
                                spans.push(Span::styled(
                                    &word.text,
                                    role_style.add_modifier(Modifier::BOLD).fg(Color::White),
                                ));
                            } else if current_time_ms >= word.start_time_ms {
                                spans.push(Span::styled(
                                    &word.text,
                                    role_style
                                        .add_modifier(Modifier::BOLD)
                                        .add_modifier(Modifier::UNDERLINED)
                                        .fg(Color::Yellow),
                                ));
                            } else {
                                spans.push(Span::styled(
                                    &word.text,
                                    Style::default().fg(Color::DarkGray),
                                ));
                            }
                        }
                    }
                    rendered_lines.push(Line::from(spans).alignment(alignment));
                }
                crate::core::app::LyricsDisplayMode::Romanized => {
                    let mut spans = Vec::new();
                    if line.words.is_empty() {
                        let text = line.romanization.as_deref().unwrap_or(&line.text);
                        spans.push(Span::styled(
                            text,
                            role_style.add_modifier(Modifier::BOLD).fg(Color::Yellow),
                        ));
                    } else {
                        for word in &line.words {
                            let text = word.romanization.as_deref().unwrap_or(&word.text);
                            if current_time_ms >= word.end_time_ms {
                                spans.push(Span::styled(
                                    text,
                                    role_style.add_modifier(Modifier::BOLD).fg(Color::White),
                                ));
                            } else if current_time_ms >= word.start_time_ms {
                                spans.push(Span::styled(
                                    text,
                                    role_style
                                        .add_modifier(Modifier::BOLD)
                                        .add_modifier(Modifier::UNDERLINED)
                                        .fg(Color::Yellow),
                                ));
                            } else {
                                spans.push(Span::styled(
                                    text,
                                    Style::default().fg(Color::DarkGray),
                                ));
                            }
                        }
                    }
                    rendered_lines.push(Line::from(spans).alignment(alignment));
                }
                crate::core::app::LyricsDisplayMode::Both => {
                    // Row 1: Original text (with syllable highlight)
                    let mut orig_spans = Vec::new();
                    if line.words.is_empty() {
                        orig_spans.push(Span::styled(
                            &line.text,
                            role_style.add_modifier(Modifier::BOLD).fg(Color::Yellow),
                        ));
                    } else {
                        for word in &line.words {
                            if current_time_ms >= word.end_time_ms {
                                orig_spans.push(Span::styled(
                                    &word.text,
                                    role_style.add_modifier(Modifier::BOLD).fg(Color::White),
                                ));
                            } else if current_time_ms >= word.start_time_ms {
                                orig_spans.push(Span::styled(
                                    &word.text,
                                    role_style
                                        .add_modifier(Modifier::BOLD)
                                        .add_modifier(Modifier::UNDERLINED)
                                        .fg(Color::Yellow),
                                ));
                            } else {
                                orig_spans.push(Span::styled(
                                    &word.text,
                                    Style::default().fg(Color::DarkGray),
                                ));
                            }
                        }
                    }
                    rendered_lines.push(Line::from(orig_spans).alignment(alignment));

                    // Row 2: Romanized text stacked directly beneath in sync!
                    let rom_line_opt = line.romanization.as_deref().or(line.translation.as_deref());
                    if let Some(rom_line) = rom_line_opt {
                        let mut rom_spans = Vec::new();
                        if !line.words.is_empty() {
                            for word in &line.words {
                                let rom_w = word.romanization.as_deref().unwrap_or(&word.text);
                                if current_time_ms >= word.end_time_ms {
                                    rom_spans.push(Span::styled(
                                        rom_w,
                                        role_style
                                            .add_modifier(Modifier::BOLD)
                                            .add_modifier(Modifier::ITALIC)
                                            .fg(Color::LightCyan),
                                    ));
                                } else if current_time_ms >= word.start_time_ms {
                                    rom_spans.push(Span::styled(
                                        rom_w,
                                        role_style
                                            .add_modifier(Modifier::BOLD)
                                            .add_modifier(Modifier::UNDERLINED)
                                            .add_modifier(Modifier::ITALIC)
                                            .fg(Color::LightYellow),
                                    ));
                                } else {
                                    rom_spans.push(Span::styled(
                                        rom_w,
                                        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                                    ));
                                }
                            }
                        } else {
                            rom_spans.push(Span::styled(
                                rom_line,
                                Style::default()
                                    .fg(Color::LightYellow)
                                    .add_modifier(Modifier::ITALIC),
                            ));
                        }
                        rendered_lines.push(Line::from(rom_spans).alignment(alignment));
                    }
                }
            }
        } else if idx < active_line_idx {
            // Past line
            match display_mode {
                crate::core::app::LyricsDisplayMode::Original => {
                    rendered_lines.push(
                        Line::from(Span::styled(&line.text, Style::default().fg(Color::DarkGray)))
                            .alignment(alignment),
                    );
                }
                crate::core::app::LyricsDisplayMode::Romanized => {
                    let text = line.romanization.as_deref().or(line.translation.as_deref()).unwrap_or(&line.text);
                    rendered_lines.push(
                        Line::from(Span::styled(text, Style::default().fg(Color::DarkGray)))
                            .alignment(alignment),
                    );
                }
                crate::core::app::LyricsDisplayMode::Both => {
                    rendered_lines.push(
                        Line::from(Span::styled(&line.text, Style::default().fg(Color::DarkGray)))
                            .alignment(alignment),
                    );
                    let rom_opt = line.romanization.as_deref().or(line.translation.as_deref());
                    if let Some(rom) = rom_opt {
                        rendered_lines.push(
                            Line::from(Span::styled(
                                rom,
                                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                            ))
                            .alignment(alignment),
                        );
                    }
                }
            }
        } else {
            // Future upcoming line
            match display_mode {
                crate::core::app::LyricsDisplayMode::Original => {
                    rendered_lines.push(
                        Line::from(Span::styled(&line.text, Style::default().fg(Color::Gray)))
                            .alignment(alignment),
                    );
                }
                crate::core::app::LyricsDisplayMode::Romanized => {
                    let text = line.romanization.as_deref().or(line.translation.as_deref()).unwrap_or(&line.text);
                    rendered_lines.push(
                        Line::from(Span::styled(text, Style::default().fg(Color::Gray)))
                            .alignment(alignment),
                    );
                }
                crate::core::app::LyricsDisplayMode::Both => {
                    rendered_lines.push(
                        Line::from(Span::styled(&line.text, Style::default().fg(Color::Gray)))
                            .alignment(alignment),
                    );
                    let rom_opt = line.romanization.as_deref().or(line.translation.as_deref());
                    if let Some(rom) = rom_opt {
                        rendered_lines.push(
                            Line::from(Span::styled(
                                rom,
                                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                            ))
                            .alignment(alignment),
                        );
                    }
                }
            }
        }

        rendered_lines.push(Line::from(""));
    }

    // Scroll so that `active_row` is always placed at the vertical midpoint (`mid_offset`)
    let scroll_y = active_row.saturating_sub(mid_offset);
    let lyrics_para = Paragraph::new(rendered_lines).scroll((scroll_y as u16, 0));
    f.render_widget(lyrics_para, chunks[1]);

    // 3. Footer controls
    let footer_text = Line::from(vec![
        Span::styled("[Esc/q]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" Back   ", Style::default().fg(Color::White)),
        Span::styled("[T]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(" Mode (Orig/Rom/Both)   ", Style::default().fg(Color::White)),
        Span::styled("[K]", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
        Span::styled(" Align (L/C/R)   ", Style::default().fg(Color::White)),
        Span::styled("[R]", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
        Span::styled(" Refetch   ", Style::default().fg(Color::White)),
        Span::styled("[ [ / ] ]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Timing Offset", Style::default().fg(Color::White)),
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
