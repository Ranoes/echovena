use super::common_key_events;
use crate::core::action::{Action, OpenTarget};
use crate::core::app::{ActiveBlock, RouteId, SidebarBlock, TrackTableContext};
use crate::core::app::{App, DialogContext, PlaylistFolderItem};
use crate::core::plugin_api::TrackInfo;
use crate::core::source::Source;
use crate::tui::event::Key;

#[derive(Clone, Debug)]
pub enum PlaylistTreeItem {
  ActionItem {
    label: String,
    action_id: usize, // 0: Add, 1: Import
  },
  PlaylistHeader {
    id: String,
    title: String,
    track_count: usize,
    platform: String,
    is_expanded: bool,
  },
  TrackLeaf {
    playlist_id: String,
    track_id: String,
    title: String,
    artist: String,
    duration_ms: u64,
    position: usize,
    uri: Option<String>,
    is_last: bool,
  },
}

/// Builds the hierarchical tree structure for playlists and their expanded tracklists.
pub fn get_playlist_tree_items(app: &App) -> Vec<PlaylistTreeItem> {
  let mut items = Vec::new();

  match app.view.playlist_panel_tab {
    // Tab 0: EchoVena / Local Playlists
    0 => {
      items.push(PlaylistTreeItem::ActionItem {
        label: "+ Add Playlist [Key A]".to_string(),
        action_id: 0,
      });
      items.push(PlaylistTreeItem::ActionItem {
        label: "+ Import URL [Key U]".to_string(),
        action_id: 1,
      });

      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        if let Ok(playlists) = db.list_imported_playlists() {
          for p in playlists.into_iter().filter(|p| p.platform == "local" || p.platform == "echovena") {
            let is_expanded = app.view.expanded_playlists.contains(&p.id);
            items.push(PlaylistTreeItem::PlaylistHeader {
              id: p.id.clone(),
              title: p.title.clone(),
              track_count: p.track_count as usize,
              platform: "local".to_string(),
              is_expanded,
            });

            if is_expanded {
              if let Ok(tracks) = db.get_playlist_tracks(&p.id) {
                if tracks.is_empty() {
                  items.push(PlaylistTreeItem::TrackLeaf {
                    playlist_id: p.id.clone(),
                    track_id: String::new(),
                    title: "(Empty playlist — press 'w' on any track to add)".to_string(),
                    artist: String::new(),
                    duration_ms: 0,
                    position: 0,
                    uri: None,
                    is_last: true,
                  });
                } else {
                  let track_count = tracks.len();
                  for (idx, t) in tracks.into_iter().enumerate() {
                    items.push(PlaylistTreeItem::TrackLeaf {
                      playlist_id: p.id.clone(),
                      track_id: t.source_track_id.clone(),
                      title: t.clean_title,
                      artist: t.artist,
                      duration_ms: t.duration_ms as u64,
                      position: idx,
                      uri: Some(t.source_track_id),
                      is_last: idx == track_count.saturating_sub(1),
                    });
                  }
                }
              }
            }
          }
        }
      }
    }
    // Tab 1: Spotify Playlists
    1 => {
      items.push(PlaylistTreeItem::ActionItem {
        label: "+ Import Spotify URL [Key U]".to_string(),
        action_id: 1,
      });

      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        if let Ok(playlists) = db.list_imported_playlists() {
          for p in playlists.into_iter().filter(|p| p.platform.starts_with("spotify")) {
            let is_expanded = app.view.expanded_playlists.contains(&p.id);
            items.push(PlaylistTreeItem::PlaylistHeader {
              id: p.id.clone(),
              title: p.title.clone(),
              track_count: p.track_count as usize,
              platform: "spotify".to_string(),
              is_expanded,
            });

            if is_expanded {
              if let Ok(tracks) = db.get_playlist_tracks(&p.id) {
                if tracks.is_empty() {
                  items.push(PlaylistTreeItem::TrackLeaf {
                    playlist_id: p.id.clone(),
                    track_id: String::new(),
                    title: "(Empty playlist — press 'w' on any track to add)".to_string(),
                    artist: String::new(),
                    duration_ms: 0,
                    position: 0,
                    uri: None,
                    is_last: true,
                  });
                } else {
                  let count = tracks.len();
                  for (idx, t) in tracks.into_iter().enumerate() {
                    items.push(PlaylistTreeItem::TrackLeaf {
                      playlist_id: p.id.clone(),
                      track_id: t.source_track_id.clone(),
                      title: t.clean_title,
                      artist: t.artist,
                      duration_ms: t.duration_ms as u64,
                      position: idx,
                      uri: Some(t.source_track_id),
                      is_last: idx == count.saturating_sub(1),
                    });
                  }
                }
              }
            }
          }
        }
      }

      for p in app.all_playlists() {
        let id_str = p.id.clone().unwrap_or_else(|| p.name.clone());
        let is_expanded = app.view.expanded_playlists.contains(&id_str);
        items.push(PlaylistTreeItem::PlaylistHeader {
          id: id_str.clone(),
          title: p.name.clone(),
          track_count: p.track_count as usize,
          platform: "spotify".to_string(),
          is_expanded,
        });

        if is_expanded {
          let mut found = false;
          if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
            if let Ok(tracks) = db.get_playlist_tracks(&id_str) {
              if !tracks.is_empty() {
                found = true;
                let count = tracks.len();
                for (idx, t) in tracks.into_iter().enumerate() {
                  items.push(PlaylistTreeItem::TrackLeaf {
                    playlist_id: id_str.clone(),
                    track_id: t.source_track_id.clone(),
                    title: t.clean_title,
                    artist: t.artist,
                    duration_ms: t.duration_ms as u64,
                    position: idx,
                    uri: Some(t.source_track_id),
                    is_last: idx == count.saturating_sub(1),
                  });
                }
              }
            }
          }
          if !found {
            items.push(PlaylistTreeItem::TrackLeaf {
              playlist_id: id_str.clone(),
              track_id: String::new(),
              title: "(Empty playlist — press 'w' on any track to add)".to_string(),
              artist: String::new(),
              duration_ms: 0,
              position: 0,
              uri: None,
              is_last: true,
            });
          }
        }
      }
    }
    // Tab 2: YouTube Playlists
    _ => {
      items.push(PlaylistTreeItem::ActionItem {
        label: "+ Import YouTube URL [Key U]".to_string(),
        action_id: 1,
      });

      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        if let Ok(playlists) = db.list_imported_playlists() {
          for p in playlists.into_iter().filter(|p| p.platform == "youtube" || p.platform == "ytmusic") {
            let is_expanded = app.view.expanded_playlists.contains(&p.id);
            items.push(PlaylistTreeItem::PlaylistHeader {
              id: p.id.clone(),
              title: p.title.clone(),
              track_count: p.track_count as usize,
              platform: "youtube".to_string(),
              is_expanded,
            });

            if is_expanded {
              if let Ok(tracks) = db.get_playlist_tracks(&p.id) {
                if tracks.is_empty() {
                  items.push(PlaylistTreeItem::TrackLeaf {
                    playlist_id: p.id.clone(),
                    track_id: String::new(),
                    title: "(Empty playlist — press 'w' on any track to add)".to_string(),
                    artist: String::new(),
                    duration_ms: 0,
                    position: 0,
                    uri: None,
                    is_last: true,
                  });
                } else {
                  let count = tracks.len();
                  for (idx, t) in tracks.into_iter().enumerate() {
                    items.push(PlaylistTreeItem::TrackLeaf {
                      playlist_id: p.id.clone(),
                      track_id: t.source_track_id.clone(),
                      title: t.clean_title,
                      artist: t.artist,
                      duration_ms: t.duration_ms as u64,
                      position: idx,
                      uri: Some(t.source_track_id),
                      is_last: idx == count.saturating_sub(1),
                    });
                  }
                }
              }
            }
          }
        }
      }

      for p in app.youtube_playlists() {
        let is_expanded = app.view.expanded_playlists.contains(&p.uri);
        items.push(PlaylistTreeItem::PlaylistHeader {
          id: p.uri.clone(),
          title: p.name.clone(),
          track_count: p.track_count as usize,
          platform: "youtube".to_string(),
          is_expanded,
        });

        if is_expanded {
          let mut found = false;
          if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
            if let Ok(tracks) = db.get_playlist_tracks(&p.uri) {
              if !tracks.is_empty() {
                found = true;
                let count = tracks.len();
                for (idx, t) in tracks.into_iter().enumerate() {
                  items.push(PlaylistTreeItem::TrackLeaf {
                    playlist_id: p.uri.clone(),
                    track_id: t.source_track_id.clone(),
                    title: t.clean_title,
                    artist: t.artist,
                    duration_ms: t.duration_ms as u64,
                    position: idx,
                    uri: Some(t.source_track_id),
                    is_last: idx == count.saturating_sub(1),
                  });
                }
              }
            }
          }
          if !found {
            if let Ok(path) = crate::infra::youtube::playlists::default_playlists_path() {
              if let Ok(file) = crate::infra::youtube::playlists::load(&path) {
                if let Some(playlist) = crate::infra::youtube::playlists::find_playlist(&file, &p.uri) {
                  let count = playlist.tracks.len();
                  if count > 0 {
                    found = true;
                    for (idx, t) in playlist.tracks.iter().enumerate() {
                      items.push(PlaylistTreeItem::TrackLeaf {
                        playlist_id: p.uri.clone(),
                        track_id: t.video_id.clone(),
                        title: t.title.clone(),
                        artist: t.channel.clone(),
                        duration_ms: t.duration_ms as u64,
                        position: idx,
                        uri: Some(format!("youtube:video:{}", t.video_id)),
                        is_last: idx == count.saturating_sub(1),
                      });
                    }
                  }
                }
              }
            }
          }
          if !found {
            items.push(PlaylistTreeItem::TrackLeaf {
              playlist_id: p.uri.clone(),
              track_id: String::new(),
              title: "(Empty playlist — press 'w' on any track to add)".to_string(),
              artist: String::new(),
              duration_ms: 0,
              position: 0,
              uri: None,
              is_last: true,
            });
          }
        }
      }
    }
  }

  items
}

/// Returns the compact playlist list items for the sidebar Playlists panel.
pub fn get_playlist_tab_items(app: &App) -> Vec<String> {
  match app.view.playlist_panel_tab {
    // Tab 0: EchoVena / Local Playlists
    0 => {
      let mut items = vec![
        "+ Add Playlist [Key A]".to_string(),
        "+ Import URL [Key U]".to_string(),
      ];
      let mut found_any = false;
      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        if let Ok(playlists) = db.list_imported_playlists() {
          for p in playlists.into_iter().filter(|p| p.platform == "local" || p.platform == "echovena") {
            found_any = true;
            items.push(format!("\u{25B6} [Local] {} ({} tracks)", p.title, p.track_count));
          }
        }
      }
      if !found_any {
        items.push("  (No local playlists yet \u{2014} press 'A' to create)".to_string());
      }
      items
    }
    // Tab 1: Spotify Playlists
    1 => {
      let mut items = vec!["+ Import Spotify URL [Key U]".to_string()];
      let mut found_any = false;
      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        if let Ok(playlists) = db.list_imported_playlists() {
          for p in playlists.into_iter().filter(|p| p.platform.starts_with("spotify")) {
            found_any = true;
            items.push(format!("\u{25B6} [Spotify] {} ({} tracks)", p.title, p.track_count));
          }
        }
      }
      for p in app.all_playlists() {
        found_any = true;
        let count = p.track_count;
        items.push(format!("\u{25B6} [Spotify] {} ({} tracks)", p.name, count));
      }
      if !found_any {
        items.push("  (No Spotify playlists imported yet \u{2014} press 'U' to import)".to_string());
      }
      items
    }
    // Tab 2: YouTube Playlists
    _ => {
      let mut items = vec!["+ Import YouTube URL [Key U]".to_string()];
      let mut found_any = false;
      for p in app.youtube_playlists() {
        found_any = true;
        items.push(format!("\u{25B6} [YouTube] {} ({} tracks)", p.name, p.track_count));
      }
      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        if let Ok(playlists) = db.list_imported_playlists() {
          for p in playlists.into_iter().filter(|p| p.platform == "youtube" || p.platform == "ytmusic") {
            found_any = true;
            items.push(format!("\u{25B6} [YouTube] {} ({} tracks)", p.title, p.track_count));
          }
        }
      }
      if !found_any {
        items.push("  (No YouTube playlists yet \u{2014} press 'U' to import)".to_string());
      }
      items
    }
  }
}

pub(crate) fn total_display_count(app: &App) -> usize {
  match app.active_source {
    Source::Local => app.local_playlists().len(),
    Source::Subsonic => app.subsonic_playlists().len(),
    Source::Radio => app.radio_stations().len(),
    Source::Qobuz => app.qobuz_playlists().len(),
    Source::AppleMusic => app.apple_music_playlists().len(),
    _ => {
      if app.get_current_route().active_block == ActiveBlock::Playlists {
        get_playlist_tree_items(app).len()
      } else {
        get_playlist_tab_items(app).len()
      }
    }
  }
}

/// Open a non-Spotify playlist's tracks in the shared track table.
pub(super) fn open_source_playlist(app: &mut App, uri: Option<String>) {
  if let Some(uri) = uri {
    app.apply(Action::Open(OpenTarget::SourcePlaylist(uri)));
  }
}

/// Local Files: open the highlighted folder's tracks in the shared track table.
fn open_local_folder(app: &mut App) {
  let uri = app
    .view
    .selected_playlist_index
    .and_then(|idx| app.local_playlists().get(idx))
    .map(|folder| folder.uri.clone());
  open_source_playlist(app, uri);
}

/// Subsonic: open the highlighted server playlist's tracks in the shared track
/// table.
fn open_subsonic_folder(app: &mut App) {
  let uri = app
    .view
    .selected_playlist_index
    .and_then(|idx| app.subsonic_playlists().get(idx))
    .map(|playlist| playlist.uri.clone());
  open_source_playlist(app, uri);
}

/// Qobuz: open the highlighted row (favorites, playlist, or album) in the
/// shared track table.
fn open_qobuz_folder(app: &mut App) {
  let uri = app
    .view
    .selected_playlist_index
    .and_then(|idx| app.qobuz_playlists().get(idx))
    .map(|playlist| playlist.uri.clone());
  open_source_playlist(app, uri);
}

/// YouTube: open the highlighted local playlist's saved videos in the shared
/// track table, or the create-playlist form on the trailing "+ New Playlist"
/// entry.
#[allow(dead_code)]
fn open_youtube_playlist(app: &mut App) {
  let Some(idx) = app.view.selected_playlist_index else {
    return;
  };
  if idx == app.youtube_playlists().len() {
    // "+ New Playlist" — reuse the create form; its name stage dispatches
    // CreateYouTubePlaylist under the YouTube source.
    app.push_navigation_stack(RouteId::CreatePlaylist, ActiveBlock::CreatePlaylistForm);
    return;
  }
  let uri = app
    .youtube_playlists()
    .get(idx)
    .map(|playlist| playlist.uri.clone());
  open_source_playlist(app, uri);
}

/// Lossless Relay: open the highlighted imported SQLite playlist in the shared
/// track table, or the URL import modal on the trailing "+ Import URL" entry.
#[allow(dead_code)]
fn open_lossless_relay_playlist(app: &mut App) {
  let Some(idx) = app.view.selected_playlist_index else {
    return;
  };
  if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
    if let Ok(playlists) = db.list_imported_playlists() {
      if idx < playlists.len() {
        let uri = playlists[idx].id.clone();
        open_source_playlist(app, Some(uri));
      } else {
        app.view.url_import_visible = true;
      }
    }
  }
}

/// Internet Radio: play the highlighted station directly. A station is a leaf,
/// not a container — there is no track list to drill into — so Enter starts the
/// stream instead of opening the track table.
fn play_radio_station(app: &mut App) {
  let Some(idx) = app.view.selected_playlist_index else {
    return;
  };
  let uri = app.radio_stations().get(idx).and_then(|s| s.uri.clone());
  if let Some(uri) = uri {
    // A one-item URI list: the radio router starts the station for both shapes.
    app.apply(Action::PlayUris {
      uris: vec![uri],
      offset: None,
    });
  }
}

fn remove_radio_station(app: &mut App) {
  let Some(idx) = app.view.selected_playlist_index else {
    app.set_status_message(app.strings().status.app.no_radio_selected.to_string(), 4);
    return;
  };
  let Some(uri) = app
    .radio_stations()
    .get(idx)
    .map(|station| station.uri.clone())
  else {
    app.set_status_message(app.strings().status.app.no_radio_selected.to_string(), 4);
    return;
  };
  let Some(uri) = uri else {
    app.set_status_message(app.strings().status.app.radio_no_url.to_string(), 4);
    return;
  };
  app.apply(Action::RemoveRadioStation(uri));
  // The clamp is a no-op for every outcome that leaves the list untouched.
  app.view.selected_playlist_index = if app.radio_stations().is_empty() {
    None
  } else {
    Some(idx.min(app.radio_stations().len() - 1))
  };
}

fn handle_delete_playlist(app: &mut App) {
  let Some(idx) = app.view.selected_playlist_index else {
    app.set_status_message("No playlist selected to delete".to_string(), 4);
    return;
  };

  if app.active_source == Source::Radio {
    remove_radio_station(app);
    return;
  }

  match app.view.playlist_panel_tab {
    // Tab 0: EchoVena Playlists
    0 => {
      if idx < 2 {
        app.set_status_message("Action item cannot be deleted".to_string(), 4);
        return;
      }
      let db_playlists = crate::sources::lossless_relay::db::Database::open_default()
        .ok()
        .and_then(|db| db.list_imported_playlists().ok())
        .unwrap_or_default();
      let db_len = db_playlists.len();
      let relative_idx = idx - 2;
      if relative_idx < db_len {
        let pl = &db_playlists[relative_idx];
        let title = pl.title.clone();
        let id = pl.id.clone();
        if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
          let _ = db.delete_imported_playlist(&id);
        }
        app.set_status_message(format!("Deleted playlist '{}' from local database", title), 4);
        let new_total = get_playlist_tab_items(app).len();
        app.view.selected_playlist_index = if new_total > 0 { Some(idx.min(new_total - 1)) } else { None };
      } else {
        let local_idx = relative_idx - db_len;
        if local_idx < app.local_playlists().len() {
          let name = app.local_playlists()[local_idx].name.clone();
          app.local_playlists_mut().remove(local_idx);
          app.set_status_message(format!("Removed local playlist '{}'", name), 4);
          let new_total = get_playlist_tab_items(app).len();
          app.view.selected_playlist_index = if new_total > 0 { Some(idx.min(new_total - 1)) } else { None };
        }
      }
    }
    // Tab 1: Spotify Playlists
    1 => {
      if idx == 0 {
        app.set_status_message("Cannot delete action item".to_string(), 4);
        return;
      }
      let spotify_db: Vec<_> = crate::sources::lossless_relay::db::Database::open_default()
        .ok()
        .and_then(|db| db.list_imported_playlists().ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.platform.starts_with("spotify"))
        .collect();
      let db_len = spotify_db.len();
      let relative_idx = idx - 1;
      if relative_idx < db_len {
        let pl = &spotify_db[relative_idx];
        let title = pl.title.clone();
        let id = pl.id.clone();
        if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
          let _ = db.delete_imported_playlist(&id);
        }
        app.set_status_message(format!("Deleted Spotify playlist '{}' from local database", title), 4);
        let new_total = get_playlist_tab_items(app).len();
        app.view.selected_playlist_index = if new_total > 0 { Some(idx.min(new_total - 1)) } else { None };
      } else {
        let spotify_idx = relative_idx - db_len;
        if let Some(playlist) = app.all_playlists().get(spotify_idx) {
          let selected_playlist = playlist.name.clone();
          app.view.dialog = Some(selected_playlist);
          app.view.confirm = false;
          app.push_navigation_stack(
            RouteId::Dialog,
            ActiveBlock::Dialog(DialogContext::PlaylistWindow),
          );
        }
      }
    }
    // Tab 2: YouTube Playlists
    _ => {
      if idx == 0 {
        app.set_status_message("Cannot delete action item".to_string(), 4);
        return;
      }
      let yt_len = app.youtube_playlists().len();
      let relative_idx = idx - 1;
      if relative_idx < yt_len {
        let playlist = app.youtube_playlists()[relative_idx].clone();
        app.dispatch(crate::infra::network::IoEvent::DeleteYouTubePlaylist(playlist.uri.clone()));
        app.set_status_message(format!("Deleted YouTube playlist '{}'", playlist.name), 4);
        let new_total = get_playlist_tab_items(app).len();
        app.view.selected_playlist_index = if new_total > 0 { Some(idx.min(new_total - 1)) } else { None };
      } else {
        let yt_db: Vec<_> = crate::sources::lossless_relay::db::Database::open_default()
          .ok()
          .and_then(|db| db.list_imported_playlists().ok())
          .unwrap_or_default()
          .into_iter()
          .filter(|p| p.platform == "youtube" || p.platform == "ytmusic")
          .collect();
        let db_idx = relative_idx - yt_len;
        if db_idx < yt_db.len() {
          let pl = &yt_db[db_idx];
          let title = pl.title.clone();
          let id = pl.id.clone();
          if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
            let _ = db.delete_imported_playlist(&id);
          }
          app.set_status_message(format!("Deleted YouTube playlist '{}'", title), 4);
          let new_total = get_playlist_tab_items(app).len();
          app.view.selected_playlist_index = if new_total > 0 { Some(idx.min(new_total - 1)) } else { None };
        }
      }
    }
  }
}

pub fn handler(key: Key, app: &mut App) {
  match key {
    Key::Esc | Key::Char('q') if app.get_current_route().active_block == ActiveBlock::Playlists => {
      app.pop_navigation_stack();
    }
    k if common_key_events::toward_content_event(k, &app.user_config)
      && app.get_current_route().active_block == ActiveBlock::MyPlaylists =>
    {
      common_key_events::focus_content(app)
    }
    k if common_key_events::down_event(k, &app.user_config.keys) => select_next(app),
    k if common_key_events::up_event(k, &app.user_config.keys) => select_previous(app),
    k if common_key_events::high_event(k) && total_display_count(app) > 0 => {
      app.view.selected_playlist_index = Some(0);
    }
    k if common_key_events::middle_event(k) => {
      let count = total_display_count(app);
      if count > 0 {
        let next_index = if count.is_multiple_of(2) {
          count.saturating_sub(1) / 2
        } else {
          count / 2
        };
        app.view.selected_playlist_index = Some(next_index);
      }
    }
    k if common_key_events::low_event(k) => {
      let count = total_display_count(app);
      if count > 0 {
        app.view.selected_playlist_index = Some(count - 1);
      }
    }
    Key::Tab => {
      app.view.playlist_panel_tab = (app.view.playlist_panel_tab + 1) % 3;
      app.view.selected_playlist_index = Some(0);
      app.view.playlist_scroll_offset.set(0);
    }
    Key::BackTab => {
      app.view.playlist_panel_tab = if app.view.playlist_panel_tab == 0 { 2 } else { app.view.playlist_panel_tab - 1 };
      app.view.selected_playlist_index = Some(0);
      app.view.playlist_scroll_offset.set(0);
    }
    Key::Char('1') => {
      app.view.playlist_panel_tab = 0;
      app.view.selected_playlist_index = Some(0);
      app.view.playlist_scroll_offset.set(0);
    }
    Key::Char('2') => {
      app.view.playlist_panel_tab = 1;
      app.view.selected_playlist_index = Some(0);
      app.view.playlist_scroll_offset.set(0);
    }
    Key::Char('3') => {
      app.view.playlist_panel_tab = 2;
      app.view.selected_playlist_index = Some(0);
      app.view.playlist_scroll_offset.set(0);
    }
    Key::Enter => {
      common_key_events::remember_sidebar_block(app, ActiveBlock::MyPlaylists);
      activate_selected(app);
    }
    Key::Char('l') | Key::Right => {
      let tree = get_playlist_tree_items(app);
      if let Some(idx) = app.view.selected_playlist_index {
        if let Some(item) = tree.get(idx) {
          match item {
            PlaylistTreeItem::PlaylistHeader { id, is_expanded, .. } => {
              if !is_expanded {
                app.view.expanded_playlists.insert(id.clone());
              } else {
                open_source_playlist(app, Some(id.clone()));
              }
            }
            PlaylistTreeItem::TrackLeaf { playlist_id, .. } => {
              open_source_playlist(app, Some(playlist_id.clone()));
            }
            _ => {}
          }
        }
      }
    }
    Key::Char('h') | Key::Left => {
      let tree = get_playlist_tree_items(app);
      if let Some(idx) = app.view.selected_playlist_index {
        if let Some(item) = tree.get(idx) {
          match item {
            PlaylistTreeItem::PlaylistHeader { id, is_expanded: true, .. } => {
              app.view.expanded_playlists.remove(id);
              let new_len = get_playlist_tree_items(app).len();
              if new_len > 0 {
                app.view.selected_playlist_index = Some(idx.min(new_len - 1));
              }
            }
            PlaylistTreeItem::PlaylistHeader { is_expanded: false, .. } => {
              common_key_events::focus_sidebar(app);
            }
            PlaylistTreeItem::TrackLeaf { playlist_id, .. } => {
              if let Some(parent_idx) = tree.iter().position(|it| matches!(it, PlaylistTreeItem::PlaylistHeader { id, .. } if id == playlist_id)) {
                app.view.selected_playlist_index = Some(parent_idx);
              }
            }
            _ => {
              common_key_events::focus_sidebar(app);
            }
          }
        }
      } else {
        common_key_events::focus_sidebar(app);
      }
    }
    Key::Char('o') | Key::Char('O') => {
      let tree = get_playlist_tree_items(app);
      if let Some(idx) = app.view.selected_playlist_index {
        if let Some(item) = tree.get(idx) {
          match item {
            PlaylistTreeItem::PlaylistHeader { id, .. } => {
              open_source_playlist(app, Some(id.clone()));
            }
            PlaylistTreeItem::TrackLeaf { playlist_id, .. } => {
              open_source_playlist(app, Some(playlist_id.clone()));
            }
            _ => {}
          }
        }
      }
    }
    Key::Char(' ') => {
      let tree = get_playlist_tree_items(app);
      if let Some(idx) = app.view.selected_playlist_index {
        if let Some(PlaylistTreeItem::PlaylistHeader { id, is_expanded, .. }) = tree.get(idx) {
          if *is_expanded {
            app.view.expanded_playlists.remove(id);
            let new_len = get_playlist_tree_items(app).len();
            if new_len > 0 {
              app.view.selected_playlist_index = Some(idx.min(new_len - 1));
            }
          } else {
            app.view.expanded_playlists.insert(id.clone());
          }
        }
      }
    }
    Key::Char('z') => {
      let tree = get_playlist_tree_items(app);
      if let Some(idx) = app.view.selected_playlist_index {
        if let Some(PlaylistTreeItem::TrackLeaf { title, artist, uri, track_id, duration_ms, .. }) = tree.get(idx) {
          let u = uri.clone().or_else(|| if !track_id.is_empty() { Some(format!("youtube:video:{}", track_id)) } else { None });
          let track_info = TrackInfo {
            uri: u,
            name: title.clone(),
            artists: vec![artist.clone()],
            album: String::new(),
            duration_ms: *duration_ms,
            id: Some(track_id.clone()),
            album_id: None,
            artist_refs: Vec::new(),
            is_playable: true,
            is_local: false,
            track_number: 1,
            explicit: false,
            image_url: None,
            release_date: None,
            generated_name: None,
          };
          app.apply(Action::QueueTrack(track_info));
          app.set_status_message(format!("Queued '{}'", title), 3);
        }
      }
    }
    Key::Char('s') => {
      let tree = get_playlist_tree_items(app);
      if let Some(idx) = app.view.selected_playlist_index {
        if let Some(PlaylistTreeItem::TrackLeaf { title, artist, track_id, duration_ms, .. }) = tree.get(idx) {
          let pl_track = crate::sources::lossless_relay::db::PlaylistTrack {
            playlist_id: "playlist".to_string(),
            position: 0,
            source_track_id: track_id.clone(),
            raw_title: title.clone(),
            clean_title: title.clone(),
            artist: artist.clone(),
            album: None,
            duration_ms: *duration_ms as i64,
            isrc: None,
            cover_art_url: None,
          };
          let dl_source = app.active_download_source;
          app.set_status_message(format!("Archiving '{} - {}' [{}] to ~/Music/EchoVena...", title, artist, dl_source.label()), 4);
          tokio::spawn(async move {
            if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
              if let Some(root) = crate::core::paths::music_archive_dir() {
                let _ = crate::sources::lossless_relay::archiver::resolve_and_archive_track_with_source(&db, &pl_track, &root, dl_source).await;
              }
            }
          });
        }
      }
    }
    Key::Char('a') | Key::Char('n') | Key::Char('N') | Key::Char('c') => {
      app.push_navigation_stack(RouteId::CreatePlaylist, ActiveBlock::CreatePlaylistForm);
    }
    Key::Char('u') | Key::Char('U') => {
      app.view.url_import_visible = true;
      app.view.url_import_input.clear();
      app.view.url_import_status = None;
      app.view.url_import_in_progress = false;
    }
    Key::Char('d') | Key::Char('D') | Key::Char('x') | Key::Delete => {
      let tree = get_playlist_tree_items(app);
      if let Some(idx) = app.view.selected_playlist_index {
        if let Some(PlaylistTreeItem::TrackLeaf { playlist_id, position, title, .. }) = tree.get(idx) {
          if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
            if let Ok(mut tracks) = db.get_playlist_tracks(playlist_id) {
              if *position < tracks.len() {
                tracks.remove(*position);
                for (new_pos, t) in tracks.iter_mut().enumerate() {
                  t.position = new_pos as i64;
                }
                let _ = db.replace_playlist_tracks(playlist_id, &tracks);
                app.set_status_message(format!("Removed '{}' from playlist", title), 4);
              }
            }
          }
          return;
        }
      }
      handle_delete_playlist(app);
    }
    Key::PageDown => {
      let count = total_display_count(app);
      if count > 0 {
        let current = app.view.selected_playlist_index.unwrap_or(0);
        app.view.selected_playlist_index = Some((current + 10).min(count - 1));
      }
    }
    Key::PageUp => {
      let count = total_display_count(app);
      if count > 0 {
        let current = app.view.selected_playlist_index.unwrap_or(0);
        app.view.selected_playlist_index = Some(current.saturating_sub(10));
      }
    }
    Key::Home => {
      app.view.selected_playlist_index = Some(0);
    }
    Key::End | Key::Char('G') => {
      let count = total_display_count(app);
      if count > 0 {
        app.view.selected_playlist_index = Some(count - 1);
      }
    }
    _ => {}
  }
}

/// The down key's cursor move, named for the mouse wheel.
pub(super) fn select_next(app: &mut App) {
  let count = total_display_count(app);
  if count > 0 {
    let current = app.view.selected_playlist_index.unwrap_or(0);
    app.view.selected_playlist_index = Some((current + 1) % count);
  }
}

pub(super) fn select_previous(app: &mut App) {
  let count = total_display_count(app);
  if count > 0 {
    let current = app.view.selected_playlist_index.unwrap_or(0);
    app.view.selected_playlist_index = Some(if current == 0 { count - 1 } else { current - 1 });
  }
}

/// The Enter consequence, routed by the active playlist panel tab.
pub(super) fn activate_selected(app: &mut App) {
  app.view.last_sidebar_block = SidebarBlock::Playlists;
  let Some(idx) = app.view.selected_playlist_index else {
    return;
  };

  match app.active_source {
    Source::Local => {
      open_local_folder(app);
      return;
    }
    Source::Subsonic => {
      open_subsonic_folder(app);
      return;
    }
    Source::Radio => {
      play_radio_station(app);
      return;
    }
    Source::Qobuz => {
      open_qobuz_folder(app);
      return;
    }
    Source::AppleMusic => {
      let uri = app
        .view
        .selected_playlist_index
        .and_then(|i| app.apple_music_playlists().get(i))
        .map(|p| p.uri.clone());
      open_source_playlist(app, uri);
      return;
    }
    _ => {}
  }

  // If in dedicated full-page Playlists tree view:
  if app.get_current_route().active_block == ActiveBlock::Playlists {
    let tree = get_playlist_tree_items(app);
    let Some(item) = tree.get(idx).cloned() else {
      return;
    };

    match item {
      PlaylistTreeItem::ActionItem { action_id, .. } => {
        match action_id {
          0 => {
            app.push_navigation_stack(RouteId::CreatePlaylist, ActiveBlock::CreatePlaylistForm);
          }
          _ => {
            app.view.url_import_visible = true;
            app.view.url_import_input.clear();
            app.view.url_import_status = None;
            app.view.url_import_in_progress = false;
          }
        }
      }
      PlaylistTreeItem::PlaylistHeader { id, is_expanded, .. } => {
        if is_expanded {
          // If already expanded, Enter opens it into the full Track Table (Song List)!
          open_source_playlist(app, Some(id.clone()));
        } else {
          // If collapsed, Enter expands it inline to reveal tracks!
          app.view.expanded_playlists.insert(id.clone());
        }
      }
      PlaylistTreeItem::TrackLeaf { playlist_id, position, title, uri, track_id: _, .. } => {
        // Play selected track from playlist and queue all upcoming tracks
        let mut uris = Vec::new();
        if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
          if let Ok(tracks) = db.get_playlist_tracks(&playlist_id) {
            app.track_table.tracks = tracks.iter().map(|t| {
              let mut info = crate::sources::lossless_relay::archiver::playlist_track_to_track_info(t);
              info.uri = Some(t.source_track_id.clone());
              info.id = Some(t.source_track_id.clone());
              info
            }).collect();
            app.track_table.context = Some(TrackTableContext::YouTubePlaylist);
            uris = tracks.iter().map(|t| t.source_track_id.clone()).collect();
          }
        }
        if uris.is_empty() {
          if let Ok(path) = crate::infra::youtube::playlists::default_playlists_path() {
            if let Ok(file) = crate::infra::youtube::playlists::load(&path) {
              if let Some(playlist) = crate::infra::youtube::playlists::find_playlist(&file, &playlist_id) {
                app.track_table.tracks = playlist.tracks.iter().map(|t| {
                  crate::infra::youtube::playlists::stored_to_track_info(t)
                }).collect();
                app.track_table.context = Some(TrackTableContext::YouTubePlaylist);
                uris = playlist.tracks.iter().map(|t| format!("youtube:video:{}", t.video_id)).collect();
              }
            }
          }
        }
        if !uris.is_empty() {
          app.apply(Action::PlayUris {
            uris,
            offset: Some(position),
          });
          app.set_status_message(format!("Playing '{}' from playlist", title), 4);
        } else if let Some(u) = uri {
          app.apply(Action::PlayUris { uris: vec![u], offset: None });
          app.set_status_message(format!("Playing '{}'", title), 4);
        }
      }
    }
    return;
  }

  // Otherwise, in sidebar quick playlist panel:
  match app.view.playlist_panel_tab {
    // Tab 0: EchoVena / Local Playlists
    0 => {
      match idx {
        0 => {
          app.push_navigation_stack(RouteId::CreatePlaylist, ActiveBlock::CreatePlaylistForm);
        }
        1 => {
          app.view.url_import_visible = true;
          app.view.url_import_input.clear();
          app.view.url_import_status = None;
          app.view.url_import_in_progress = false;
        }
        _ => {
          let local_db_playlists: Vec<_> = crate::sources::lossless_relay::db::Database::open_default()
            .ok()
            .and_then(|db| db.list_imported_playlists().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.platform == "local" || p.platform == "echovena")
            .collect();
          let db_len = local_db_playlists.len();
          let relative_idx = idx.saturating_sub(2);
          if relative_idx < db_len {
            let uri = local_db_playlists[relative_idx].id.clone();
            open_source_playlist(app, Some(uri));
          } else {
            let local_idx = relative_idx.saturating_sub(db_len);
            let uri = app.local_playlists().get(local_idx).map(|p| p.uri.clone());
            open_source_playlist(app, uri);
          }
        }
      }
    }
    // Tab 1: Spotify Playlists
    1 => {
      if idx == 0 {
        app.view.url_import_visible = true;
        app.view.url_import_input.clear();
        app.view.url_import_status = None;
        app.view.url_import_in_progress = false;
      } else {
        let spotify_db_playlists: Vec<_> = crate::sources::lossless_relay::db::Database::open_default()
          .ok()
          .and_then(|db| db.list_imported_playlists().ok())
          .unwrap_or_default()
          .into_iter()
          .filter(|p| p.platform.starts_with("spotify"))
          .collect();
        let db_len = spotify_db_playlists.len();
        let relative_idx = idx.saturating_sub(1);
        if relative_idx < db_len {
          let uri = spotify_db_playlists[relative_idx].id.clone();
          open_source_playlist(app, Some(uri));
        } else {
          let spotify_idx = relative_idx.saturating_sub(db_len);
          let id_opt = app.all_playlists().get(spotify_idx).and_then(|p| p.id.clone());
          if let Some(id_str) = id_opt {
            app.view.active_playlist_index = Some(spotify_idx);
            if app.active_source == crate::core::source::Source::Spotify && app.spotify_connected {
              app.apply(Action::Open(OpenTarget::Playlist {
                id: id_str,
                from_search: false,
              }));
            } else {
              open_source_playlist(app, Some(id_str));
            }
          }
        }
      }
    }
    // Tab 2: YouTube Playlists
    _ => {
      if idx == 0 {
        app.view.url_import_visible = true;
        app.view.url_import_input.clear();
        app.view.url_import_status = None;
        app.view.url_import_in_progress = false;
      } else {
        let yt_local_len = app.youtube_playlists().len();
        let relative_idx = idx.saturating_sub(1);
        if relative_idx < yt_local_len {
          let uri = app.youtube_playlists().get(relative_idx).map(|p| p.uri.clone());
          open_source_playlist(app, uri);
        } else {
          let yt_db_playlists: Vec<_> = crate::sources::lossless_relay::db::Database::open_default()
            .ok()
            .and_then(|db| db.list_imported_playlists().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.platform == "youtube" || p.platform == "ytmusic")
            .collect();
          let db_idx = relative_idx.saturating_sub(yt_local_len);
          if let Some(pl) = yt_db_playlists.get(db_idx) {
            open_source_playlist(app, Some(pl.id.clone()));
          }
        }
      }
    }
  }
}

#[allow(dead_code)]
fn open_spotify_row(app: &mut App) {
  if let Some(selected_idx) = app.view.selected_playlist_index {
    if selected_idx == 0 {
      // "+ Add Playlist" is the leading row (row 0).
      app.push_navigation_stack(RouteId::CreatePlaylist, ActiveBlock::CreatePlaylistForm);
    } else if let Some(item) = app.get_playlist_display_item_at(selected_idx - 1) {
      match item {
        PlaylistFolderItem::Folder(folder) => {
          // Navigate into/out of folder
          let target_id = folder.target_id;
          app.apply(Action::Open(OpenTarget::PlaylistFolder(target_id)));
          // Land on the first item below the leading "+ Add Playlist" row.
          // Counted after the apply: the count is folder-scoped.
          let has_items = app.get_playlist_display_count() > 0;
          app.view.selected_playlist_index = Some(if has_items { 1 } else { 0 });
        }
        PlaylistFolderItem::Playlist { index, .. } => {
          // Open the playlist tracks: navigates immediately with the
          // cleared table as the loading state (see open_playlist_tracks).
          let index = *index;
          let id_str = app.all_playlists().get(index).and_then(|p| p.id.clone());
          if let Some(id_str) = id_str {
            app.view.active_playlist_index = Some(index);
            app.apply(Action::Open(OpenTarget::Playlist {
              id: id_str,
              from_search: false,
            }));
          }
        }
        PlaylistFolderItem::CommunityPin => {
          app.view.active_playlist_index = None;
          app.apply(Action::Open(OpenTarget::Playlist {
            id: crate::core::app::COMMUNITY_PLAYLIST_ID.to_string(),
            from_search: false,
          }));
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::plugin_api::TrackInfo;
  use crate::core::state::RadioStationConfig;
  use crate::core::test_helpers::playlist_info;
  use crate::core::user_config::{UserConfig, UserConfigPaths};
  use crate::infra::network::IoEvent;
  use std::sync::mpsc::channel;
  use std::time::SystemTime;

  fn radio_station_row(name: &str, url: &str) -> TrackInfo {
    TrackInfo {
      generated_name: None,
      uri: Some(format!("radio:{url}")),
      name: name.to_string(),
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
    }
  }

  #[test]
  fn enter_playlist_dispatches_only_visible_page_load() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    // Exercise a real playlist (row 1, below the leading "+ Add Playlist" row),
    // not the pinned community entry.
    app.user_config.behavior.pin_community_playlist = false;
    *app.all_playlists_mut() = vec![playlist_info(
      "37i9dQZF1DXcBWIGoYBM5M",
      "Test Playlist",
      "echovena-test-user",
      false,
    )];
    *app.playlist_folder_items_mut() = vec![PlaylistFolderItem::Playlist {
      index: 0,
      current_id: 0,
    }];
    app.view.selected_playlist_index = Some(1);

    handler(Key::Enter, &mut app);

    assert_eq!(app.view.last_sidebar_block, ActiveBlock::MyPlaylists.into());

    match rx.recv().unwrap() {
      IoEvent::GetPlaylistItems(id, 0) => assert_eq!(id, "37i9dQZF1DXcBWIGoYBM5M"),
      _ => panic!("expected playlist page fetch"),
    }

    assert!(rx.try_recv().is_err());
  }

  #[test]
  fn enter_playlist_navigates_immediately_and_dedups_inflight_open() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    // Exercise a real playlist (row 1, below the leading "+ Add Playlist" row),
    // not the pinned community entry.
    app.user_config.behavior.pin_community_playlist = false;
    *app.all_playlists_mut() = vec![playlist_info(
      "37i9dQZF1DXcBWIGoYBM5M",
      "Test Playlist",
      "echovena-test-user",
      false,
    )];
    *app.playlist_folder_items_mut() = vec![PlaylistFolderItem::Playlist {
      index: 0,
      current_id: 0,
    }];
    app.view.selected_playlist_index = Some(1);

    handler(Key::Enter, &mut app);

    // The screen opens on the press itself, not on response arrival.
    assert_eq!(app.get_current_route().id, RouteId::TrackTable);
    match rx.recv().unwrap() {
      IoEvent::GetPlaylistItems(id, 0) => assert_eq!(id, "37i9dQZF1DXcBWIGoYBM5M"),
      _ => panic!("expected playlist page fetch"),
    }

    // Pressing Enter again while the same open is in flight (after navigating
    // back to the sidebar) re-opens the screen but dispatches no duplicate
    // fetch.
    app.pop_navigation_stack();
    handler(Key::Enter, &mut app);
    assert_eq!(app.get_current_route().id, RouteId::TrackTable);
    assert!(rx.try_recv().is_err());
  }

  #[test]
  fn enter_on_row_zero_opens_add_playlist_form() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    // Row 0 is always the "+ Add Playlist" entry.
    app.view.selected_playlist_index = Some(0);

    handler(Key::Enter, &mut app);

    assert_eq!(app.get_current_route().id, RouteId::CreatePlaylist);
    assert!(rx.try_recv().is_err());
  }

  #[test]
  fn enter_on_community_pin_opens_community_playlist() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    // No real playlists: the pin is the only display item, at sidebar row 1
    // (row 0 is "+ Add Playlist").
    assert!(app.community_pin_visible());
    app.view.selected_playlist_index = Some(1);

    handler(Key::Enter, &mut app);

    assert_eq!(app.get_current_route().id, RouteId::TrackTable);
    // The pin is not a real sidebar selection.
    assert_eq!(app.view.active_playlist_index, None);
    match rx.recv().unwrap() {
      IoEvent::GetPlaylistItems(id, 0) => {
        assert_eq!(id, crate::core::app::COMMUNITY_PLAYLIST_ID)
      }
      _ => panic!("expected community playlist fetch"),
    }
  }

  #[test]
  fn shift_d_on_community_pin_is_a_no_op() {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    assert!(app.community_pin_visible());
    // Sidebar row 1 is the pin (row 0 is "+ Add Playlist").
    app.view.selected_playlist_index = Some(1);
    let route_before = app.get_current_route().id.clone();

    handler(Key::Char('D'), &mut app);

    assert_eq!(app.get_current_route().id, route_before);
    assert!(app.view.dialog.is_none());
    assert!(rx.try_recv().is_err());
  }

  #[test]
  fn enter_on_local_folder_dispatches_get_local_tracks() {
    use crate::core::plugin_api::PlaylistInfo;
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    app.active_source = Source::Local;
    *app.local_playlists_mut() = vec![PlaylistInfo {
      generated_name: None,
      uri: "file:///music/Jazz".to_string(),
      name: "Jazz".to_string(),
      owner: "local".to_string(),
      track_count: 0,
      id: None,
      owner_id: None,
      collaborative: false,
      public: None,
      image_url: None,
    }];
    app.view.selected_playlist_index = Some(0);

    handler(Key::Enter, &mut app);

    match rx.recv().unwrap() {
      IoEvent::GetLocalTracks(uri) => assert_eq!(uri, "file:///music/Jazz"),
      _ => panic!("expected local tracks fetch"),
    }
  }

  #[test]
  fn shift_d_on_radio_station_removes_favorite_and_updates_sidebar() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, _rx) = channel();
    let mut config = UserConfig::new();
    config.path_to_config = Some(UserConfigPaths {
      config_file_path: dir.path().join("config.yml"),
    });
    let mut app = App::new(tx, config, Some(SystemTime::now()));
    app.state_path = Some(dir.path().join("state.yml"));
    app.runtime_state.radio_stations = vec![
      RadioStationConfig {
        name: "Groove Salad".to_string(),
        url: "https://ice1.somafm.com/groovesalad-128-mp3".to_string(),
      },
      RadioStationConfig {
        name: "Secret Agent".to_string(),
        url: "https://ice1.somafm.com/secretagent-128-mp3".to_string(),
      },
    ];

    app.active_source = Source::Radio;
    *app.radio_stations_mut() = vec![
      TrackInfo {
        generated_name: None,
        uri: Some("radio:https://ice1.somafm.com/groovesalad-128-mp3".to_string()),
        name: "Groove Salad".to_string(),
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
      },
      TrackInfo {
        generated_name: None,
        uri: Some("radio:https://ice1.somafm.com/secretagent-128-mp3".to_string()),
        name: "Secret Agent".to_string(),
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
      },
    ];
    app.view.selected_playlist_index = Some(0);

    handler(Key::Char('D'), &mut app);

    assert_eq!(app.runtime_state.radio_stations.len(), 1);
    assert_eq!(
      app.runtime_state.radio_stations[0].url,
      "https://ice1.somafm.com/secretagent-128-mp3"
    );
    assert_eq!(app.radio_stations().len(), 1);
    assert_eq!(app.radio_stations()[0].name, "Secret Agent");
    assert_eq!(app.view.selected_playlist_index, Some(0));
    assert_eq!(
      app.status_message(),
      Some("Removed saved radio station: Groove Salad")
    );
  }

  #[test]
  fn shift_d_on_config_only_radio_station_reports_config_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, _rx) = channel();
    let mut config = UserConfig::new();
    config.path_to_config = Some(UserConfigPaths {
      config_file_path: dir.path().join("config.yml"),
    });
    config.behavior.radio_stations = vec![RadioStationConfig {
      name: "Configured Groove".to_string(),
      url: "https://ice1.somafm.com/groovesalad-128-mp3".to_string(),
    }];
    let mut app = App::new(tx, config, Some(SystemTime::now()));
    app.state_path = Some(dir.path().join("state.yml"));
    app.active_source = Source::Radio;
    *app.radio_stations_mut() = vec![radio_station_row(
      "Configured Groove",
      "https://ice1.somafm.com/groovesalad-128-mp3",
    )];
    app.view.selected_playlist_index = Some(0);

    handler(Key::Char('D'), &mut app);

    assert!(app.runtime_state.radio_stations.is_empty());
    assert_eq!(app.radio_stations().len(), 1);
    assert_eq!(app.view.selected_playlist_index, Some(0));
    assert_eq!(
      app.status_message(),
      Some("Radio station is configured in config.yml: Configured Groove")
    );
  }

  #[test]
  fn shift_d_on_config_and_state_radio_station_removes_saved_copy_only() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, _rx) = channel();
    let mut config = UserConfig::new();
    config.path_to_config = Some(UserConfigPaths {
      config_file_path: dir.path().join("config.yml"),
    });
    config.behavior.radio_stations = vec![RadioStationConfig {
      name: "Configured Groove".to_string(),
      url: "https://ice1.somafm.com/groovesalad-128-mp3".to_string(),
    }];
    let mut app = App::new(tx, config, Some(SystemTime::now()));
    app.state_path = Some(dir.path().join("state.yml"));
    app.runtime_state.radio_stations = vec![RadioStationConfig {
      name: "Runtime Duplicate".to_string(),
      url: "https://ice1.somafm.com/groovesalad-128-mp3".to_string(),
    }];
    app.active_source = Source::Radio;
    *app.radio_stations_mut() = vec![radio_station_row(
      "Configured Groove",
      "https://ice1.somafm.com/groovesalad-128-mp3",
    )];
    app.view.selected_playlist_index = Some(0);

    handler(Key::Char('D'), &mut app);

    assert!(app.runtime_state.radio_stations.is_empty());
    assert_eq!(app.radio_stations().len(), 1);
    assert_eq!(app.radio_stations()[0].name, "Configured Groove");
    assert_eq!(app.view.selected_playlist_index, Some(0));
    assert_eq!(
      app.status_message(),
      Some("Removed saved radio station: Runtime Duplicate")
    );
  }

  #[test]
  fn m_on_a_qobuz_playlist_opens_the_mirror_picker() {
    use crate::core::plugin_api::PlaylistInfo;
    let (tx, _rx) = channel();
    let mut app =
      App::new(tx, UserConfig::new(), Some(SystemTime::now())).under_source(Source::Qobuz);
    app.qobuz_playlists_mut().push(PlaylistInfo {
      generated_name: None,
      uri: "qobuz:playlist:9".to_string(),
      name: "Mine".to_string(),
      owner: "qobuz".to_string(),
      track_count: 3,
      id: Some("9".to_string()),
      owner_id: None,
      collaborative: false,
      public: None,
      image_url: None,
    });
    app.view.selected_playlist_index = Some(0);

    handler(Key::Char('m'), &mut app);

    assert_eq!(
      app.get_current_route().active_block,
      ActiveBlock::Dialog(DialogContext::PlaylistSyncPicker)
    );
  }

  #[test]
  fn opening_a_playlist_row_remembers_the_playlist_panel() {
    let mut app = App::default();
    app.view.last_sidebar_block = SidebarBlock::Library;
    activate_selected(&mut app);
    assert_eq!(app.view.last_sidebar_block, SidebarBlock::Playlists);
  }
}
