use crate::core::app::{active_lyric_index, App, LyricsStatus};
use crate::lyrics::VocalRole;
use crate::tui::layout::fullscreen_view_layout;
use crate::ui::karaoke_view::detect_instrumental_break;
use ratatui::{
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, BorderType, Borders, Gauge, Paragraph},
  Frame,
};

use super::player::draw_playbar;
use crate::tui::theme::{EmphasisExt, ThemeExt};

pub fn draw_lyrics_view(f: &mut Frame<'_>, app: &App) {
  f.render_widget(ratatui::widgets::Clear, f.area());
  let (content_area, playbar_area) = fullscreen_view_layout(&app.runtime_state, f.area());

  draw_lyrics(f, app, content_area);
  if let Some(playbar_area) = playbar_area {
    draw_playbar(f, app, playbar_area);
  }
}

pub fn draw_lyrics(f: &mut Frame<'_>, app: &App, area: Rect) {
  let theme = &app.user_config.theme;
  let offset_ms = app.view.lyrics_view.timing_offset_ms;

  let provider_str = crate::core::app::LYRIC_PROVIDERS[app.view.karaoke_provider_index.min(crate::core::app::LYRIC_PROVIDERS.len() - 1)];
  let mode_str = app.view.karaoke_display_mode.label();
  let align_str = app.view.karaoke_alignment.label();
  let offset_str = format!("{:+.1}s", offset_ms as f64 / 1000.0);

  let title = format!(
    " Lyrics │ {} │ Mode: {} │ Align: {} │ Offset: {} ",
    provider_str, mode_str, align_str, offset_str
  );

  let block = Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .title(Span::styled(title, Style::default().fg(theme.header.into()).add_modifier(Modifier::BOLD)))
    .style(theme.base_style());
  f.render_widget(block.clone(), area);

  let inner_area = block.inner(area);
  if inner_area.width == 0 || inner_area.height == 0 {
    return;
  }

  // 1. If our rich syllable-by-syllable, stacked Romaji karaoke timeline is available, render with rich engine!
  if let Some(timeline) = &app.view.active_karaoke_timeline {
    if !timeline.lines.is_empty() {
      draw_rich_karaoke_stage(f, app, timeline, inner_area);
      return;
    }
  }

  // 2. Otherwise fall back to legacy lyrics if found
  if app.lyrics_status() == LyricsStatus::Found {
    if let Some(lyrics) = app.lyrics() {
      if !lyrics.is_empty() {
        draw_legacy_lyrics(f, app, lyrics, inner_area);
        return;
      }
    }
  }

  // 3. Otherwise show state message or prompt to refetch
  draw_state_message(f, app, inner_area);
}

fn draw_rich_karaoke_stage(
  f: &mut Frame<'_>,
  app: &App,
  timeline: &crate::lyrics::KaraokeTimeline,
  inner_area: Rect,
) {
  let theme = &app.user_config.theme;
  let offset_ms = app.view.lyrics_view.timing_offset_ms;

  #[cfg(feature = "youtube")]
  let current_time_ms = if let Some(yt) = app.youtube_playback.as_ref() {
    (yt.player.position().as_millis() as i64) + offset_ms
  } else {
    (app.song_progress_ms as i64) + offset_ms
  };
  #[cfg(not(feature = "youtube"))]
  let current_time_ms = (app.song_progress_ms as i64) + (offset_ms as i64);

  let break_info = detect_instrumental_break(timeline, current_time_ms);
  let break_height = if break_info.is_some() { 3 } else { 0 };

  let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
      Constraint::Length(break_height), // Instrumental break banner
      Constraint::Min(4),               // Lyrics lines
      Constraint::Length(1),            // Bottom controls / browsing hint
    ])
    .split(inner_area);

  // Instrumental Break Countdown Bar
  if let Some((remaining_sec, progress)) = break_info {
    let label = format!("♪ ♫ [Instrumental Break — {:04.1}s] ♫ ♪", remaining_sec);
    let gauge = Gauge::default()
      .block(Block::default().borders(Borders::NONE))
      .gauge_style(
        Style::default()
          .fg(theme.selected.into())
          .bg(theme.inactive.into())
          .add_modifier(Modifier::BOLD),
      )
      .percent((progress * 100.0) as u16)
      .label(label);
    f.render_widget(gauge, chunks[0]);
  }

  // Lyrics Stage Rendering
  let active_line_idx = timeline.active_line_index(current_time_ms).unwrap_or(0);
  let manual = app.view.lyrics_view.manual_index.is_some();
  let focus_idx = app.view.lyrics_view.manual_index.unwrap_or(active_line_idx).min(timeline.lines.len().saturating_sub(1));

  let available_lines = chunks[1].height as usize;
  let mid_offset = available_lines / 2;

  let mut rendered_lines = Vec::new();
  let mut focus_row = 0;

  for _ in 0..mid_offset {
    rendered_lines.push(Line::from(""));
  }

  let alignment = app.view.karaoke_alignment.to_ratatui();
  let display_mode = app.view.karaoke_display_mode;

  for (idx, line) in timeline.lines.iter().enumerate() {
    let is_active = idx == active_line_idx;
    let is_focused = manual && idx == focus_idx;

    if is_focused || (!manual && is_active) {
      focus_row = rendered_lines.len();
    }

    let role_style = match line.vocal_role {
      VocalRole::Lead1 => Style::default().fg(theme.selected.into()),
      VocalRole::Lead2 => Style::default().fg(theme.hovered.into()),
      VocalRole::Background => Style::default().fg(theme.inactive.into()).add_modifier(Modifier::ITALIC),
    };

    let prefix = if is_focused { "▶ " } else { "  " };

    if is_active {
      match display_mode {
        crate::core::app::LyricsDisplayMode::Original => {
          let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD))];
          if line.words.is_empty() {
            spans.push(Span::styled(&line.text, role_style.add_modifier(Modifier::BOLD).fg(theme.highlighted_lyrics.into())));
          } else {
            for word in &line.words {
              if current_time_ms >= word.end_time_ms {
                spans.push(Span::styled(&word.text, role_style.add_modifier(Modifier::BOLD).fg(theme.text.into())));
              } else if current_time_ms >= word.start_time_ms {
                spans.push(Span::styled(
                  &word.text,
                  role_style.add_modifier(Modifier::BOLD).add_modifier(Modifier::UNDERLINED).fg(theme.highlighted_lyrics.into()),
                ));
              } else {
                spans.push(Span::styled(&word.text, Style::default().fg(theme.inactive.into())));
              }
            }
          }
          rendered_lines.push(Line::from(spans).alignment(alignment));
        }
        crate::core::app::LyricsDisplayMode::Romanized => {
          let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD))];
          if line.words.is_empty() {
            let text = line.romanization.as_deref().unwrap_or(&line.text);
            spans.push(Span::styled(text, role_style.add_modifier(Modifier::BOLD).fg(theme.highlighted_lyrics.into())));
          } else {
            for word in &line.words {
              let text = word.romanization.as_deref().unwrap_or(&word.text);
              if current_time_ms >= word.end_time_ms {
                spans.push(Span::styled(text, role_style.add_modifier(Modifier::BOLD).fg(theme.text.into())));
              } else if current_time_ms >= word.start_time_ms {
                spans.push(Span::styled(
                  text,
                  role_style.add_modifier(Modifier::BOLD).add_modifier(Modifier::UNDERLINED).fg(theme.highlighted_lyrics.into()),
                ));
              } else {
                spans.push(Span::styled(text, Style::default().fg(theme.inactive.into())));
              }
            }
          }
          rendered_lines.push(Line::from(spans).alignment(alignment));
        }
        crate::core::app::LyricsDisplayMode::Both => {
          // Row 1: Original text
          let mut orig_spans = vec![Span::styled(prefix, Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD))];
          if line.words.is_empty() {
            orig_spans.push(Span::styled(&line.text, role_style.add_modifier(Modifier::BOLD).fg(theme.highlighted_lyrics.into())));
          } else {
            for word in &line.words {
              if current_time_ms >= word.end_time_ms {
                orig_spans.push(Span::styled(&word.text, role_style.add_modifier(Modifier::BOLD).fg(theme.text.into())));
              } else if current_time_ms >= word.start_time_ms {
                orig_spans.push(Span::styled(
                  &word.text,
                  role_style.add_modifier(Modifier::BOLD).add_modifier(Modifier::UNDERLINED).fg(theme.highlighted_lyrics.into()),
                ));
              } else {
                spans_push(&mut orig_spans, &word.text, Style::default().fg(theme.inactive.into()));
              }
            }
          }
          rendered_lines.push(Line::from(orig_spans).alignment(alignment));

          // Row 2: Romanized text stacked directly beneath in sync!
          let rom_line_opt = line.romanization.as_deref().or(line.translation.as_deref());
          if let Some(rom_line) = rom_line_opt {
            let mut rom_spans = vec![Span::raw("  ")];
            if !line.words.is_empty() {
              for word in &line.words {
                let rom_w = word.romanization.as_deref().unwrap_or(&word.text);
                if current_time_ms >= word.end_time_ms {
                  rom_spans.push(Span::styled(rom_w, Style::default().fg(theme.hint.into())));
                } else if current_time_ms >= word.start_time_ms {
                  rom_spans.push(Span::styled(rom_w, Style::default().fg(theme.highlighted_lyrics.into()).add_modifier(Modifier::BOLD)));
                } else {
                  rom_spans.push(Span::styled(rom_w, Style::default().fg(theme.inactive.into())));
                }
              }
            } else {
              rom_spans.push(Span::styled(rom_line, Style::default().fg(theme.hint.into())));
            }
            rendered_lines.push(Line::from(rom_spans).alignment(alignment));
          }
        }
      }
    } else {
      let mut text_to_display = line.text.clone();
      if display_mode == crate::core::app::LyricsDisplayMode::Romanized {
        if let Some(ref rom) = line.romanization {
          text_to_display = rom.clone();
        }
      }

      let line_style = if is_focused {
        Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)
      } else {
        Style::default().fg(theme.inactive.into())
      };

      let prefix_span = if is_focused {
        Span::styled("▶ ", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD))
      } else {
        Span::raw("  ")
      };

      rendered_lines.push(Line::from(vec![prefix_span, Span::styled(text_to_display, line_style)]).alignment(alignment));

      if display_mode == crate::core::app::LyricsDisplayMode::Both {
        if let Some(ref rom) = line.romanization {
          rendered_lines.push(Line::from(vec![Span::raw("  "), Span::styled(rom.clone(), Style::default().fg(theme.inactive.into()).add_modifier(Modifier::ITALIC))]).alignment(alignment));
        }
      }
    }

    if app.view.lyrics_line_spacing > 0 {
      rendered_lines.push(Line::from(""));
    }
  }

  for _ in 0..mid_offset {
    rendered_lines.push(Line::from(""));
  }

  let scroll_target = focus_row.saturating_sub(mid_offset);
  let visible_lines: Vec<Line> = rendered_lines
    .into_iter()
    .skip(scroll_target)
    .take(available_lines)
    .collect();

  f.render_widget(Paragraph::new(visible_lines), chunks[1]);

  // Bottom control hints
  let hint_text = if manual {
    "[j/k: Browse │ Enter: Seek to line │ f/Esc: Auto-Follow │ b: Align │ t: Mode │ p: Spacing │ [/]: Offset]"
  } else {
    "[j/k: Browse lines │ b: Align │ t: Mode │ p: Spacing │ [/]: Offset]"
  };
  let hint_widget = Paragraph::new(Line::from(Span::styled(hint_text, Style::default().fg(theme.inactive.into())))).alignment(Alignment::Center);
  f.render_widget(hint_widget, chunks[2]);
}

fn spans_push(spans: &mut Vec<Span>, text: &str, style: Style) {
  spans.push(Span::styled(text.to_string(), style));
}

pub(crate) fn draw_legacy_lyrics(f: &mut Frame<'_>, app: &App, lyrics: &[(u128, String)], inner_area: Rect) {
  let theme = &app.user_config.theme;
  let manual = app.view.lyrics_view.manual_index.is_some();
  let mut lyric_area = inner_area;
  if lyric_area.height > 1 {
    lyric_area.height -= 1;
  }

  let active_idx = active_lyric_index(lyrics, app.lyric_progress_ms());
  let focus_idx = app
    .view
    .lyrics_view
    .manual_index
    .unwrap_or(active_idx)
    .min(lyrics.len() - 1);

  let target_row = i64::from(lyric_area.y) + i64::from(lyric_area.height / 2);
  let scroll_offset = app.view.lyrics_view.scroll_pos;
  let line_spacing = if app.view.lyrics_line_spacing > 0 { 2 } else { 1 };

  for (line_idx, (_, text)) in lyrics.iter().enumerate() {
    let y = target_row + ((line_idx * line_spacing) as f64 - scroll_offset * line_spacing as f64).round() as i64;
    if y < i64::from(lyric_area.y) || y >= i64::from(lyric_area.y) + i64::from(lyric_area.height) {
      continue;
    }

    let style = if line_idx == active_idx {
      Style::default()
        .fg(theme.highlighted_lyrics.into())
        .add_modifier(app.user_config.behavior.emphasis(Modifier::BOLD))
    } else if manual && line_idx == focus_idx {
      Style::default().fg(theme.hovered.into()).add_modifier(Modifier::BOLD)
    } else {
      Style::default().fg(theme.inactive.into())
    };

    let prefix = if manual && line_idx == focus_idx { "▶ " } else { "" };
    let display = format!("{}{}", prefix, text);

    f.render_widget(
      Paragraph::new(display)
        .style(style)
        .alignment(app.view.karaoke_alignment.to_ratatui()),
      Rect {
        x: lyric_area.x,
        y: y as u16,
        width: lyric_area.width,
        height: 1,
      },
    );
  }

  let hint_text = if manual {
    "[j/k: Browse │ Enter: Seek to line │ f/Esc: Auto-Follow │ b: Align │ p: Spacing]"
  } else {
    "[j/k: Browse │ b: Align │ p: Spacing]"
  };
  f.render_widget(
    Paragraph::new(hint_text)
      .style(Style::default().fg(theme.hint.into()))
      .alignment(Alignment::Center),
    Rect {
      y: inner_area.y + inner_area.height - 1,
      height: 1,
      ..inner_area
    },
  );
}

/// Centered message when lyrics are loading, missing, or nothing is playing.
fn draw_state_message(f: &mut Frame<'_>, app: &App, inner_area: Rect) {
  let (primary, secondary) = match app.lyrics_status() {
    LyricsStatus::Loading => (
      app.strings().screens.lyrics_loading,
      app.strings().screens.lyrics_from,
    ),
    LyricsStatus::NotFound => (
      app.strings().screens.lyrics_missing,
      app.strings().screens.lyrics_credit,
    ),
    LyricsStatus::NotStarted => (
      app.strings().screens.nothing_playing,
      app.strings().screens.lyrics_start_track,
    ),
    LyricsStatus::Found => (
      "No lyrics available for this track",
      "Press 'Ctrl+r' to retry fetching lyrics via relay waterfall",
    ),
  };

  let theme = &app.user_config.theme;
  let primary_y = inner_area.y + inner_area.height.saturating_sub(1) / 2;
  f.render_widget(
    Paragraph::new(primary)
      .style(Style::default().fg(theme.text.into()))
      .alignment(Alignment::Center),
    Rect {
      y: primary_y,
      height: 1,
      ..inner_area
    },
  );
  let secondary_y = primary_y + 1;
  if secondary_y < inner_area.y + inner_area.height {
    f.render_widget(
      Paragraph::new(secondary)
        .style(Style::default().fg(theme.inactive.into()))
        .alignment(Alignment::Center),
      Rect {
        y: secondary_y,
        height: 1,
        ..inner_area
      },
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::app::{ActiveBlock, RouteId};
  use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

  fn app_with_lyrics() -> App {
    let mut app = App::default();
    app.set_lyrics(
      LyricsStatus::Found,
      Some(vec![
        (0, "first line".to_string()),
        (10_000, "second line".to_string()),
        (20_000, "third line".to_string()),
      ]),
      true,
    );
    app.song_progress_ms = 11_000;
    app.view.lyrics_view.scroll_pos = 1.0;
    app.push_navigation_stack(RouteId::LyricsView, ActiveBlock::LyricsView);
    app
  }

  fn render(app: &App) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| draw_lyrics_view(f, app)).unwrap();
    terminal.backend().buffer().clone()
  }

  fn buffer_text(buffer: &Buffer) -> String {
    (0..buffer.area().height)
      .map(|y| {
        (0..buffer.area().width)
          .filter_map(|x| buffer.cell((x, y)).map(|c| c.symbol().to_string()))
          .collect::<String>()
      })
      .collect::<Vec<_>>()
      .join("\n")
  }

  fn row_of(buffer: &Buffer, needle: &str) -> Option<u16> {
    (0..buffer.area().height).find(|&y| {
      (0..buffer.area().width)
        .filter_map(|x| buffer.cell((x, y)).map(|c| c.symbol().to_string()))
        .collect::<String>()
        .contains(needle)
    })
  }

  #[test]
  fn renders_all_lines_with_active_centered() {
    let app = app_with_lyrics();
    let buffer = render(&app);
    let text = buffer_text(&buffer);
    assert!(text.contains(" Lyrics "), "missing block title:\n{text}");
    assert!(text.contains("first line"), "missing prior line:\n{text}");
    assert!(text.contains("second line"), "missing active line:\n{text}");
    assert!(text.contains("third line"), "missing next line:\n{text}");

    let active_row = row_of(&buffer, "second line").expect("active line row");
    let available_height = (24 - app.runtime_state.playbar_height_rows - 2).saturating_sub(1);
    let expected = 1 + available_height / 2;
    assert_eq!(active_row, expected);
  }

  #[test]
  fn fractional_scroll_shifts_the_whole_column() {
    let mut app = app_with_lyrics();
    let before = row_of(&render(&app), "second line").expect("active line row");
    app.view.lyrics_view.scroll_pos = 1.6;
    let buffer = render(&app);
    let after = row_of(&buffer, "second line").expect("active line row");
    assert!(after < before, "after={after} must be above before={before}");
  }

  #[test]
  fn test_lyrics_line_spacing_spaces_lines() {
    let mut app = app_with_lyrics();
    // Default spacing = 0: adjacent lines have distance 1
    let buf_compact = render(&app);
    let r1_compact = row_of(&buf_compact, "second line").expect("row 2");
    let r2_compact = row_of(&buf_compact, "third line").expect("row 3");
    assert_eq!(r2_compact - r1_compact, 1);

    // Spaced mode: lines separated by 1 blank line (distance 2)
    app.view.lyrics_line_spacing = 1;
    let buf_spaced = render(&app);
    let r1_spaced = row_of(&buf_spaced, "second line").expect("row 2 spaced");
    let r2_spaced = row_of(&buf_spaced, "third line").expect("row 3 spaced");
    assert_eq!(r2_spaced - r1_spaced, 2);
  }
}
