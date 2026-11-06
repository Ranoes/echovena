#[cfg(feature = "ai-dj")]
mod ai_dj;
mod album_list;
mod album_tracks;
mod analysis;
mod announcement_prompt;
mod artist;
mod artists;
mod common_key_events;
mod community_pin_prompt;
#[cfg(feature = "cover-art")]
mod cover_art_view;
mod create_playlist;
mod dialog;
mod discover;
mod empty;
mod episode_table;
mod error_screen;
mod filter_input;
mod friends;
mod help_menu;
mod home;
mod input;
mod library;
mod local_browser;
mod lyrics_view;
mod miniplayer;
mod mouse;
mod party;
mod playbar;
pub(crate) mod playlist;
mod playlist_sync;
mod plugin_screen;
mod podcasts;
mod queue_menu;
mod recap_prompt;
mod recently_played;
pub mod resize;
mod search_results;
mod select_device;
mod settings;
mod sort_menu;
mod stats;
mod track_table;

use crate::core::action::{Action, CopyTarget, NavTarget};
use crate::core::app::{
  ActiveBlock, AlbumTableContext, App, ArtistBlock, InputContext, PlaybackOwner, RouteId, SearchResultBlock,
  TrackTableContext,
};
use crate::tui::event::Key;

pub use announcement_prompt::dismiss as dismiss_announcement;
pub use input::handler as input_handler;
pub use mouse::handler as mouse_handler;

#[cfg(target_os = "macos")]
fn key_matches_open_settings_binding(key: Key, binding: Key) -> bool {
  key == binding
    || (binding == Key::Alt(',') && key == Key::Char('≤'))
    || (binding == Key::Ctrl(',')
      && (key == Key::Ctrl('l')
        || key == Key::Ctrl('L')
        || key == Key::Ctrl('4')
        || key == Key::Ctrl('<')))
}

#[cfg(not(target_os = "macos"))]
fn key_matches_open_settings_binding(key: Key, binding: Key) -> bool {
  key == binding
}

fn should_route_friends_before_globals(key: Key, app: &App) -> bool {
  if app.get_current_route().active_block != ActiveBlock::Friends {
    return false;
  }

  app.view.friend_add_dialog_visible
    || !app.view.friend_search_input.is_empty()
    || matches!(
      key,
      Key::Char('a') | Key::Char('c') | Key::Char('u') | Key::Tab
    )
}

/// Global handler for bracketed paste events delivered from the terminal.
pub fn handle_paste(text: String, app: &mut App) {
  let cleaned = text.trim_matches(|c| c == '\r' || c == '\n').to_string();
  if cleaned.is_empty() {
    return;
  }

  // 1. If URL Import modal is open, paste directly into it
  if app.view.url_import_visible {
    app.view.url_import_input = cleaned.chars().collect();
    app.set_status_message("Pasted URL from clipboard. Press Enter to import.".to_string(), 4);
    return;
  }

  // 2. Check if text looks like a URL or Spotify/YouTube link:
  let is_url = cleaned.starts_with("http://")
    || cleaned.starts_with("https://")
    || cleaned.starts_with("spotify:")
    || cleaned.starts_with("youtube:")
    || cleaned.starts_with("yt:")
    || cleaned.starts_with("www.")
    || cleaned.contains("youtube.com")
    || cleaned.contains("youtu.be")
    || cleaned.contains("spotify.com");

  if is_url {
    app.view.url_import_visible = true;
    app.view.url_import_input = cleaned.chars().collect();
    app.set_status_message("Pasted music URL! Press Enter to import.".to_string(), 5);
    return;
  }

  // 3. If Search input is focused, paste directly into search buffer
  if app.get_current_route().active_block == ActiveBlock::Input {
    for c in cleaned.chars() {
      app.view.input.insert(app.view.input_idx, c);
      app.view.input_idx += 1;
      app.view.input_cursor_position += input::compute_character_width(c);
    }
    return;
  }

  // If it's plain text pasted on main screen, open search and fill it
  focus_global_search(app);
  app.view.input = cleaned.chars().collect();
  app.view.input_idx = app.view.input.len();
  app.view.input_cursor_position = cleaned.len() as u16;
  app.set_status_message("Pasted search query! Press Enter to search.".to_string(), 4);
}

pub fn handle_app(key: Key, app: &mut App) {
  // Relay Health modal: intercept keys when visible.
  if app.view.relay_status_visible {
    match key {
      Key::Esc | Key::Char('q') | Key::Char('R') | Key::Char('r') => {
        app.view.relay_status_visible = false;
      }
      k if k == app.user_config.keys.back => {
        app.view.relay_status_visible = false;
      }
      _ => {}
    }
    return;
  }

  // Full-Screen Now Playing Dashboard / Karaoke Stage View: intercept keys when visible.
  if app.view.karaoke_stage_visible {
    match key {
      Key::Esc | Key::Char('q') | Key::Char('O') => {
        app.view.karaoke_stage_visible = false;
      }
      k if k == app.user_config.keys.back => {
        app.view.karaoke_stage_visible = false;
      }
      Key::Char('b') => {
        app.view.karaoke_alignment = app.view.karaoke_alignment.cycle();
        app.set_status_message(format!("Lyrics Position: {}", app.view.karaoke_alignment.label()), 3);
      }
      Key::Char('T') | Key::Char('t') => {
        app.view.karaoke_display_mode = app.view.karaoke_display_mode.cycle();
        app.view.karaoke_show_romanization = app.view.karaoke_display_mode != crate::core::app::LyricsDisplayMode::Original;
        app.set_status_message(format!("Lyrics Mode: {}", app.view.karaoke_display_mode.label()), 3);
      }
      Key::Ctrl('r') => {
        app.set_status_message("Re-fetching lyrics via auto waterfall...".to_string(), 4);
        app.dispatch(crate::infra::network::IoEvent::RefetchKaraokeLyrics { force: true });
      }
      Key::Char('[') => {
        app.view.lyrics_view.timing_offset_ms -= 100;
        app.set_status_message(format!("Lyrics Offset: {:+.1}s", app.view.lyrics_view.timing_offset_ms as f64 / 1000.0), 3);
      }
      Key::Char(']') => {
        app.view.lyrics_view.timing_offset_ms += 100;
        app.set_status_message(format!("Lyrics Offset: {:+.1}s", app.view.lyrics_view.timing_offset_ms as f64 / 1000.0), 3);
      }
      Key::Char('j') | Key::Down => {
        lyrics_view::scroll_by(app, 1);
      }
      Key::Char('k') | Key::Up => {
        lyrics_view::scroll_by(app, -1);
      }
      Key::Char('f') => {
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
          }
        }
      }
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
      Key::Char('s') => handle_save_single_track(app),
      Key::Char('S') => handle_save_album_or_playlist(app),
      Key::Char('w') => { let _ = app.apply(Action::OpenAddPlayingTrackDialog); }
      Key::Char('y') => {
        copy_url(app, Action::CopyUrl(CopyTarget::CurrentSong));
      }
      Key::Char('Y') => {
        copy_url(app, Action::CopyUrl(CopyTarget::CurrentAlbum));
      }
      _ => {}
    }
    return;
  }

  // URL Import modal: intercept keys when visible.
  if app.view.url_import_visible {
    match key {
      Key::Esc => {
        app.view.url_import_visible = false;
      }
      Key::Ctrl('v') => {
        if let Some(text) = app.get_clipboard_text() {
          let trimmed = text.trim_matches(|c| c == '\r' || c == '\n').to_string();
          app.view.url_import_input = trimmed.chars().collect();
          app.set_status_message("Pasted URL from clipboard. Press Enter to import.".to_string(), 4);
        }
      }
      Key::Char('p') | Key::Char('P') if app.view.url_import_input.is_empty() => {
        if let Some(text) = app.get_clipboard_text() {
          let trimmed = text.trim_matches(|c| c == '\r' || c == '\n').to_string();
          app.view.url_import_input = trimmed.chars().collect();
          app.set_status_message("Vim 'p' pasted URL from clipboard! Press Enter to import.".to_string(), 4);
        } else {
          app.view.url_import_input.push('p');
        }
      }
      Key::Backspace => {
        app.view.url_import_input.pop();
      }
      Key::Ctrl('u') => {
        app.view.url_import_input.clear();
      }
      Key::Enter => {
        let input_str: String = app.view.url_import_input.iter().collect();
        let trimmed = input_str.trim().to_string();
        if !trimmed.is_empty() {
          app.view.url_import_in_progress = true;
          app.view.url_import_status = Some("Connecting and resolving playlist in background...".to_string());
          app.set_status_message(format!("Importing playlist '{}' in background...", trimmed), 15);
          app.dispatch(crate::infra::network::IoEvent::ImportPlaylistUrl(trimmed));
        }
      }
      Key::Char(c) => {
        app.view.url_import_input.push(c);
      }
      _ => {}
    }
    return;
  }

  // Sync Auth Modal: intercept keys when visible.
  if let Some(modal_type) = app.view.sync_auth_modal {
    match key {
      Key::Esc => {
        app.view.sync_auth_modal = None;
      }
      Key::Ctrl('v') => {
        if let Some(text) = app.get_clipboard_text() {
          let trimmed = text.trim_matches(|c| c == '\r' || c == '\n').to_string();
          for c in trimmed.chars() {
            app.view.sync_auth_input.push(c);
          }
          app.set_status_message("Pasted from clipboard. Press Enter to save.".to_string(), 4);
        }
      }
      Key::Char('p') | Key::Char('P') if app.view.sync_auth_input.is_empty() => {
        if let Some(text) = app.get_clipboard_text() {
          let trimmed = text.trim_matches(|c| c == '\r' || c == '\n').to_string();
          for c in trimmed.chars() {
            app.view.sync_auth_input.push(c);
          }
          app.set_status_message("Vim 'p' pasted from clipboard. Press Enter to save.".to_string(), 4);
        } else {
          app.view.sync_auth_input.push('p');
        }
      }
      Key::Backspace => {
        app.view.sync_auth_input.pop();
      }
      Key::Ctrl('u') => {
        app.view.sync_auth_input.clear();
      }
      Key::Enter => {
        let input_str: String = app.view.sync_auth_input.iter().collect();
        let trimmed = input_str.trim().to_string();
        if !trimmed.is_empty() {
          match modal_type {
            crate::core::app::SyncAuthModalType::SpotifySpDc => {
              if let Err(e) = crate::sources::sync::auth::SpotifyAuthHandler::save_sp_dc(&trimmed) {
                app.set_status_message(format!("Failed to save Spotify sp_dc: {e}"), 6);
              } else {
                app.set_status_message("Spotify sp_dc saved! Press 's' to run bidirectional sync.".to_string(), 6);
              }
            }
            crate::core::app::SyncAuthModalType::YouTubeCookies => {
              if let Err(e) = crate::sources::sync::auth::YouTubeAuthHandler::save_cookies_file(&trimmed) {
                app.set_status_message(format!("Failed to save YouTube cookies: {e}"), 6);
              } else {
                app.set_status_message("YouTube cookies saved! Press 's' to run bidirectional sync.".to_string(), 6);
              }
            }
          }
        }
        app.view.sync_auth_modal = None;
      }
      Key::Char(c) => {
        app.view.sync_auth_input.push(c);
      }
      _ => {}
    }
    return;
  }

  // Manual YouTube Remap modal: intercept keys when visible.
  if app.view.remap_modal_visible {
    match key {
      Key::Esc => {
        app.view.remap_modal_visible = false;
        app.view.remap_input.clear();
      }
      Key::Ctrl('v') => {
        if let Some(text) = app.get_clipboard_text() {
          let trimmed = text.trim_matches(|c| c == '\r' || c == '\n').to_string();
          for c in trimmed.chars() {
            app.view.remap_input.push(c);
          }
          app.set_status_message("Pasted YouTube link/ID from clipboard. Press Enter to map.".to_string(), 4);
        }
      }
      Key::Char('p') | Key::Char('P') if app.view.remap_input.is_empty() => {
        if let Some(text) = app.get_clipboard_text() {
          let trimmed = text.trim_matches(|c| c == '\r' || c == '\n').to_string();
          for c in trimmed.chars() {
            app.view.remap_input.push(c);
          }
          app.set_status_message("Vim 'p' pasted YouTube link/ID! Press Enter to map.".to_string(), 4);
        } else {
          app.view.remap_input.push('p');
        }
      }
      Key::Backspace => {
        app.view.remap_input.pop();
      }
      Key::Ctrl('u') => {
        app.view.remap_input.clear();
      }
      Key::Enter => {
        let input_str: String = app.view.remap_input.iter().collect();
        let trimmed = input_str.trim();
        #[cfg(feature = "youtube")]
        let extracted_vid = crate::infra::youtube::extract_youtube_video_id(trimmed);
        #[cfg(not(feature = "youtube"))]
        let extracted_vid: Option<String> = if trimmed.len() == 11 { Some(trimmed.to_string()) } else { None };

        if let Some(vid) = extracted_vid {
          if let Some(track_id) = app.view.remap_track_id.take() {
            if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
              let entry = crate::sources::lossless_relay::db::ResolutionEntry {
                source_track_id: track_id.clone(),
                isrc: None,
                qobuz_id: None,
                tidal_id: None,
                resolution_method: "manual_remap".to_string(),
                match_confidence: 1.0,
                local_archive_path: Some(format!("youtube:{}", vid)),
                resolved_at: chrono::Utc::now().timestamp(),
              };
              let _ = db.upsert_resolution(&entry);
            }
            // Update the track in the currently open track table
            if let Some(track) = app.track_table.tracks.get_mut(app.view.track_table_index) {
              if track.uri.as_ref() == Some(&track_id) || track.id.as_ref() == Some(&track_id) {
                track.uri = Some(format!("youtube:{}", vid));
              }
            }
            app.set_status_message(format!("Successfully remapped track to YouTube [{}]!", vid), 5);
          }
          app.view.remap_modal_visible = false;
          app.view.remap_input.clear();
        } else {
          app.set_status_message("Invalid YouTube video ID or URL. Please provide an 11-char ID or YouTube URL.".to_string(), 4);
        }
      }
      Key::Char(c) => {
        app.view.remap_input.push(c);
      }
      _ => {}
    }
    return;
  }

  // Plugin popup is a modal: intercept all keys before anything else.
  if app.plugin_popup.is_some() {
    match key {
      Key::Esc | Key::Char('q') => {
        app.apply(Action::ClosePopup);
      }
      k if k == app.user_config.keys.back => {
        app.apply(Action::ClosePopup);
      }
      k if common_key_events::up_event(k, &app.user_config.keys) => {
        app.view.plugin_popup_scroll = app.view.plugin_popup_scroll.saturating_sub(1);
      }
      k if common_key_events::down_event(k, &app.user_config.keys) => {
        let max_scroll = app
          .plugin_popup
          .as_ref()
          .map(|p| p.lines.len().saturating_sub(1) as u16)
          .unwrap_or(0);
        app.view.plugin_popup_scroll = app
          .view
          .plugin_popup_scroll
          .saturating_add(1)
          .min(max_scroll);
      }
      _ => {} // swallow all other keys
    }
    return;
  }

  // Help filtering is an inline modal input. Give it priority over global
  // bindings so queries can contain keys such as d, n, p, q, or ?.
  if app.get_current_route().active_block == ActiveBlock::HelpMenu
    && (app.view.help_filter_editing || key == app.user_config.keys.search)
  {
    help_menu::handler(key, app);
    return;
  }

  // Settings has the same inline modal inputs: an open edit, the unsaved-changes
  // prompt, and the row filter. Each needs the raw key before global bindings
  // claim it, and the search binding is what opens the filter in the first place.
  if app.get_current_route().active_block == ActiveBlock::Settings
    && (app.view.settings_unsaved_prompt_visible
      || app.view.settings_edit_mode
      || app.view.settings_filter_editing
      || key == app.user_config.keys.search)
  {
    settings::handler(key, app);
    return;
  }

  // When Party popup is open, all keys go to the party handler first (so 'c' and 'l' aren't stolen by global bindings).
  if app.get_current_route().active_block == ActiveBlock::Party {
    handle_block_events(key, app);
    return;
  }

  // The DJ prompt is a typing surface, so it needs first refusal on every key —
  // otherwise global bindings eat any character that happens to be a shortcut
  // ('d' for devices, space for play/pause, and so on).
  #[cfg(feature = "ai-dj")]
  if app.get_current_route().active_block == ActiveBlock::AiDj {
    handle_block_events(key, app);
    return;
  }

  // When Create Playlist form is open, all keys go directly to the form handler
  // (so typed characters aren't stolen by global bindings like 'd', space, etc.)
  if app.get_current_route().active_block == ActiveBlock::CreatePlaylistForm {
    handle_block_events(key, app);
    return;
  }

  // The sort menu is a popup whose shortcut letters ('n', 'd', 'a', ...)
  // collide with global bindings, so it needs the keys before they are claimed.
  if app.get_current_route().active_block == ActiveBlock::SortMenu {
    handle_block_events(key, app);
    return;
  }

  // Friends has a few local keys that conflict with globals, plus inline input modes
  // that need first chance to consume typed characters.
  if should_route_friends_before_globals(key, app) {
    handle_block_events(key, app);
    return;
  }

  if app.maybe_activate_open_settings_fallback(key) {
    app.open_settings_screen();
    if app.pending_keybinding_persist_key().is_some() {
      app.push_navigation_stack(
        RouteId::Dialog,
        ActiveBlock::Dialog(crate::core::app::DialogContext::PersistKeybindingFallback),
      );
    }
    return;
  }

  let effective_open_settings = app.effective_open_settings_key();
  if key_matches_open_settings_binding(key, app.user_config.keys.open_settings)
    || key_matches_open_settings_binding(key, effective_open_settings)
  {
    app.apply(Action::Navigate(NavTarget::Settings));
    return;
  }

  // First handle any global event and then move to block event
  match key {
    Key::Esc => {
      if app.view.search_drilldown != crate::core::app::SearchDrilldown::None {
        search_results::handler(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::Playlists {
        app.pop_navigation_stack();
      } else if app.get_current_route().active_block == ActiveBlock::Settings {
        settings::handler(key, app);
      } else {
        handle_escape(app);
      }
    }
    _ if key == app.user_config.keys.jump_to_album || key == Key::Char('G') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        app.apply(Action::JumpToAlbum);
      }
    }
    _ if key == app.user_config.keys.jump_to_artist_album => {
      app.apply(Action::JumpToArtist);
    }
    _ if key == app.user_config.keys.jump_to_context => {
      app.apply(Action::JumpToContext);
    }
    // Reachable from anywhere, without a trip through the sidebar row.
    #[cfg(feature = "ai-dj")]
    _ if key == app.user_config.keys.dj_open => {
      ai_dj::open(app);
    }
    #[cfg(feature = "ai-dj")]
    _ if key == app.user_config.keys.dj_toggle_auto_queue => {
      app.apply(Action::ToggleDjAutoQueue);
    }
    #[cfg(feature = "ai-dj")]
    _ if key == app.user_config.keys.dj_vibe_shift => {
      app.apply(Action::DjVibeShift);
    }
    #[cfg(feature = "ai-dj")]
    _ if key == app.user_config.keys.dj_toggle_fresh_only => {
      app.apply(Action::ToggleDjFreshOnly);
    }
    #[cfg(feature = "ai-dj")]
    _ if key == app.user_config.keys.dj_pick_model => {
      app.apply(Action::OpenDjSetup);
    }
    _ if key == app.user_config.keys.manage_devices || key == Key::Char('i') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::SelectDevice
        || app.get_current_route().id == RouteId::SelectedDevice
      {
        app.pop_navigation_stack();
      } else {
        app.apply(Action::Navigate(NavTarget::Devices));
      }
    }
    Key::Char('d') | Key::Char('D') | Key::Char('x') | Key::Char('X') => {
      handle_block_events(key, app);
    }
    _ if key == app.user_config.keys.decrease_volume => {
      app.apply(Action::VolumeDown);
    }
    _ if key == app.user_config.keys.increase_volume => {
      app.apply(Action::VolumeUp);
    }
    // Press space to toggle playback
    _ if key == app.user_config.keys.toggle_playback => {
      app.apply(Action::TogglePlayback);
    }
    _ if key == app.user_config.keys.seek_backwards => {
      app.apply(Action::SeekBackward);
    }
    _ if key == app.user_config.keys.seek_forwards => {
      app.apply(Action::SeekForward);
    }
    _ if key == app.user_config.keys.next_track => {
      app.apply(Action::NextTrack);
    }
    _ if key == app.user_config.keys.previous_track => {
      if let Some(clip) = app.get_clipboard_text() {
        let trimmed = clip.trim_matches(|c| c == '\r' || c == '\n').to_string();
        if trimmed.starts_with("http://")
          || trimmed.starts_with("https://")
          || trimmed.starts_with("spotify:")
          || trimmed.contains("youtube.com")
          || trimmed.contains("youtu.be")
          || trimmed.contains("spotify.com")
        {
          app.view.url_import_visible = true;
          app.view.url_import_input = trimmed.chars().collect();
          app.set_status_message("Vim 'p' pasted playlist URL! Press Enter to import.".to_string(), 5);
          return;
        }
      }
      app.apply(Action::PreviousTrack);
    }
    Key::Ctrl('v') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if let Some(clip) = app.get_clipboard_text() {
        let trimmed = clip.trim_matches(|c| c == '\r' || c == '\n').to_string();
        app.view.url_import_visible = true;
        app.view.url_import_input = trimmed.chars().collect();
        app.set_status_message("Pasted clipboard into URL import! Press Enter to import.".to_string(), 5);
      }
    }
    _ if key == app.user_config.keys.force_previous_track => {
      app.apply(Action::ForcePreviousTrack);
    }
    _ if key == app.user_config.keys.help => {
      help_menu::open(app);
    }
    _ if key == app.user_config.keys.show_queue => {
      app.apply(Action::Navigate(NavTarget::Queue));
    }

    _ if key == app.user_config.keys.shuffle || key == Key::Char('R') => {
      app.apply(Action::ToggleShuffle);
    }
    _ if key == app.user_config.keys.repeat || key == Key::Char('r') => {
      app.apply(Action::CycleRepeat);
    }
    Key::Ctrl('f')
      if app.get_current_route().active_block == ActiveBlock::TrackTable
        && (app.is_playlist_track_table_context()
          || app.track_table.context == Some(TrackTableContext::SavedTracks)) =>
    {
      let input_context = if app.is_playlist_track_table_context() {
        InputContext::PlaylistTrackSearch
      } else {
        InputContext::SavedTrackSearch
      };

      app.view.input.clear();
      app.view.input_idx = 0;
      app.view.input_cursor_position = 0;
      app.view.input_context = input_context;
      app.set_current_route_state(Some(ActiveBlock::Input), Some(ActiveBlock::Input));
    }
    _ if key == app.user_config.keys.search => {
      open_global_search(app);
    }
    _ if key == app.user_config.keys.copy_song_url || key == Key::Char('y') => {
      copy_url(app, Action::CopyUrl(CopyTarget::CurrentSong));
    }
    _ if key == app.user_config.keys.copy_album_url || key == Key::Char('Y') => {
      copy_url(app, Action::CopyUrl(CopyTarget::CurrentAlbum));
    }
    _ if key == app.user_config.keys.audio_analysis => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::Analysis
        || app.get_current_route().id == RouteId::Analysis
      {
        app.pop_navigation_stack();
      } else {
        app.apply(Action::Navigate(NavTarget::Analysis));
      }
    }
    _ if key == app.user_config.keys.lyrics_view || key == Key::Char('B') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::LyricsView
        || app.get_current_route().id == RouteId::LyricsView
      {
        app.pop_navigation_stack();
      } else {
        app.apply(Action::Navigate(NavTarget::Lyrics));
      }
    }
    _ if key == app.user_config.keys.miniplayer_view => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::MiniPlayer {
        app.pop_navigation_stack();
      } else {
        app.apply(Action::Navigate(NavTarget::MiniPlayer));
      }
    }
    Key::Char('M') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::MiniPlayer {
        app.pop_navigation_stack();
      } else {
        app.apply(Action::Navigate(NavTarget::MiniPlayer));
      }
    }
    #[cfg(feature = "cover-art")]
    _ if key == app.user_config.keys.cover_art_view || key == Key::Char('N') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::CoverArtView
        || app.get_current_route().id == RouteId::CoverArtView
      {
        app.pop_navigation_stack();
      } else {
        app.push_navigation_stack(RouteId::CoverArtView, ActiveBlock::CoverArtView);
      }
    }
    Key::Char('O') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        app.view.karaoke_stage_visible = !app.view.karaoke_stage_visible;
        if app.view.karaoke_stage_visible && app.view.active_karaoke_timeline.is_none() {
          app.dispatch(crate::infra::network::IoEvent::RefetchKaraokeLyrics { force: false });
        }
      }
    }
    Key::Char('I') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        app.view.url_import_visible = !app.view.url_import_visible;
        if app.view.url_import_visible {
          app.view.url_import_input.clear();
          app.view.url_import_status = None;
          app.view.url_import_in_progress = false;
        }
      }
    }
    Key::Char('i') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        common_key_events::focus_sidebar(app);
      }
    }
    Key::Char('A') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::CreatePlaylistForm {
        app.pop_navigation_stack();
      } else {
        app.push_navigation_stack(RouteId::CreatePlaylist, ActiveBlock::CreatePlaylistForm);
      }
    }
    Key::Char('\'') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else if app.get_current_route().active_block == ActiveBlock::SortMenu {
        app.pop_navigation_stack();
      } else {
        sort_menu::open_sort_menu(app, crate::core::sort::SortContext::PlaylistTracks);
      }
    }
    Key::Char('w') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        app.apply(Action::OpenAddPlayingTrackDialog);
      }
    }
    Key::Char('W') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        let active = app.get_current_route().active_block;
        if active == ActiveBlock::TrackTable
          || active == ActiveBlock::SearchResultBlock
          || active == ActiveBlock::AlbumTracks
          || active == ActiveBlock::Playlists
        {
          handle_block_events(key, app);
        } else {
          app.apply(Action::OpenAddTrackDialog);
        }
      }
    }
    Key::Char('P') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        app.view.karaoke_provider_index = (app.view.karaoke_provider_index + 1) % crate::core::app::LYRIC_PROVIDERS.len();
        let provider_name = crate::core::app::LYRIC_PROVIDERS[app.view.karaoke_provider_index];
        app.set_status_message(format!("Switched Lyric Provider to {} — re-fetching...", provider_name), 4);
        app.dispatch(crate::infra::network::IoEvent::RefetchKaraokeLyrics { force: true });
      }
    }
    Key::Char('s') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        handle_save_single_track(app);
      }
    }
    Key::Char('S') | Key::Char('Z') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        handle_save_album_or_playlist(app);
      }
    }
    _ if key == app.user_config.keys.generate_recap => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        app.apply(Action::GenerateRecap);
      }
    }
    Key::Ctrl('r') | Key::Ctrl('R') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        app.apply(Action::RunPlaylistSync);
      }
    }
    // Resize sidebar: { decreases, } increases width
    Key::Char('{') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        resize::decrease_sidebar_width(app);
      }
    }
    Key::Char('}') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        resize::increase_sidebar_width(app);
      }
    }
    // Resize playbar or library/playlist split depending on hovered pane:
    // ( decreases height, ) increases height
    Key::Char('(') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        match app.get_current_route().hovered_block {
          ActiveBlock::Library | ActiveBlock::MyPlaylists => resize::decrease_library_height(app),
          _ => resize::decrease_playbar_height(app),
        }
      }
    }
    Key::Char(')') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        match app.get_current_route().hovered_block {
          ActiveBlock::Library | ActiveBlock::MyPlaylists => resize::increase_library_height(app),
          _ => resize::increase_playbar_height(app),
        }
      }
    }
    // Reset all pane sizes to defaults
    Key::Char('|') => {
      if is_input_mode(app) {
        handle_block_events(key, app);
      } else {
        resize::reset_layout(app);
      }
    }
    _ => handle_block_events(key, app),
  }
}

/// Open the global search box. Never rewrite the error frame in place to
/// `{Error, Input}`: push dedupes on the top frame's id, so a surviving Error
/// frame makes every later error silently fail to render. Dismiss the error
/// and open search on the screen below.
pub(super) fn focus_global_search(app: &mut App) {
  if app.get_current_route().id == RouteId::Error {
    app.clear_api_error();
  }
  app.view.input_context = InputContext::GlobalSearch;
  app.set_current_route_state(Some(ActiveBlock::Input), Some(ActiveBlock::Input));
}

/// Focuses the search input in EchoVena. Search is always available via YouTube & live catalog.
pub(super) fn open_global_search(app: &mut App) -> bool {
  focus_global_search(app);
  true
}

/// Archives the single highlighted or currently playing track.
pub fn handle_save_single_track(app: &mut App) {
  let active = app.get_current_route().active_block;

  // Context-aware target resolution:
  // 1. Try resolving highlighted track from cursor in active block
  let target_from_cursor: Option<crate::sources::lossless_relay::db::PlaylistTrack> = match active {
    ActiveBlock::AlbumTracks => {
      album_tracks::selected_album_track(app).map(|t| {
        let idx = match app.album_table_context {
          AlbumTableContext::Full => app.view.saved_album_tracks_index,
          AlbumTableContext::Simplified => app.selected_album_simplified.as_ref().map_or(0, |s| s.selected_index),
        };
        crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, idx, "album")
      })
    }
    ActiveBlock::TrackTable => {
      app.track_table.tracks.get(app.view.track_table_index).map(|t| {
        crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, app.view.track_table_index, "library")
      })
    }
    ActiveBlock::SearchResultBlock => {
      // First check YouTube live search items
      app.selected_live_search_item().map(|item| {
        let t = item.to_track_info();
        crate::sources::lossless_relay::archiver::track_info_to_playlist_track(&t, app.live_search_selected_index, "search")
      }).or_else(|| {
        // Legacy Spotify search fallback
        if let crate::core::app::SearchResultBlock::SongSearch = app.view.search_selected_block {
          app.view.search_selected_tracks_index.and_then(|idx| {
            app.search_results().tracks.as_ref().and_then(|p| p.items.get(idx)).map(|t| {
              crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, idx, "search")
            })
          })
        } else {
          None
        }
      })
    }
    ActiveBlock::Playlists | ActiveBlock::MyPlaylists => {
      let tree = playlist::get_playlist_tree_items(app);
      if let Some(idx) = app.view.selected_playlist_index {
        if let Some(playlist::PlaylistTreeItem::TrackLeaf { playlist_id, track_id, title, artist, duration_ms, position, .. }) = tree.get(idx) {
          Some(crate::sources::lossless_relay::db::PlaylistTrack {
            playlist_id: playlist_id.clone(),
            position: *position as i64,
            source_track_id: track_id.clone(),
            raw_title: title.clone(),
            clean_title: title.clone(),
            artist: artist.clone(),
            album: None,
            duration_ms: *duration_ms as i64,
            isrc: None,
            cover_art_url: None,
          })
        } else {
          None
        }
      } else {
        None
      }
    }
    _ => None,
  };

  // 2. Also check if live search dropdown has an active selection
  let target_from_cursor = target_from_cursor.or_else(|| {
    app.selected_live_search_item().map(|item| {
      let t = item.to_track_info();
      crate::sources::lossless_relay::archiver::track_info_to_playlist_track(&t, 0, "search")
    })
  });

  // 3. Fall back unconditionally to currently playing track (YouTube, Queue, Lossless, Snapshot)
  let track_to_archive = target_from_cursor.or_else(|| {
    #[cfg(feature = "audio-decode-queue")]
    let queue_track = if let Some(crate::infra::queue::QueueNowPlaying::Decoded(d)) = app.queue_now.as_ref() {
      Some(crate::sources::lossless_relay::archiver::track_info_to_playlist_track(&d.track, 0, "queue"))
    } else if let Some(crate::infra::queue::QueueNowPlaying::Spotify { track }) = app.queue_now.as_ref() {
      Some(crate::sources::lossless_relay::archiver::track_info_to_playlist_track(track, 0, "queue"))
    } else {
      None
    };
    #[cfg(not(feature = "audio-decode-queue"))]
    let queue_track = None;

    #[cfg(feature = "youtube")]
    let yt_track = app.youtube_playback.as_ref().and_then(|yt| {
      yt.current().map(|t| {
        crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, 0, "youtube_playback")
      })
    });
    #[cfg(not(feature = "youtube"))]
    let yt_track = None;

    queue_track
      .or(yt_track)
      .or_else(|| app.lossless_playback.as_ref().and_then(|l| l.current_track.clone()))
      .or_else(|| {
        crate::infra::media_metadata::tracking_playback_snapshot(app).map(|snapshot| {
          crate::sources::lossless_relay::db::PlaylistTrack {
            playlist_id: "now_playing".to_string(),
            position: 0,
            source_track_id: snapshot.item_uri.clone().unwrap_or_else(|| format!("track:{}", snapshot.metadata.title)),
            raw_title: snapshot.metadata.title.clone(),
            clean_title: snapshot.metadata.title.clone(),
            artist: snapshot.primary_artist(),
            album: if snapshot.metadata.album.is_empty() { None } else { Some(snapshot.metadata.album) },
            duration_ms: snapshot.metadata.duration_ms as i64,
            isrc: None,
            cover_art_url: snapshot.metadata.image_url,
          }
        })
      })
  });

  if let Some(track) = track_to_archive {
    let title = track.clean_title.clone();
    let artist = track.artist.clone();
    let dl_source = app.active_download_source;
    app.set_status_message(format!("Archiving '{} - {}' [{}] to ~/Music/EchoVena...", title, artist, dl_source.label()), 4);
    tokio::spawn(async move {
      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        if let Some(root) = crate::core::paths::music_archive_dir() {
          let _ = crate::sources::lossless_relay::archiver::resolve_and_archive_track_with_source(&db, &track, &root, dl_source).await;
        }
      }
    });
  } else {
    app.set_status_message("No track playing or selected to archive".to_string(), 4);
  }
}

/// Archives the entire open or currently playing album or playlist.
pub fn handle_save_album_or_playlist(app: &mut App) {
  let active = app.get_current_route().active_block;

  // 1. Check if inside AlbumTracks block
  let mut album_tracks_to_archive: Vec<crate::sources::lossless_relay::db::PlaylistTrack> = Vec::new();
  let mut album_name = String::new();

  if active == ActiveBlock::AlbumTracks {
    match app.album_table_context {
      AlbumTableContext::Full => {
        if let Some(selected_album) = &app.selected_album_full {
          album_name = selected_album.album.name.clone();
          album_tracks_to_archive = selected_album.album.tracks.iter().enumerate().map(|(pos, t)| {
            crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, pos, &album_name)
          }).collect();
        }
      }
      AlbumTableContext::Simplified => {
        if let Some(selected_album) = &app.selected_album_simplified {
          album_name = selected_album.album.name.clone();
          album_tracks_to_archive = selected_album.tracks.items.iter().enumerate().map(|(pos, t)| {
            crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, pos, &album_name)
          }).collect();
        }
      }
    }
  }

  // 2. Check if inside Playlists block
  if album_tracks_to_archive.is_empty() && (active == ActiveBlock::Playlists || active == ActiveBlock::MyPlaylists) {
    let tree = playlist::get_playlist_tree_items(app);
    if let Some(idx) = app.view.selected_playlist_index {
      if let Some(playlist::PlaylistTreeItem::PlaylistHeader { id, title, .. }) = tree.get(idx) {
        album_name = title.clone();
        if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
          if let Ok(tracks) = db.get_playlist_tracks(id) {
            album_tracks_to_archive = tracks;
          }
        }
      }
    }
  }

  // 3. Check track table
  if album_tracks_to_archive.is_empty() && !app.track_table.tracks.is_empty() {
    album_name = app.track_table.context.as_ref().map(|_| "Current Tracklist".to_string()).unwrap_or_else(|| "EchoVena Album".to_string());
    album_tracks_to_archive = app.track_table.tracks.iter().enumerate().map(|(pos, t)| {
      crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, pos, &album_name)
    }).collect();
  }

  // 4. Check YouTube playback queue
  #[cfg(feature = "youtube")]
  if album_tracks_to_archive.is_empty() {
    if let Some(yt) = app.youtube_playback.as_ref() {
      if !yt.tracks.is_empty() {
        album_name = "YouTube Queue".to_string();
        album_tracks_to_archive = yt.tracks.iter().enumerate().map(|(pos, t)| {
          crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, pos, &album_name)
        }).collect();
      }
    }
  }

  // 5. If we have tracks, batch archive them now!
  if !album_tracks_to_archive.is_empty() {
    let count = album_tracks_to_archive.len();
    let name_owned = album_name.clone();
    let dl_source = app.active_download_source;
    app.set_status_message(format!("Batch archiving {} tracks from '{}' [{}] to ~/Music/EchoVena...", count, name_owned, dl_source.label()), 4);
    tokio::spawn(async move {
      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        if let Some(root) = crate::core::paths::music_archive_dir() {
          let mut archived_paths = Vec::new();
          for trk in &album_tracks_to_archive {
            if let Ok(path) = crate::sources::lossless_relay::archiver::resolve_and_archive_track_with_source(&db, trk, &root, dl_source).await {
              archived_paths.push(path);
            }
          }
          let _ = crate::sources::lossless_relay::archiver::generate_m3u8_playlist_to(&name_owned, &album_tracks_to_archive, &archived_paths, &root);
        }
      }
    });
    return;
  }

  // 6. Otherwise check playing track to resolve its album
  #[cfg(feature = "youtube")]
  let playing_yt = app.youtube_playback.as_ref().and_then(|yt| yt.current().cloned());
  #[cfg(not(feature = "youtube"))]
  let playing_yt: Option<crate::core::plugin_api::TrackInfo> = None;

  let playing_track = playing_yt.or_else(|| {
    app.lossless_playback.as_ref().and_then(|l| l.current_track.as_ref()).map(|pl| crate::core::plugin_api::TrackInfo {
      generated_name: None,
      uri: Some(pl.source_track_id.clone()),
      name: pl.clean_title.clone(),
      artists: vec![pl.artist.clone()],
      album: pl.album.clone().unwrap_or_default(),
      duration_ms: pl.duration_ms as u64,
      id: Some(pl.source_track_id.clone()),
      album_id: None,
      artist_refs: vec![],
      is_playable: true,
      is_local: false,
      track_number: 0,
      explicit: false,
      image_url: pl.cover_art_url.clone(),
      release_date: None,
    })
  });

  if let Some(trk) = playing_track {
    let alb_id_opt = trk.album_id.clone();
    let alb_name_opt = if trk.album.is_empty() { None } else { Some(trk.album.clone()) };
    let _trk_title = trk.name.clone();
    let trk_artist = trk.artists.first().cloned().unwrap_or_default();
    let dl_source = app.active_download_source;

    if let Some(alb_id) = alb_id_opt {
      app.set_status_message(format!("Fetching album tracks for '{}' to archive...", trk.album), 4);
      tokio::spawn(async move {
        let importer = crate::sources::lossless_relay::youtube_importer::YouTubeImporter::new();
        if let Ok((title, _artist, _yr, _cover, tracks)) = importer.fetch_innertube_album(&alb_id).await {
          let pl_tracks: Vec<crate::sources::lossless_relay::db::PlaylistTrack> = tracks.iter().enumerate().map(|(pos, t)| {
            crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, pos, &title)
          }).collect();
          if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
            if let Some(root) = crate::core::paths::music_archive_dir() {
              let mut archived_paths = Vec::new();
              for t in &pl_tracks {
                if let Ok(path) = crate::sources::lossless_relay::archiver::resolve_and_archive_track_with_source(&db, t, &root, dl_source).await {
                  archived_paths.push(path);
                }
              }
              let _ = crate::sources::lossless_relay::archiver::generate_m3u8_playlist_to(&title, &pl_tracks, &archived_paths, &root);
            }
          }
        }
      });
    } else if let Some(alb_name) = alb_name_opt {
      app.set_status_message(format!("Searching album '{}' by {} to archive...", alb_name, trk_artist), 4);
      let query = format!("{} {}", trk_artist, alb_name);
      tokio::spawn(async move {
        let client = crate::sources::youtube_search::InnerTubeClient::new();
        if let Ok(results) = client.search(&query, crate::sources::youtube_search::SearchScope::Album).await {
          if let Some(first_album) = results.into_iter().find(|it| it.scope == crate::sources::youtube_search::SearchScope::Album && it.playlist_id.is_some()) {
            if let Some(bid) = first_album.playlist_id {
              let importer = crate::sources::lossless_relay::youtube_importer::YouTubeImporter::new();
              if let Ok((title, _artist, _yr, _cover, tracks)) = importer.fetch_innertube_album(&bid).await {
                let pl_tracks: Vec<crate::sources::lossless_relay::db::PlaylistTrack> = tracks.iter().enumerate().map(|(pos, t)| {
                  crate::sources::lossless_relay::archiver::track_info_to_playlist_track(t, pos, &title)
                }).collect();
                if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
                  if let Some(root) = crate::core::paths::music_archive_dir() {
                    let mut archived_paths = Vec::new();
                    for t in &pl_tracks {
                      if let Ok(path) = crate::sources::lossless_relay::archiver::resolve_and_archive_track_with_source(&db, t, &root, dl_source).await {
                        archived_paths.push(path);
                      }
                    }
                    let _ = crate::sources::lossless_relay::archiver::generate_m3u8_playlist_to(&title, &pl_tracks, &archived_paths, &root);
                  }
                }
                return;
              }
            }
          }
        }
        // Fall back to archiving the single playing track
        let single = crate::sources::lossless_relay::archiver::track_info_to_playlist_track(&trk, 0, &alb_name);
        if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
          if let Some(root) = crate::core::paths::music_archive_dir() {
            let _ = crate::sources::lossless_relay::archiver::resolve_and_archive_track_with_source(&db, &single, &root, dl_source).await;
          }
        }
      });
    } else {
      handle_save_single_track(app);
    }
  } else {
    app.set_status_message("No playlist or track list open to batch archive".to_string(), 4);
  }
}

/// The copy keys copy the current playing song URL to the clipboard.
fn copy_url(app: &mut App, copy: Action) {
  match app.playback_owner() {
    PlaybackOwner::Spotify | PlaybackOwner::NativeSpotify => {
      let has_item = app
        .current_playback_context
        .as_ref()
        .is_some_and(|context| context.item.is_some());
      if has_item {
        app.apply(copy);
      } else {
        app.set_status_message(app.strings().status.app.nothing_playing, 4);
      }
    }
    PlaybackOwner::Decoded | PlaybackOwner::Queue => {
      let yt_info: Option<crate::core::plugin_api::TrackInfo> = {
        #[cfg(feature = "youtube")]
        {
          app.youtube_playback.as_ref().and_then(|yt| yt.current().cloned())
        }
        #[cfg(not(feature = "youtube"))]
        {
          None
        }
      };
      if let Some(track) = yt_info {
        let url = if let Some(ref uri) = track.uri {
          #[cfg(feature = "youtube")]
          if let Ok(vid) = crate::infra::youtube::video_id_from_uri(uri) {
            format!("https://www.youtube.com/watch?v={}", vid)
          } else {
            uri.clone()
          }
          #[cfg(not(feature = "youtube"))]
          uri.clone()
        } else {
          format!("https://www.youtube.com/results?search_query={}", track.name)
        };
        if let Some(clipboard) = &mut app.clipboard {
          let _ = clipboard.set_text(url.clone());
        }
        app.set_status_message(format!("Copied YouTube URL to clipboard: {}", url), 4);
      } else {
        app.set_status_message(app.strings().status.app.nothing_playing, 4);
      }
    }
    PlaybackOwner::None | PlaybackOwner::AppleMusic => {
      app.set_status_message(app.strings().status.app.nothing_playing, 4);
    }
  }
}

fn is_input_mode(app: &App) -> bool {
  matches!(
    app.get_current_route().active_block,
    ActiveBlock::Input
      | ActiveBlock::Dialog(_)
      | ActiveBlock::AnnouncementPrompt
      | ActiveBlock::ExitPrompt
      | ActiveBlock::CreatePlaylistForm
      | ActiveBlock::RecapPrompt
      | ActiveBlock::CommunityPinPrompt
  ) || {
    #[cfg(feature = "ai-dj")]
    {
      app.get_current_route().active_block == ActiveBlock::AiDj
    }
    #[cfg(not(feature = "ai-dj"))]
    {
      false
    }
  }
}

// Handle event for the current active block
fn handle_block_events(key: Key, app: &mut App) {
  let current_route = app.get_current_route();
  match current_route.active_block {
    ActiveBlock::Analysis => {
      analysis::handler(key, app);
    }
    ActiveBlock::ArtistBlock => {
      artist::handler(key, app);
    }
    ActiveBlock::Input => {
      input::handler(key, app);
    }
    ActiveBlock::MyPlaylists | ActiveBlock::Playlists => {
      playlist::handler(key, app);
    }
    ActiveBlock::TrackTable => {
      track_table::handler(key, app);
    }
    ActiveBlock::EpisodeTable => {
      episode_table::handler(key, app);
    }
    ActiveBlock::HelpMenu => {
      help_menu::handler(key, app);
    }
    ActiveBlock::Error => {
      error_screen::handler(key, app);
    }
    ActiveBlock::SelectDevice => {
      select_device::handler(key, app);
    }
    ActiveBlock::SearchResultBlock => {
      search_results::handler(key, app);
    }
    ActiveBlock::Home => {
      home::handler(key, app);
    }
    ActiveBlock::AlbumList => {
      album_list::handler(key, app);
    }
    ActiveBlock::AlbumTracks => {
      album_tracks::handler(key, app);
    }
    ActiveBlock::Library => {
      library::handler(key, app);
    }
    ActiveBlock::Empty => {
      empty::handler(key, app);
    }
    ActiveBlock::RecentlyPlayed => {
      recently_played::handler(key, app);
    }
    ActiveBlock::Artists => {
      artists::handler(key, app);
    }
    ActiveBlock::LocalBrowser => {
      local_browser::handler(key, app);
    }
    ActiveBlock::Discover => {
      discover::handler(key, app);
    }
    ActiveBlock::Podcasts => {
      podcasts::handler(key, app);
    }
    ActiveBlock::PlayBar => {
      playbar::handler(key, app);
    }
    ActiveBlock::LyricsView => {
      lyrics_view::handler(key, app);
    }
    ActiveBlock::MiniPlayer => {
      miniplayer::handler(key, app);
    }
    ActiveBlock::CoverArtView => {
      #[cfg(feature = "cover-art")]
      cover_art_view::handler(key, app);
    }
    ActiveBlock::Dialog(_) => {
      dialog::handler(key, app);
    }

    ActiveBlock::AnnouncementPrompt => {
      announcement_prompt::handler(key, app);
    }
    ActiveBlock::ExitPrompt => {}
    ActiveBlock::Settings => {
      settings::handler(key, app);
    }
    ActiveBlock::SortMenu => {
      sort_menu::handler(key, app);
    }
    ActiveBlock::Queue => {
      queue_menu::handler(key, app);
    }
    ActiveBlock::Party => {
      party::handler(key, app);
    }
    ActiveBlock::CreatePlaylistForm => {
      create_playlist::handler(key, app);
    }
    ActiveBlock::Friends => {
      friends::handler(key, app);
    }
    ActiveBlock::Stats => {
      stats::handler(key, app);
    }
    ActiveBlock::PlaylistSync => {
      playlist_sync::handler(key, app);
    }
    #[cfg(feature = "ai-dj")]
    ActiveBlock::AiDj => {
      ai_dj::handler(key, app);
    }
    ActiveBlock::RecapPrompt => {
      recap_prompt::handler(key, app);
    }
    ActiveBlock::CommunityPinPrompt => {
      community_pin_prompt::handler(key, app);
    }
    ActiveBlock::PluginScreen => {
      plugin_screen::handler(key, app);
    }
  }
}

/// Unified Back/Esc handler to clear active track filters before exiting.
///
/// Handles playlist filters first (saved tracks are subsumed).
/// Returns `true` if a filter was cleared.
pub(crate) fn clear_active_track_filter(app: &mut App) -> bool {
  if app.is_playlist_track_filter_active() {
    app.clear_playlist_track_filter();
    true
  } else if app.is_saved_track_filter_active() {
    app.clear_saved_track_filter();
    true
  } else {
    false
  }
}

fn handle_escape(app: &mut App) {
  match app.get_current_route().active_block {
    // Delegated rather than duplicated, the way `ActiveBlock::Friends` is below.
    // The DJ's Esc is now three-state (step back through the picker, clear a
    // half-typed prompt, leave the screen), so a second copy of the rule here would
    // go stale the moment either side changed.
    #[cfg(feature = "ai-dj")]
    ActiveBlock::AiDj => ai_dj::handler(Key::Esc, app),
    ActiveBlock::SearchResultBlock => {
      if app.view.search_drilldown != crate::core::app::SearchDrilldown::None {
        match &app.view.search_drilldown {
          crate::core::app::SearchDrilldown::AlbumDetail { from_artist: Some(prev), .. } => {
            app.view.search_drilldown = *prev.clone();
          }
          _ => {
            app.view.search_drilldown = crate::core::app::SearchDrilldown::None;
          }
        }
      } else {
        app.view.search_selected_block = SearchResultBlock::Empty;
        app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));
      }
    }
    ActiveBlock::Playlists => {
      app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));
    }
    ActiveBlock::ArtistBlock => {
      if let Some(artist) = &mut app.artist {
        artist.artist_selected_block = ArtistBlock::Empty;
      }
    }
    ActiveBlock::Error => {
      app.pop_navigation_stack();
    }
    ActiveBlock::Dialog(dialog_context) => {
      if dialog_context == crate::core::app::DialogContext::PersistKeybindingFallback {
        app.set_status_message(app.strings().status.app.shortcut_session_only, 4);
      }
      app.pop_navigation_stack();
      app.clear_dialog_state();
    }
    ActiveBlock::HelpMenu => {
      if app.view.help_filter_editing || !app.view.help_filter.is_empty() {
        help_menu::clear_filter(app);
      } else {
        app.pop_navigation_stack();
      }
    }
    ActiveBlock::TrackTable => {
      if !clear_active_track_filter(app) {
        app.set_current_route_state(Some(ActiveBlock::Empty), None);
      }
    }
    ActiveBlock::Queue => {
      app.pop_navigation_stack();
    }
    ActiveBlock::Party => {
      app.pop_navigation_stack();
    }
    ActiveBlock::LyricsView => {
      // Esc first leaves lyrics browsing mode; a second Esc closes the view.
      if app.view.lyrics_view.manual_index.is_some() {
        app.view.lyrics_view.manual_index = None;
      } else {
        app.pop_navigation_stack();
      }
    }
    ActiveBlock::SelectDevice
    | ActiveBlock::CoverArtView
    | ActiveBlock::MiniPlayer
    | ActiveBlock::PluginScreen
    | ActiveBlock::Analysis => {
      app.pop_navigation_stack();
    }

    // Esc hands the popup its own dismissal, like Enter does
    ActiveBlock::AnnouncementPrompt => {
      announcement_prompt::handler(Key::Esc, app);
    }
    ActiveBlock::ExitPrompt => {}
    // Sort menu closes on escape
    ActiveBlock::SortMenu => {
      app.view.sort_menu_visible = false;
      app.view.sort_context = None;
      app.set_current_route_state(Some(ActiveBlock::Empty), None);
    }
    ActiveBlock::CreatePlaylistForm => {
      create_playlist::handler(Key::Esc, app);
    }
    ActiveBlock::Friends => {
      friends::handler(Key::Esc, app);
    }
    // "[ESC] Later" on the recap popup: dismiss without opening.
    ActiveBlock::RecapPrompt => {
      app.apply(Action::DismissRecapPrompt);
    }
    // Esc keeps the pin but still marks the prompt shown so it never re-nags.
    ActiveBlock::CommunityPinPrompt => {
      community_pin_prompt::handler(Key::Esc, app);
    }
    _ => {
      app.set_current_route_state(Some(ActiveBlock::Empty), None);
    }
  }
}

/// Test fixture: a track table for an open playlist with its sort menu open.
/// Shared by the issue #660 routing tests below.
#[cfg(test)]
fn app_with_open_playlist_sort_menu() -> App {
  use crate::core::app::TrackTableContext;
  use crate::core::sort::SortContext;

  let mut app = App::default();
  app.set_track_table(Vec::new(), TrackTableContext::MyPlaylists);
  app.push_navigation_stack(RouteId::TrackTable, ActiveBlock::TrackTable);
  sort_menu::open_sort_menu(&mut app, SortContext::PlaylistTracks);
  app
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::source::Source;
  use crate::core::app::TrackTableContext;
  use crate::core::sort::{SortContext, SortField, SortOrder};
  use crate::core::test_helpers::full_track;
  use crate::core::user_config::UserConfig;
  use crate::infra::network::IoEvent;
  use chrono::Utc;
  use rspotify::model::{
    context::{Actions, CurrentPlaybackContext},
    device::DevicePayload,
    enums::{DeviceType, RepeatState},
    idtypes::PlaylistId,
    CurrentlyPlayingType, Device, PlayableItem,
  };
  use rspotify::prelude::Id;
  use std::{
    sync::mpsc::{channel, TryRecvError},
    time::SystemTime,
  };

  // --- Sort menu shortcut routing (issue #660) ---

  #[test]
  fn sort_menu_shortcut_letters_reach_the_menu_instead_of_global_bindings() {
    // 'n' is next_track, 'd' is manage_devices and 'a' is jump_to_album
    // globally; inside the open sort menu each must sort instead.
    for (key, field) in [
      (Key::Char('n'), SortField::Name),
      (Key::Char('d'), SortField::Default),
      (Key::Char('a'), SortField::DateAdded),
    ] {
      let mut app = app_with_open_playlist_sort_menu();

      handle_app(key, &mut app);

      assert_eq!(app.sort_state(SortContext::PlaylistTracks).field, field);
      assert!(!app.view.sort_menu_visible);
    }
  }

  #[test]
  fn uppercase_sort_menu_shortcuts_reach_the_menu_too() {
    // 'R' is generate_recap globally, and the sort shortcut 'r' sorts by
    // artist: the uppercase letter must reach the menu, not the binding.
    let mut app = app_with_open_playlist_sort_menu();

    handle_app(Key::Char('R'), &mut app);

    let sort = app.sort_state(SortContext::PlaylistTracks);
    assert_eq!(sort.field, SortField::Artist);
    assert_eq!(sort.order, SortOrder::Descending);
    assert!(!app.view.sort_menu_visible);
  }

  #[test]
  fn sort_menu_letters_without_a_shortcut_keep_the_menu_open() {
    // 'x' is not a sort shortcut; the menu must stay open and not trigger
    // any global binding either.
    let mut app = app_with_open_playlist_sort_menu();

    handle_app(Key::Char('x'), &mut app);

    assert!(app.view.sort_menu_visible);
    assert_eq!(
      app.sort_state(SortContext::PlaylistTracks).field,
      SortField::Default
    );
  }

  fn friends_app() -> App {
    let mut app = App::default();
    app.push_navigation_stack(RouteId::Friends, ActiveBlock::Friends);
    app
  }

  fn seed_desk_speaker(app: &mut App) {
    app.set_devices(DevicePayload {
      devices: vec![Device {
        id: Some("device-1".to_string()),
        is_active: false,
        is_private_session: false,
        is_restricted: false,
        name: "Desk Speaker".to_string(),
        _type: DeviceType::Computer,
        volume_percent: Some(42),
      }],
    });
    app.view.selected_device_index = Some(0);
    app.push_navigation_stack(RouteId::SelectedDevice, ActiveBlock::SelectDevice);
  }

  #[test]
  fn keys_on_the_playlist_sync_block_reach_its_handler() {
    use crate::core::playlist_sync::{Endpoint, Link};
    let mut app = App::default_connected();
    app.set_playlist_sync_links(vec![Link {
      id: "aaaaaaaaaaaa".to_string(),
      master: Endpoint {
        source: crate::core::source::Source::Spotify,
        playlist_uri: "spotify:playlist:1".to_string(),
        name: "Road Trip".to_string(),
      },
      mirrors: Vec::new(),
    }]);
    app.push_navigation_stack(RouteId::PlaylistSync, ActiveBlock::PlaylistSync);

    handle_block_events(Key::Char('D'), &mut app);

    assert_eq!(
      app.get_current_route().active_block,
      ActiveBlock::Dialog(crate::core::app::DialogContext::RemovePlaylistSyncLinkConfirm)
    );
  }

  #[test]
  fn search_key_on_the_error_page_dismisses_the_error_first() {
    let mut app = App::default_connected();
    app.handle_error(anyhow::anyhow!("boom"));
    assert_eq!(app.get_current_route().id, RouteId::Error);

    handle_app(app.user_config.keys.search, &mut app);

    let route = app.get_current_route();
    assert_ne!(route.id, RouteId::Error, "the error frame must not survive");
    assert_eq!(route.active_block, ActiveBlock::Input);
    assert!(
      app.api_error().is_empty(),
      "the dismissal drops the message with the frame"
    );

    // A later error must still render: the dismissal above is what keeps the
    // navigation stack free of a deduping `{Error, ...}` frame.
    handle_app(Key::Esc, &mut app);
    app.handle_error(anyhow::anyhow!("again"));
    assert_eq!(app.get_current_route().id, RouteId::Error);
  }

  #[test]
  fn global_shift_w_adds_current_track_from_anywhere() {
    let mut app = App::default();
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    handle_app(Key::Char('W'), &mut app);

    assert_eq!(app.status_message(), Some("No track currently playing"));
  }

  #[test]
  fn global_shift_w_is_not_intercepted_in_input_mode() {
    let mut app = App::default();
    app.set_current_route_state(Some(ActiveBlock::Input), Some(ActiveBlock::Input));

    handle_app(Key::Char('W'), &mut app);

    assert_eq!(app.view.input, vec!['W']);
    assert!(app.status_message().is_none());
  }

  #[test]
  fn force_previous_track_dispatches_from_anywhere() {
    let mut app = App::default();
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    // Default force_previous_track is Key::Char('P')
    handle_app(Key::Char('P'), &mut app);

    // force_previous_track dispatches through App which requires no io_tx in tests,
    // so just confirm the route didn't change (it shouldn't navigate anywhere)
    assert_eq!(app.get_current_route().active_block, ActiveBlock::Empty);
  }

  #[test]
  fn escape_exits_device_selector() {
    let mut app = App::default();
    app.push_navigation_stack(RouteId::SelectedDevice, ActiveBlock::SelectDevice);

    handle_app(Key::Esc, &mut app);

    assert_ne!(
      app.get_current_route().active_block,
      ActiveBlock::SelectDevice
    );
  }

  #[test]
  fn enter_on_device_selector_dispatches_transfer_and_exits() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    seed_desk_speaker(&mut app);

    handle_app(Key::Enter, &mut app);

    match rx.recv().unwrap() {
      IoEvent::TransferPlaybackToDevice(device_id, persist_device_id) => {
        assert_eq!(device_id, "device-1");
        assert!(persist_device_id);
      }
      _ => panic!("unexpected event"),
    }
    assert_ne!(
      app.get_current_route().active_block,
      ActiveBlock::SelectDevice
    );
    assert_eq!(
      app.status_message(),
      Some("Switching playback to Desk Speaker")
    );
  }

  #[test]
  fn enter_on_device_selector_under_a_decoded_owner_refuses_the_transfer() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    seed_desk_speaker(&mut app);
    app.claim_decoded_sink(crate::core::source::Source::YouTube);

    handle_app(Key::Enter, &mut app);

    assert!(rx.try_recv().is_err());
    assert_eq!(app.status_message(), Some("Another source owns playback"));
  }

  #[test]
  fn global_shift_f_likes_current_track_from_anywhere() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    let track = full_track("0000000000000000000001", "Track 1");
    let expected_track_id = track.id.clone().unwrap();

    app.current_playback_context = Some(CurrentPlaybackContext {
      device: Device {
        id: Some("device-1".to_string()),
        is_active: true,
        is_private_session: false,
        is_restricted: false,
        name: "Desk Speaker".to_string(),
        _type: DeviceType::Computer,
        volume_percent: Some(42),
      },
      repeat_state: RepeatState::Off,
      shuffle_state: false,
      context: None,
      timestamp: Utc::now(),
      progress: None,
      is_playing: false,
      item: Some(PlayableItem::Track(track)),
      currently_playing_type: CurrentlyPlayingType::Track,
      actions: Actions::default(),
    });
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    // Default like_track is Key::Char('F')
    handle_app(Key::Char('F'), &mut app);

    match rx.recv().unwrap() {
      IoEvent::ToggleSaveTrack(track_id) => {
        assert_eq!(track_id, expected_track_id.uri());
      }
      _ => panic!("unexpected event"),
    }
  }

  #[test]
  fn global_shift_f_is_not_intercepted_in_input_mode() {
    let mut app = App::default();
    app.set_current_route_state(Some(ActiveBlock::Input), Some(ActiveBlock::Input));

    handle_app(Key::Char('F'), &mut app);

    // In input mode, 'F' should be added to the input buffer
    assert_eq!(app.view.input, vec!['F']);
  }

  #[test]
  fn friends_a_opens_add_dialog_before_global_album_jump() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    let track = full_track("0000000000000000000001", "Track 1");
    app.current_playback_context = Some(CurrentPlaybackContext {
      device: Device {
        id: Some("device-1".to_string()),
        is_active: true,
        is_private_session: false,
        is_restricted: false,
        name: "Desk Speaker".to_string(),
        _type: DeviceType::Computer,
        volume_percent: Some(42),
      },
      repeat_state: RepeatState::Off,
      shuffle_state: false,
      context: None,
      timestamp: Utc::now(),
      progress: None,
      is_playing: false,
      item: Some(PlayableItem::Track(track)),
      currently_playing_type: CurrentlyPlayingType::Track,
      actions: Actions::default(),
    });
    app.push_navigation_stack(RouteId::Friends, ActiveBlock::Friends);

    handle_app(Key::Char('a'), &mut app);

    assert!(app.view.friend_add_dialog_visible);
    assert_eq!(app.get_current_route().active_block, ActiveBlock::Friends);
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
  }

  #[test]
  fn friends_c_prefers_friend_code_copy_over_global_song_copy() {
    let mut app = friends_app();
    app.friend_code = Some("jay-1234".to_string());
    app.clipboard = None;

    handle_app(Key::Char('c'), &mut app);

    assert_eq!(app.status_message(), Some("Clipboard not available"));
    assert!(!app.view.friend_add_dialog_visible);
  }

  #[test]
  fn friends_search_buffer_keeps_globally_bound_characters_local() {
    let mut app = friends_app();
    app.view.friend_search_input = vec!['j'];

    handle_app(Key::Char('a'), &mut app);
    handle_app(Key::Char('c'), &mut app);

    assert_eq!(app.view.friend_search_input, vec!['j', 'a', 'c']);
    assert!(!app.view.friend_add_dialog_visible);
    assert!(app.status_message().is_none());
  }

  #[test]
  fn friends_without_local_state_still_allows_non_conflicting_globals() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    app.push_navigation_stack(RouteId::Friends, ActiveBlock::Friends);

    handle_app(app.user_config.keys.next_track, &mut app);

    match rx.recv().unwrap() {
      IoEvent::NextTrack => {}
      _ => panic!("unexpected event"),
    }
    assert!(app.view.friend_search_input.is_empty());
    assert!(!app.view.friend_add_dialog_visible);
  }

  #[test]
  fn friends_add_dialog_keeps_priority_for_conflicting_keys() {
    let mut app = friends_app();
    app.open_friend_add_dialog();

    handle_app(Key::Char('c'), &mut app);

    assert!(app.view.friend_add_dialog_visible);
    assert_eq!(app.view.friend_add_input, vec!['c']);
    assert!(app.status_message().is_none());
  }

  #[test]
  fn ctrl_f_in_playlist_track_table_opens_playlist_search_input() {
    let mut app = App::default();
    let playlist_id = PlaylistId::from_id("37i9dQZF1DX4WYpdgoIcn6")
      .unwrap()
      .into_static();
    app.track_table.context = Some(TrackTableContext::MyPlaylists);
    app.playlist_track_table_id = Some(playlist_id);
    app.push_navigation_stack(RouteId::TrackTable, ActiveBlock::TrackTable);

    handle_app(Key::Ctrl('f'), &mut app);

    assert_eq!(app.view.input_context, InputContext::PlaylistTrackSearch);
    assert_eq!(app.get_current_route().active_block, ActiveBlock::Input);
  }

  #[test]
  fn ctrl_f_in_saved_track_table_opens_saved_track_search_input() {
    let mut app = App::default();
    // The continuous saved-tracks table is what names the context.
    app.set_saved_tracks_to_table_continuous();
    app.push_navigation_stack(RouteId::TrackTable, ActiveBlock::TrackTable);

    handle_app(Key::Ctrl('f'), &mut app);

    assert_eq!(app.view.input_context, InputContext::SavedTrackSearch);
    assert_eq!(app.get_current_route().active_block, ActiveBlock::Input);
  }

  #[test]
  fn ctrl_f_off_a_track_table_leaves_the_input_context_alone() {
    let mut app = App::default();
    app.push_navigation_stack(RouteId::TrackTable, ActiveBlock::TrackTable);

    handle_app(Key::Ctrl('f'), &mut app);

    assert_eq!(app.view.input_context, InputContext::GlobalSearch);
    assert_eq!(
      app.get_current_route().active_block,
      ActiveBlock::TrackTable
    );
  }

  #[test]
  fn search_key_in_playlist_track_table_opens_global_search_input() {
    let mut app = App::default_connected();
    let playlist_id = PlaylistId::from_id("37i9dQZF1DX4WYpdgoIcn6")
      .unwrap()
      .into_static();
    app.track_table.context = Some(TrackTableContext::MyPlaylists);
    app.playlist_track_table_id = Some(playlist_id);
    app.push_navigation_stack(RouteId::TrackTable, ActiveBlock::TrackTable);

    handle_app(app.user_config.keys.search, &mut app);

    assert_eq!(app.view.input_context, InputContext::GlobalSearch);
    assert_eq!(app.get_current_route().active_block, ActiveBlock::Input);
  }

  #[test]
  fn search_key_outside_playlist_track_table_opens_global_search_input() {
    let mut app = App::default_connected();
    app.track_table.context = Some(TrackTableContext::SavedTracks);
    app.push_navigation_stack(RouteId::TrackTable, ActiveBlock::TrackTable);

    handle_app(app.user_config.keys.search, &mut app);

    assert_eq!(app.view.input_context, InputContext::GlobalSearch);
    assert_eq!(app.get_current_route().active_block, ActiveBlock::Input);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn plain_comma_fallback_opens_settings_and_prompts_to_persist() {
    let mut app = App::default();
    app.user_config.keys.open_settings = Key::Ctrl(',');
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    handle_app(Key::Char(','), &mut app);

    assert_eq!(
      app.keybinding_runtime.effective_open_settings,
      Some(Key::Alt(','))
    );
    assert_eq!(
      app.get_current_route().active_block,
      ActiveBlock::Dialog(crate::core::app::DialogContext::PersistKeybindingFallback)
    );
    assert!(app.status_message().is_some());
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn plain_comma_does_not_override_track_table_sort_menu() {
    let mut app = App::default();
    app.user_config.keys.open_settings = Key::Ctrl(',');
    app.track_table.context = Some(TrackTableContext::MyPlaylists);
    app.push_navigation_stack(RouteId::TrackTable, ActiveBlock::TrackTable);

    handle_app(Key::Char(','), &mut app);

    assert_eq!(app.get_current_route().active_block, ActiveBlock::SortMenu);
  }

  // --- U5: source-gate tests ---

  #[test]
  fn like_track_shows_hint_when_local_source() {
    let mut app = App::default();
    app.active_source = Source::Local;
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    // Default like_track key is 'F' (Shift+F)
    handle_app(app.user_config.keys.like_track, &mut app);

    assert_eq!(
      app.status_message(),
      Some("Like isn't available for Local Files")
    );
  }

  #[test]
  fn like_track_does_not_show_hint_when_spotify_source() {
    // No io_tx here so dispatch is a no-op; we just verify no status is set.
    let mut app = App::default();
    app.active_source = Source::Spotify;
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    handle_app(app.user_config.keys.like_track, &mut app);

    // No playback context, so toggle_like returns early with a different message
    // (or nothing). The important thing is it's NOT the Local-gate message.
    assert_ne!(
      app.status_message(),
      Some("Like isn't available for Local Files")
    );
  }

  #[test]
  fn global_like_key_favorites_radio_search_station() {
    use crate::core::pagination::Paged;
    use crate::core::plugin_api::TrackInfo;
    use crate::core::user_config::UserConfigPaths;

    let dir = tempfile::tempdir().unwrap();
    let mut app = App::default();
    app.active_source = Source::Radio;
    app.user_config.path_to_config = Some(UserConfigPaths {
      config_file_path: dir.path().join("config.yml"),
    });
    app.state_path = Some(dir.path().join("state.yml"));
    app.set_search_results(crate::core::app::SearchResult {
      tracks: Some(Paged {
        items: vec![TrackInfo {
          generated_name: None,
          uri: Some("radio:https://example.com/stream".to_string()),
          name: "Example FM".to_string(),
          artists: vec![],
          album: String::new(),
          duration_ms: 0,
          id: None,
          album_id: None,
          artist_refs: vec![],
          is_playable: true,
          is_local: false,
          track_number: 0,
          explicit: false,
          image_url: None,
          release_date: None,
        }],
        total: 1,
        ..Default::default()
      }),
      ..Default::default()
    });
    app.view.search_selected_tracks_index = Some(0);
    app.push_navigation_stack(RouteId::Search, ActiveBlock::SearchResultBlock);

    let favorite_key = app.user_config.keys.like_track;
    handle_app(favorite_key, &mut app);

    assert_eq!(app.runtime_state.radio_stations.len(), 1);
    assert_eq!(
      app.runtime_state.radio_stations[0].url,
      "https://example.com/stream"
    );
    assert_eq!(
      app.status_message(),
      Some("Favorited radio station: Example FM")
    );
  }

  fn spotify_track_context() -> CurrentPlaybackContext {
    CurrentPlaybackContext {
      device: Device {
        id: Some("device-1".to_string()),
        is_active: true,
        is_private_session: false,
        is_restricted: false,
        name: "Desk Speaker".to_string(),
        _type: DeviceType::Computer,
        volume_percent: Some(42),
      },
      repeat_state: RepeatState::Off,
      shuffle_state: false,
      context: None,
      timestamp: Utc::now(),
      progress: None,
      is_playing: true,
      item: Some(PlayableItem::Track(full_track(
        "0000000000000000000001",
        "Track 1",
      ))),
      currently_playing_type: CurrentlyPlayingType::Track,
      actions: Actions::default(),
    }
  }

  #[test]
  fn copy_url_keys_follow_the_playback_owner_not_the_browse_scope() {
    let mut app = App::default().under_source(Source::Local);
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    handle_app(app.user_config.keys.copy_song_url, &mut app);
    assert_eq!(app.status_message(), Some("Nothing is playing"));

    app.set_status_message("cleared", 1);
    handle_app(app.user_config.keys.copy_album_url, &mut app);
    assert_eq!(app.status_message(), Some("Nothing is playing"));

    // An idle session is not a playing track.
    let mut app = App::default_connected().under_source(Source::Local);
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));
    handle_app(app.user_config.keys.copy_album_url, &mut app);
    assert_eq!(app.status_message(), Some("Nothing is playing"));

    // A Spotify track playing under the Local scope still copies; with no
    // clipboard in a test App the copy exits without a message.
    let mut app = App::default_connected()
      .under_source(Source::Local)
      .with_playback(spotify_track_context());
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));
    handle_app(app.user_config.keys.copy_album_url, &mut app);
    assert_eq!(app.status_message(), None);
  }

  #[test]
  fn the_search_key_without_a_session_says_so_instead_of_opening_the_input() {
    let mut app = App::default();
    app.active_source = Source::Spotify;
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    handle_app(app.user_config.keys.search, &mut app);

    assert_ne!(app.get_current_route().active_block, ActiveBlock::Input);
    assert_eq!(app.status_message(), Some("Search needs a Spotify session"));
  }

  /// The DJ's own tests all call `ai_dj::handler` directly, which is the branch
  /// taken once the DJ screen already has focus (`handle_app` returns early for
  /// `ActiveBlock::AiDj`). That leaves the `dj_pick_model` arm of the global match
  /// untested, and it is the arm that makes the key work from Home, Search, Library
  /// or anywhere else — which is the only reason a global binding exists rather
  /// than a DJ-screen-local one.
  #[cfg(feature = "ai-dj")]
  #[test]
  fn the_reopen_binding_opens_the_dj_with_the_picker_from_another_screen() {
    let mut app = App::default();
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    handle_app(app.user_config.keys.dj_pick_model, &mut app);

    assert_eq!(app.get_current_route().id, RouteId::AiDj);
    assert!(
      app.dj.setup.is_some(),
      "and it arrives with the picker already up, not on an empty prompt"
    );
  }

  #[test]
  fn copy_song_url_proceeds_when_spotify_source() {
    // No clipboard in default App, so copy_song_url exits early; but no Local gate message.
    let mut app = App::default();
    app.active_source = Source::Spotify;
    app.set_current_route_state(Some(ActiveBlock::Empty), Some(ActiveBlock::Library));

    handle_app(app.user_config.keys.copy_song_url, &mut app);

    assert_ne!(
      app.status_message(),
      Some("Copy URL isn't available for Local Files")
    );
  }

  #[test]
  fn esc_dismisses_the_announcement_prompt() {
    use crate::core::app::{Announcement, AnnouncementLevel, RouteId};

    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new_with_state(
      std::sync::mpsc::channel().0,
      crate::core::user_config::UserConfig::new(),
      crate::core::state::RuntimeState::default(),
      Some(dir.path().join("state.yml")),
      false,
      crate::core::i18n::Language::En,
    );
    let announcement = |id: &str| Announcement {
      id: id.to_string(),
      title: id.to_string(),
      body: format!("{id} body"),
      level: AnnouncementLevel::Info,
      url: None,
      received_at: std::time::Instant::now(),
    };
    app.enqueue_announcements(vec![announcement("first"), announcement("second")]);
    app.push_navigation_stack(RouteId::AnnouncementPrompt, ActiveBlock::AnnouncementPrompt);

    handle_app(Key::Esc, &mut app);
    let active = app.active_announcement.as_ref().map(|a| a.id.as_str());
    assert_eq!(active, Some("second"));
    assert_eq!(app.get_current_route().id, RouteId::AnnouncementPrompt);
    assert_eq!(
      app.get_current_route().active_block,
      ActiveBlock::AnnouncementPrompt
    );

    handle_app(Key::Esc, &mut app);
    assert!(app.active_announcement.is_none());
    assert_ne!(app.get_current_route().id, RouteId::AnnouncementPrompt);
    assert_ne!(
      app.get_current_route().active_block,
      ActiveBlock::AnnouncementPrompt
    );
  }

  #[test]
  fn esc_closes_the_audio_analysis_screen() {
    let mut app = App::default();
    app.get_audio_analysis();
    assert_eq!(app.get_current_route().id, RouteId::Analysis);

    handle_app(Key::Esc, &mut app);

    assert_eq!(app.get_current_route().id, RouteId::Home);
    assert_eq!(app.get_current_route().active_block, ActiveBlock::Empty);
  }

  #[test]
  fn test_handle_paste_into_url_import_modal() {
    let mut app = App::default();
    app.view.url_import_visible = true;
    app.view.url_import_input.clear();

    let test_url = "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=123q_y";
    handle_paste(test_url.to_string(), &mut app);

    assert!(app.view.url_import_visible, "modal must remain open after paste");
    let input: String = app.view.url_import_input.iter().collect();
    assert_eq!(input, test_url);
  }

  #[test]
  fn test_handle_paste_from_normal_screen_opens_import_modal_for_url() {
    let mut app = App::default();
    app.view.url_import_visible = false;

    let test_url = "https://www.youtube.com/playlist?list=PL12345";
    handle_paste(test_url.to_string(), &mut app);

    assert!(app.view.url_import_visible, "pasting a URL on normal screen should auto-open modal");
    let input: String = app.view.url_import_input.iter().collect();
    assert_eq!(input, test_url);
  }
}
