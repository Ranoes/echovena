use super::common_key_events;
use crate::core::action::{Action, CopyTarget};
use crate::core::app::{active_lyric_index, App, LyricsStatus};
use crate::tui::event::Key;
use std::time::Instant;

pub fn handler(key: Key, app: &mut App) {
  match key {
    Key::Char('b') => {
      app.view.karaoke_alignment = app.view.karaoke_alignment.cycle();
      app.set_status_message(format!("Lyrics Position: {}", app.view.karaoke_alignment.label()), 3);
    }
    Key::Char('p') => {
      app.view.lyrics_line_spacing = if app.view.lyrics_line_spacing == 0 { 1 } else { 0 };
      let label = if app.view.lyrics_line_spacing > 0 { "+1 Line (Spaced)" } else { "0 Lines (Compact)" };
      app.set_status_message(format!("Lyrics Line Spacing: {}", label), 3);
    }
    Key::Char('t') | Key::Char('T') => {
      app.view.karaoke_display_mode = app.view.karaoke_display_mode.cycle();
      app.view.karaoke_show_romanization = app.view.karaoke_display_mode != crate::core::app::LyricsDisplayMode::Original;
      app.set_status_message(format!("Lyrics Mode: {}", app.view.karaoke_display_mode.label()), 3);
    }
    Key::Char('f') | Key::Esc => {
      app.view.lyrics_view.manual_index = None;
      app.set_status_message("Resumed lyrics auto-follow".to_string(), 3);
    }
    Key::Enter => {
      if let Some(target) = app.view.lyrics_view.manual_index {
        if let Some(timeline) = &app.view.active_karaoke_timeline {
          if let Some(line) = timeline.lines.get(target) {
            let seek_ms = line.start_time_ms.max(0) as u32;
            app.dispatch(crate::infra::network::IoEvent::Seek(seek_ms));
            app.set_status_message(format!("Jumped playback to line {}: {:02}:{:02}", target + 1, seek_ms / 60_000, (seek_ms / 1_000) % 60), 4);
          }
        } else if let Some(lyrics) = app.lyrics() {
          if let Some((ms, _)) = lyrics.get(target) {
            let seek_ms = *ms as u32;
            app.dispatch(crate::infra::network::IoEvent::Seek(seek_ms));
            app.set_status_message(format!("Jumped playback to line {}: {:02}:{:02}", target + 1, seek_ms / 60_000, (seek_ms / 1_000) % 60), 4);
          }
        }
      }
    }
    Key::Char('q') | Key::Char('B') => {
      app.pop_navigation_stack();
    }
    k if k == app.user_config.keys.back => {
      app.pop_navigation_stack();
    }
    k if common_key_events::up_event(k, &app.user_config.keys) => scroll_by(app, -1),
    k if common_key_events::down_event(k, &app.user_config.keys) => scroll_by(app, 1),
    Key::Char('[') => {
      app.view.lyrics_view.timing_offset_ms -= 100;
      app.set_status_message(format!("Lyrics Offset: {:+.1}s", app.view.lyrics_view.timing_offset_ms as f64 / 1000.0), 3);
    }
    Key::Char(']') => {
      app.view.lyrics_view.timing_offset_ms += 100;
      app.set_status_message(format!("Lyrics Offset: {:+.1}s", app.view.lyrics_view.timing_offset_ms as f64 / 1000.0), 3);
    }
    k if common_key_events::left_event(k, &app.user_config.keys) => nudge_timing(app, -500),
    k if common_key_events::right_event(k, &app.user_config.keys) => nudge_timing(app, 500),
    k if k == app.user_config.keys.next_page => scroll_by(app, 5),
    k if k == app.user_config.keys.previous_page => scroll_by(app, -5),
    k if k == app.user_config.keys.jump_to_start => jump_to(app, 0),
    k if k == app.user_config.keys.jump_to_end => jump_to(app, usize::MAX),

    // Media controls
    Key::Char(' ') => { let _ = app.apply(Action::TogglePlayback); }
    Key::Char('>') => { let _ = app.apply(Action::NextTrack); }
    Key::Char('<') => { let _ = app.apply(Action::PreviousTrack); }
    Key::Char('.') => { let _ = app.apply(Action::SeekForward); }
    Key::Char(',') => { let _ = app.apply(Action::SeekBackward); }
    Key::Char('+') | Key::Char('=') => { let _ = app.apply(Action::VolumeUp); }
    Key::Char('-') => { let _ = app.apply(Action::VolumeDown); }
    Key::Char('r') => { let _ = app.apply(Action::CycleRepeat); }
    Key::Char('R') => { let _ = app.apply(Action::ToggleShuffle); }
    Key::Char('s') => super::handle_save_single_track(app),
    Key::Char('S') => super::handle_save_album_or_playlist(app),
    Key::Char('w') => { let _ = app.apply(Action::OpenAddPlayingTrackDialog); }
    Key::Char('y') => {
      super::copy_url(app, Action::CopyUrl(CopyTarget::CurrentSong));
    }
    Key::Char('Y') => {
      super::copy_url(app, Action::CopyUrl(CopyTarget::CurrentAlbum));
    }
    Key::Ctrl('r') => {
      app.set_status_message("Re-fetching lyrics via auto waterfall...".to_string(), 4);
      app.dispatch(crate::infra::network::IoEvent::RefetchKaraokeLyrics { force: true });
    }
    _ => {}
  }
}

/// Scroll the browsed line by `delta`, entering manual mode from the
/// currently playing line when auto-follow was active.
pub(super) fn scroll_by(app: &mut App, delta: i64) {
  if let Some(timeline) = &app.view.active_karaoke_timeline {
    if !timeline.lines.is_empty() {
      let offset_ms = app.view.lyrics_view.timing_offset_ms;
      #[cfg(feature = "youtube")]
      let current_time_ms = if let Some(yt) = app.youtube_playback.as_ref() {
        (yt.player.position().as_millis() as i64) + (offset_ms as i64)
      } else {
        (app.song_progress_ms as i64) + (offset_ms as i64)
      };
      #[cfg(not(feature = "youtube"))]
      let current_time_ms = (app.song_progress_ms as i64) + (offset_ms as i64);

      let active_idx = timeline.active_line_index(current_time_ms).unwrap_or(0);
      let from = app.view.lyrics_view.manual_index.unwrap_or(active_idx);
      let target = (from as i64 + delta).clamp(0, timeline.lines.len() as i64 - 1) as usize;
      app.view.lyrics_view.manual_index = Some(target);
      app.view.lyrics_view.last_manual_input = Some(Instant::now());
      return;
    }
  }

  if app.lyrics_status() != LyricsStatus::Found {
    return;
  }
  let Some(lyrics) = app.lyrics() else {
    return;
  };
  if lyrics.is_empty() {
    return;
  }
  let from = app
    .view
    .lyrics_view
    .manual_index
    .unwrap_or_else(|| active_lyric_index(lyrics, app.lyric_progress_ms()));
  let target = (from as i64 + delta).clamp(0, lyrics.len() as i64 - 1) as usize;
  app.view.lyrics_view.manual_index = Some(target);
  app.view.lyrics_view.last_manual_input = Some(Instant::now());
}

/// Shift lyric timing relative to playback, for correcting misaligned LRC
/// files. Positive delta shows lyrics earlier.
fn nudge_timing(app: &mut App, delta_ms: i64) {
  app.view.lyrics_view.timing_offset_ms += delta_ms;
  app.set_status_message(format!("Lyrics Offset: {:+.1}s", app.view.lyrics_view.timing_offset_ms as f64 / 1000.0), 3);
}

fn jump_to(app: &mut App, index: usize) {
  if let Some(timeline) = &app.view.active_karaoke_timeline {
    if !timeline.lines.is_empty() {
      app.view.lyrics_view.manual_index = Some(index.min(timeline.lines.len() - 1));
      app.view.lyrics_view.last_manual_input = Some(Instant::now());
      return;
    }
  }

  if app.lyrics_status() != LyricsStatus::Found {
    return;
  }
  let Some(lyrics) = app.lyrics() else {
    return;
  };
  if lyrics.is_empty() {
    return;
  }
  app.view.lyrics_view.manual_index = Some(index.min(lyrics.len() - 1));
  app.view.lyrics_view.last_manual_input = Some(Instant::now());
}
