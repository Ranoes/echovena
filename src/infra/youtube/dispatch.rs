//! YouTube search/playback routing.
//!
//! [`route_youtube_event`] is called from the runtime IoEvent pump *after* the
//! local/Subsonic/radio dispatches and *before* `handle_network_event`. When an
//! event targets the YouTube source (a search request, or a `youtube:` playback
//! URI) it is handled here and consumed; otherwise it falls through to the
//! normal Spotify dispatch.
//!
//! ## Decoupling & device ownership
//!
//! YouTube playback owns a single piece of state, [`App::youtube_playback`],
//! and never writes Spotify/librespot fields — the playbar reads progress/pause
//! live from the player. Only one backend holds the audio device at a time:
//! starting YouTube pauses or parks librespot **and** tears down any local/Subsonic/
//! radio session; the reciprocal teardowns live in those sources' start paths.
//!
//! ## Streaming
//!
//! Each video's audio is downloaded by `yt-dlp` to a tempfile (off the `App`
//! lock — this is the slowest download window of any source), then played from
//! disk through the shared [`LocalPlayer`]. Like Subsonic, a download failure
//! tears the session down rather than skipping past, so a broken extractor or
//! dead network can't cascade error toasts across the whole queue.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tempfile::NamedTempFile;
use tokio::sync::Mutex;

use super::{
  is_youtube_uri, uri_for_video_id, video_id_from_uri, YouTubePlaybackState, YouTubeSource,
};
use crate::core::app::App;
use crate::core::plugin_api::TrackInfo;
use crate::core::source::{Searcher, Source};
use crate::infra::audio::LocalPlayer;
use crate::infra::network::IoEvent;
use crate::infra::queue::{
  advance_index, apply_resolved, replay_file, snapshot_tracks,
};

/// Skip direction within the YouTube queue.
#[derive(Clone, Copy)]
enum Direction {
  Next,
  Prev,
}

/// Intercept events that target the YouTube source.
///
/// Returns `true` if the event was handled (and must **not** be forwarded to
/// the Spotify network), `false` to let the normal dispatch run.
pub async fn route_youtube_event(app: &Arc<Mutex<App>>, event: &IoEvent) -> bool {
  match event {
    IoEvent::GetYouTubeSearchResults(query) => {
      run_youtube_search(app, query).await;
      true
    }
    IoEvent::GetYouTubePlaylists => {
      load_playlists(app).await;
      true
    }
    IoEvent::GetYouTubeTracks(uri) => {
      load_playlist_tracks(app, uri).await;
      true
    }
    IoEvent::CreateYouTubePlaylist(name) => {
      create_playlist(app, name).await;
      true
    }
    IoEvent::DeleteYouTubePlaylist(uri) => {
      delete_playlist(app, uri).await;
      true
    }
    IoEvent::AddTrackToYouTubePlaylist(playlist_ref, video_ref) => {
      add_track_to_playlist(app, playlist_ref, video_ref).await;
      true
    }
    IoEvent::RemoveTrackFromYouTubePlaylist(playlist_ref, video_ref) => {
      remove_track_from_playlist(app, playlist_ref, video_ref).await;
      true
    }
    IoEvent::FetchAlbumDetail(album_id, title) => {
      fetch_album_drilldown(app, album_id, title).await;
      true
    }
    IoEvent::FetchArtistDetail(artist_id, name) => {
      fetch_artist_drilldown(app, artist_id, name).await;
      true
    }
    // Start a queue of videos: handle if YouTube URI or if Spotify is not active & connected
    IoEvent::StartPlayback(None, Some(uris), offset) => {
      let is_yt = uris.first().is_some_and(|u| is_youtube_uri(u)) || matches!(app.lock().await.track_table.context, Some(crate::core::app::TrackTableContext::YouTubePlaylist));
      let is_foreign = uris.first().is_some_and(|u| u.starts_with("radio:") || u.starts_with("local:"));
      let spotify_active = {
        let guard = app.lock().await;
        guard.active_source == Source::Spotify && guard.spotify_connected
      };
      let is_spotify = uris.first().is_some_and(|u| u.starts_with("spotify:"));
      if is_yt || (!is_foreign && (!is_spotify || !spotify_active)) {
        start_youtube_queue(app, uris, offset.unwrap_or(0)).await;
        true
      } else {
        teardown_youtube(app).await;
        false
      }
    }
    // Context with specific track(s) in queue
    IoEvent::StartPlayback(Some(_ctx), Some(uris), offset) => {
      let is_yt = uris.first().is_some_and(|u| is_youtube_uri(u)) || matches!(app.lock().await.track_table.context, Some(crate::core::app::TrackTableContext::YouTubePlaylist));
      let is_foreign = uris.first().is_some_and(|u| u.starts_with("radio:") || u.starts_with("local:"));
      let spotify_active = {
        let guard = app.lock().await;
        guard.active_source == Source::Spotify && guard.spotify_connected
      };
      let is_spotify = uris.first().is_some_and(|u| u.starts_with("spotify:"));
      if is_yt || (!is_foreign && (!is_spotify || !spotify_active)) {
        start_youtube_queue(app, uris, offset.unwrap_or(0)).await;
        true
      } else {
        teardown_youtube(app).await;
        false
      }
    }
    // Context URI or single video with no surrounding list
    IoEvent::StartPlayback(Some(uri), None, offset) => {
      let is_yt = is_youtube_uri(uri) || matches!(app.lock().await.track_table.context, Some(crate::core::app::TrackTableContext::YouTubePlaylist));
      let is_foreign = uri.starts_with("radio:") || uri.starts_with("local:");
      let spotify_active = {
        let guard = app.lock().await;
        guard.active_source == Source::Spotify && guard.spotify_connected
      };
      let is_spotify = uri.starts_with("spotify:");
      if is_yt || (!is_foreign && (!is_spotify || !spotify_active)) {
        let table_uris: Vec<String> = {
          let guard = app.lock().await;
          guard.track_table.tracks.iter().filter_map(|t| t.uri.clone().or_else(|| t.id.clone())).collect()
        };
        if !table_uris.is_empty() {
          start_youtube_queue(app, &table_uris, offset.unwrap_or(0)).await;
        } else if uri.starts_with("youtube:playlist:") || uri.starts_with("yt:playlist:") {
          load_playlist_tracks(app, uri).await;
          let loaded_uris: Vec<String> = {
            let guard = app.lock().await;
            guard.track_table.tracks.iter().filter_map(|t| t.uri.clone().or_else(|| t.id.clone())).collect()
          };
          if !loaded_uris.is_empty() {
            start_youtube_queue(app, &loaded_uris, offset.unwrap_or(0)).await;
          }
        } else {
          start_youtube_queue(app, std::slice::from_ref(uri), 0).await;
        }
        true
      } else {
        teardown_youtube(app).await;
        false
      }
    }
    // Bare "resume current" — ours only while YouTube owns the session.
    IoEvent::StartPlayback(None, None, None) => match player(app).await {
      Some(p) => {
        p.resume();
        true
      }
      None => false,
    },
    // Any other start is a local/Subsonic/radio/Spotify play: relinquish the
    // device, then let the normal dispatch run.
    IoEvent::StartPlayback(..) => {
      teardown_youtube(app).await;
      false
    }
    IoEvent::PausePlayback => match player(app).await {
      Some(p) => {
        p.pause();
        true
      }
      None => false,
    },
    IoEvent::Seek(position_ms) => match player(app).await {
      Some(p) => {
        let _ = p.seek(Duration::from_millis(*position_ms as u64));
        true
      }
      None => false,
    },
    IoEvent::ChangeVolume(volume) => match player(app).await {
      Some(p) => {
        p.set_volume(*volume);
        let mut app = app.lock().await;
        app.runtime_state.volume_percent = *volume;
        app.schedule_state_save(crate::core::state::PersistedRuntimeState::volume_percent(
          *volume,
        ));
        true
      }
      None => false,
    },
    IoEvent::NextTrack => skip(app, Direction::Next).await,
    IoEvent::JumpToQueueIndex(target) => {
      let target = *target;
      let valid = {
        let guard = app.lock().await;
        guard.youtube_playback.as_ref().is_some_and(|s| target < s.tracks.len())
      };
      if valid {
        play_index(app, target).await;
        true
      } else {
        false
      }
    },
    IoEvent::PreviousTrack | IoEvent::ForcePreviousTrack => skip(app, Direction::Prev).await,
    // Repeat-one auto-advance: replay the current video (dispatched by the runner
    // tick only while YouTube owns the session).
    IoEvent::ReplayCurrentTrack => replay_current(app).await,
    // Explicit repeat-mode set (e.g. a plugin's `set_repeat`): store it as the
    // decoded repeat mode while a queueable decoded source owns playback (the
    // method's ownership gate excludes a context suspended under the native
    // queue), so it never reaches the Spotify context.
    IoEvent::Repeat(state) => app.lock().await.set_decoded_repeat_from_state(*state),
    _ => false,
  }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Build a [`YouTubeSource`] from the user config (`behavior.ytdlp_path`
/// override, else `yt-dlp` on `$PATH`). Unlike Subsonic there is nothing that
/// can be "unconfigured" — a missing binary surfaces as an actionable error
/// from the first search instead.
pub(crate) async fn build_source(app: &Arc<Mutex<App>>) -> YouTubeSource {
  let ytdlp_path = app.lock().await.user_config.behavior.ytdlp_path.clone();
  YouTubeSource::new(ytdlp_path)
}

/// Run a YouTube search and populate `app.search_results`. Like the Subsonic
/// M2 search, only the songs block is populated — each row is a video; Enter
/// dispatches the `youtube:` URIs back through this module.
async fn run_youtube_search(app: &Arc<Mutex<App>>, query: &str) {
  let source = build_source(app).await;
  match source.search(query).await {
    Ok(results) => app
      .lock()
      .await
      .show_source_search_tracks(query, results.tracks),
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_search_failed(&e.to_string()),
      )
      .await
    }
  }
}

// ---------------------------------------------------------------------------
// Local playlists (youtube_playlists.yml)
// ---------------------------------------------------------------------------

/// Run a load→mutate→save transaction against the playlists file on the
/// blocking pool (never on the async executor, and never under the `App`
/// lock). Returns whatever the mutation returns.
async fn with_playlists_file<T, F>(mutate: F) -> Result<T>
where
  T: Send + 'static,
  F: FnOnce(&mut super::playlists::PlaylistsFile) -> Result<T> + Send + 'static,
{
  tokio::task::spawn_blocking(move || {
    let path = super::playlists::default_playlists_path()?;
    let mut file = super::playlists::load(&path)?;
    let out = mutate(&mut file)?;
    super::playlists::save(&path, &file)?;
    Ok(out)
  })
  .await
  .context("YouTube playlists task failed")?
}

/// Read-only load of the playlists file on the blocking pool.
async fn read_playlists_file() -> Result<super::playlists::PlaylistsFile> {
  tokio::task::spawn_blocking(|| {
    let path = super::playlists::default_playlists_path()?;
    super::playlists::load(&path)
  })
  .await
  .context("YouTube playlists task failed")?
}

/// Refresh the sidebar list in `App` from an already-loaded file.
async fn publish_playlists(app: &Arc<Mutex<App>>, file: &super::playlists::PlaylistsFile) {
  let infos: Vec<_> = file
    .playlists
    .iter()
    .map(super::playlists::playlist_to_info)
    .collect();
  *app.lock().await.youtube_playlists_mut() = infos;
}

/// Load `youtube_playlists.yml` into the sidebar.
async fn load_playlists(app: &Arc<Mutex<App>>) {
  match read_playlists_file().await {
    Ok(file) => publish_playlists(app, &file).await,
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_playlists_failed(&e.to_string()),
      )
      .await
    }
  }
}

/// Open a playlist's saved tracks in the shared track table.
async fn load_playlist_tracks(app: &Arc<Mutex<App>>, uri: &str) {
  // 1. Check local playlists file (user-created YouTube playlists in playlists.json)
  if let Ok(file) = read_playlists_file().await {
    if let Some(playlist) = super::playlists::find_playlist(&file, uri) {
      let rows: Vec<TrackInfo> = playlist
        .tracks
        .iter()
        .map(super::playlists::stored_to_track_info)
        .collect();
      let mut guard = app.lock().await;
      guard.set_source_track_table(
        uri,
        rows,
        crate::core::app::TrackTableContext::YouTubePlaylist,
      );
      guard.youtube_open_playlist = Some(uri.to_string());
      if guard.get_current_route().id != crate::core::app::RouteId::TrackTable {
        guard.push_navigation_stack(
          crate::core::app::RouteId::TrackTable,
          crate::core::app::ActiveBlock::TrackTable,
        );
      }
      return;
    }
  }

  // 2. Check local SQLite imported playlists / cached playlist tracks
  if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
    let clean = uri
      .strip_prefix("youtube:playlist:")
      .or_else(|| uri.strip_prefix("yt:playlist:"))
      .unwrap_or(uri);
    if let Ok(db_tracks) = db.get_playlist_tracks(uri).or_else(|_| db.get_playlist_tracks(clean)) {
      if !db_tracks.is_empty() {
        let rows: Vec<TrackInfo> = db_tracks
          .iter()
          .map(crate::sources::lossless_relay::archiver::playlist_track_to_track_info)
          .collect();
        let mut guard = app.lock().await;
        guard.set_source_track_table(
          uri,
          rows,
          crate::core::app::TrackTableContext::YouTubePlaylist,
        );
        guard.youtube_open_playlist = Some(uri.to_string());
        if guard.get_current_route().id != crate::core::app::RouteId::TrackTable {
          guard.push_navigation_stack(
            crate::core::app::RouteId::TrackTable,
            crate::core::app::ActiveBlock::TrackTable,
          );
        }
        return;
      }
    }
  }

  // 3. Online fetch via YouTube InnerTube (for Discover playlists, Radio, Mixes, Charts)
  let clean_pid = uri
    .strip_prefix("youtube:playlist:")
    .or_else(|| uri.strip_prefix("yt:playlist:"))
    .or_else(|| uri.strip_prefix("youtube:"))
    .or_else(|| uri.strip_prefix("yt:"))
    .unwrap_or(uri);

  let importer = crate::sources::lossless_relay::youtube_importer::YouTubeImporter::new();

  let fetched_tracks: Option<Vec<TrackInfo>> = if clean_pid.starts_with("RDAMVM") || clean_pid.starts_with("RD") {
    let seed_id = clean_pid
      .strip_prefix("RDAMVM")
      .or_else(|| clean_pid.strip_prefix("RD"))
      .unwrap_or(clean_pid);
    importer.fetch_youtube_radio_tracks(seed_id).await.ok()
  } else {
    importer.fetch_innertube_playlist(clean_pid).await.ok().map(|(_pl, pl_tracks)| {
      pl_tracks
        .iter()
        .map(crate::sources::lossless_relay::archiver::playlist_track_to_track_info)
        .collect()
    })
  };

  if let Some(rows) = fetched_tracks {
    if !rows.is_empty() {
      let mut guard = app.lock().await;
      guard.set_source_track_table(
        uri,
        rows,
        crate::core::app::TrackTableContext::YouTubePlaylist,
      );
      guard.youtube_open_playlist = Some(uri.to_string());
      if guard.get_current_route().id != crate::core::app::RouteId::TrackTable {
        guard.push_navigation_stack(
          crate::core::app::RouteId::TrackTable,
          crate::core::app::ActiveBlock::TrackTable,
        );
      }
      return;
    }
  }

  set_error(
    app,
    crate::infra::app_strings(app)
      .await
      .status
      .infra
      .youtube_playlist_missing
      .to_string(),
  )
  .await;
}

/// Create a playlist and refresh the sidebar.
async fn create_playlist(app: &Arc<Mutex<App>>, name: &str) {
  let name_owned = name.to_string();
  let result = with_playlists_file(move |file| {
    super::playlists::create_playlist(file, &name_owned)?;
    Ok(file.clone())
  })
  .await;
  match result {
    Ok(file) => {
      publish_playlists(app, &file).await;
      let mut guard = app.lock().await;
      {
        let strings = guard.strings();
        guard.set_status_message(strings.status.infra.youtube_created(name.trim()), 4)
      };
    }
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_create_failed(&e.to_string()),
      )
      .await
    }
  }
}

/// Delete a playlist and refresh the sidebar.
async fn delete_playlist(app: &Arc<Mutex<App>>, uri: &str) {
  let uri_owned = uri.to_string();
  let result = with_playlists_file(move |file| {
    let deleted = super::playlists::delete_playlist(file, &uri_owned)?;
    Ok((deleted.name, file.clone()))
  })
  .await;
  match result {
    Ok((name, file)) => {
      publish_playlists(app, &file).await;
      let mut guard = app.lock().await;
      // The deleted playlist may be open in the track table; drop the marker so
      // the remove-track flow can't edit a ghost.
      if guard.youtube_open_playlist.as_deref() == Some(uri) {
        guard.youtube_open_playlist = None;
      }
      {
        let strings = guard.strings();
        guard.set_status_message(strings.status.infra.youtube_deleted(&name), 4)
      };
    }
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_delete_failed(&e.to_string()),
      )
      .await
    }
  }
}

/// Find a video's browse row by bare id or `youtube:` URI, in the search
/// results, the track table, the active youtube playback session, or queue.
fn find_video_row(app: &App, video_ref: &str) -> Option<TrackInfo> {
  let uri = super::uri_for_video_id(video_ref);
  let clean_id = super::extract_youtube_video_id(video_ref);
  let matches = |t: &&TrackInfo| {
    t.id.as_deref() == Some(video_ref)
      || t.uri.as_deref() == Some(video_ref)
      || t.uri.as_deref() == Some(uri.as_str())
      || (clean_id.is_some() && (
        t.id.as_deref() == clean_id.as_deref()
          || t.uri.as_deref().and_then(super::extract_youtube_video_id).as_deref() == clean_id.as_deref()
      ))
  };
  app
    .search_results()
    .tracks
    .as_ref()
    .and_then(|p| p.items.iter().find(matches))
    .or_else(|| app.track_table.tracks.iter().find(matches))
    .or_else(|| {
      app.youtube_playback.as_ref().and_then(|yt| {
        yt.tracks.iter().find(matches)
      })
    })
    .cloned()
    .or_else(|| {
      if let Some(pending) = app.pending_playlist_track_add.as_ref() {
        if pending.track_id == video_ref {
          let vid = clean_id.clone().unwrap_or_else(|| video_ref.to_string());
          return Some(TrackInfo {
            id: Some(vid.clone()),
            uri: Some(format!("youtube:video:{}", vid)),
            name: pending.track_name.clone(),
            artists: vec!["YouTube".to_string()],
            album: String::new(),
            duration_ms: 0,
            album_id: None,
            artist_refs: vec![],
            is_playable: true,
            is_local: false,
            track_number: 1,
            explicit: false,
            image_url: None,
            release_date: None,
            generated_name: None,
          });
        }
      }
      if let Some(vid) = clean_id {
        Some(TrackInfo {
          id: Some(vid.clone()),
          uri: Some(format!("youtube:video:{}", vid)),
          name: video_ref.to_string(),
          artists: vec!["YouTube".to_string()],
          album: String::new(),
          duration_ms: 0,
          album_id: None,
          artist_refs: vec![],
          is_playable: true,
          is_local: false,
          track_number: 1,
          explicit: false,
          image_url: None,
          release_date: None,
          generated_name: None,
        })
      } else {
        None
      }
    })
}

/// Add a video to a playlist, resolving its metadata from the browse views.
async fn add_track_to_playlist(app: &Arc<Mutex<App>>, playlist_ref: &str, video_ref: &str) {
  let row = {
    let guard = app.lock().await;
    find_video_row(&guard, video_ref)
  };
  let Some(row) = row else {
    set_error(
      app,
      crate::infra::app_strings(app)
        .await
        .status
        .infra
        .youtube_metadata_missing
        .to_string(),
    )
    .await;
    return;
  };
  let stored = match super::playlists::track_info_to_stored(&row) {
    Ok(s) => s,
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_add_failed(&e.to_string()),
      )
      .await;
      return;
    }
  };

  let playlist_owned = playlist_ref.to_string();
  let result = with_playlists_file(move |file| {
    let added = super::playlists::add_track(file, &playlist_owned, stored)?;
    let name = super::playlists::find_playlist(file, &playlist_owned)
      .map(|p| p.name.clone())
      .unwrap_or_default();
    Ok((added, name, file.clone()))
  })
  .await;

  match result {
    Ok((added, playlist_name, file)) => {
      publish_playlists(app, &file).await;
      let msg = if added {
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_added(&row.name, &playlist_name)
      } else {
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_already_added(&row.name, &playlist_name)
      };
      app.lock().await.set_status_message(msg, 4);
    }
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_add_failed(&e.to_string()),
      )
      .await
    }
  }
}

/// Remove a video from a playlist; refreshes the open track table when it
/// shows that playlist.
async fn remove_track_from_playlist(app: &Arc<Mutex<App>>, playlist_ref: &str, video_ref: &str) {
  // The remove dialog passes the bare video id; normalize from a URI too.
  let video_id = super::video_id_from_uri(video_ref)
    .map(str::to_owned)
    .unwrap_or_else(|_| video_ref.to_string());

  let playlist_owned = playlist_ref.to_string();
  let result = with_playlists_file(move |file| {
    let removed = super::playlists::remove_track(file, &playlist_owned, &video_id)?;
    let remaining: Vec<_> = super::playlists::find_playlist(file, &playlist_owned)
      .map(|p| {
        p.tracks
          .iter()
          .map(super::playlists::stored_to_track_info)
          .collect()
      })
      .unwrap_or_default();
    Ok((removed.title, remaining, file.clone()))
  })
  .await;

  match result {
    Ok((title, remaining, file)) => {
      publish_playlists(app, &file).await;
      let mut guard = app.lock().await;
      // Keep the open track table in sync with the file.
      if guard.youtube_open_playlist.as_deref() == Some(playlist_ref) {
        guard.track_table.tracks = remaining;
        guard.clamp_track_table_cursor();
      }
      {
        let strings = guard.strings();
        guard.set_status_message(strings.status.infra.youtube_removed(&title), 4)
      };
    }
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_remove_failed(&e.to_string()),
      )
      .await
    }
  }
}

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

/// The live YouTube player, if a YouTube session is active.
async fn player(app: &Arc<Mutex<App>>) -> Option<Arc<LocalPlayer>> {
  app
    .lock()
    .await
    .youtube_playback
    .as_ref()
    .map(|s| Arc::clone(&s.player))
}

/// Release the other backends so only YouTube holds the output device.
async fn release_other_backends(app: &Arc<Mutex<App>>) {
  // Take the sink from native Spotify so no rebuild resumes it under this
  // source.
  #[cfg(feature = "streaming")]
  app.lock().await.release_native_for_decoded();
  let players = app
    .lock()
    .await
    .take_decoded_sessions_except(Source::YouTube);
  for player in players {
    player.stop_detached();
  }
}

/// Reuse the live YouTube player, or open a fresh output device for one. A
/// freshly opened player is **not** published to `App` here — the caller
/// publishes the session only on a successful first play.
async fn acquire_player(app: &Arc<Mutex<App>>) -> Option<Arc<LocalPlayer>> {
  if let Some(p) = player(app).await {
    return Some(p);
  }
  match tokio::task::spawn_blocking(LocalPlayer::new).await {
    Ok(Ok(p)) => Some(Arc::new(p)),
    Ok(Err(e)) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_audio_unavailable(&e.to_string()),
      )
      .await;
      None
    }
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .audio_init_failed(&e.to_string()),
      )
      .await;
      None
    }
  }
}

/// Safely allocate a temporary file, prioritizing the application audio cache dir over system %TEMP%.
fn create_audio_tempfile(suffix: &str) -> Result<NamedTempFile> {
  if let Some(dir) = crate::core::paths::audio_cache_dir() {
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(tmp) = tempfile::Builder::new().suffix(suffix).tempfile_in(&dir) {
      return Ok(tmp);
    }
    if let Ok(tmp) = tempfile::Builder::new().tempfile_in(&dir) {
      return Ok(tmp);
    }
  }
  if let Ok(tmp) = NamedTempFile::with_suffix(suffix) {
    return Ok(tmp);
  }
  if let Ok(tmp) = NamedTempFile::new() {
    return Ok(tmp);
  }
  if let Some(dir) = crate::core::paths::app_cache_dir() {
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(tmp) = tempfile::Builder::new().tempfile_in(&dir) {
      return Ok(tmp);
    }
  }
  tempfile::Builder::new()
    .tempfile_in(".")
    .context("creating temp file for YouTube audio")
}

/// Download a video's audio into a fresh tempfile (or load from disk cache),
/// with the metadata yt-dlp printed for it when it could be read. Must be awaited
/// **without** holding the `App` lock (this is the slowest download of any source).
async fn download_audio(
  source: &YouTubeSource,
  video_id: &str,
) -> Result<(NamedTempFile, std::path::PathBuf, Option<TrackInfo>)> {
  if let Some(cached) = super::cached_audio_path(video_id) {
    let ext = cached.extension().and_then(|e| e.to_str()).unwrap_or("m4a");
    let tmp = create_audio_tempfile(&format!(".{ext}"))?;
    let _ = std::fs::copy(&cached, tmp.path());
    let meta = if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
      db.find_playlist_track_by_source_id(video_id).ok().flatten()
        .map(|pt| crate::sources::lossless_relay::archiver::playlist_track_to_track_info(&pt))
    } else {
      None
    };
    return Ok((tmp, cached, meta));
  }
  // Check local offline archive in SQLite
  if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
    let id1 = format!("yt:video:{}", video_id);
    let id2 = format!("youtube:video:{}", video_id);
    let res = db.get_resolution(&id1).ok().flatten()
      .or_else(|| db.get_resolution(&id2).ok().flatten())
      .or_else(|| db.get_resolution(video_id).ok().flatten());
    if let Some(res) = res {
      if let Some(archive_path_str) = res.local_archive_path {
        let p = std::path::PathBuf::from(archive_path_str);
        if p.exists() {
          let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("flac");
          let tmp = create_audio_tempfile(&format!(".{ext}"))?;
          let _ = std::fs::copy(&p, tmp.path());
          let meta = db.find_playlist_track_by_source_id(video_id).ok().flatten()
            .map(|pt| crate::sources::lossless_relay::archiver::playlist_track_to_track_info(&pt));
          return Ok((tmp, p, meta));
        }
      }
    }
    if let Ok(downloaded) = db.list_downloaded_tracks() {
      if let Some(dt) = downloaded.into_iter().find(|d| {
        d.source_track_id == video_id
          || d.source_track_id == format!("youtube:{}", video_id)
          || d.source_track_id == format!("yt:video:{}", video_id)
          || d.file_path.contains(video_id)
      }) {
        let p = std::path::PathBuf::from(&dt.file_path);
        if p.exists() {
          let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("flac");
          let tmp = create_audio_tempfile(&format!(".{ext}"))?;
          let _ = std::fs::copy(&p, tmp.path());
          let info = TrackInfo {
            uri: Some(format!("youtube:video:{}", video_id)),
            name: dt.title,
            artists: vec![dt.artist],
            album: dt.album.unwrap_or_default(),
            duration_ms: dt.duration_ms,
            id: Some(dt.source_track_id),
            album_id: None,
            artist_refs: Vec::new(),
            is_playable: true,
            is_local: true,
            track_number: 1,
            explicit: false,
            image_url: dt.cover_art_url,
            release_date: None,
            generated_name: None,
          };
          return Ok((tmp, p, Some(info)));
        }
      }
    }
  }
  let tmp = create_audio_tempfile(".m4a")?;
  let tmp_path = tmp.path().to_path_buf();
  let resolved = source.download_audio(video_id, &tmp_path).await?;
  let play_path = if let Some(cached) = super::cached_audio_path(video_id) {
    cached
  } else {
    tmp_path.clone()
  };
  if play_path != tmp_path && play_path.exists() {
    let _ = std::fs::copy(&play_path, &tmp_path);
  }
  Ok((tmp, play_path, resolved))
}

/// Download the audio for `uri` into a tempfile, for the native queue engine's
/// off-pump fetch (playing is the queue's job — it re-checks that the queue
/// slot is still current before touching the shared player).
#[allow(dead_code)]
pub(crate) async fn download_for_queue(source: &YouTubeSource, uri: &str) -> Result<NamedTempFile> {
  let video_id = video_id_from_uri(uri)?.to_string();
  let (tmp, play_path, _) = download_audio(source, &video_id).await?;
  if play_path != tmp.path() && play_path.exists() {
    let _ = std::fs::copy(&play_path, tmp.path());
  }
  Ok(tmp)
}

pub(crate) async fn download_for_queue_track(
  app: &Arc<Mutex<App>>,
  source: &YouTubeSource,
  track: &TrackInfo,
) -> Result<NamedTempFile> {
  let video_id = resolve_or_extract_video_id(app, track).await?;
  let (tmp, play_path, _) = download_audio(source, &video_id).await?;
  if play_path != tmp.path() && play_path.exists() {
    let _ = std::fs::copy(&play_path, tmp.path());
  }
  Ok(tmp)
}

/// Decode-failure hint: without ffmpeg on `$PATH`, yt-dlp leaves the download
/// as a fragmented DASH container some decoders reject.
async fn decode_hint(app: &Arc<Mutex<App>>, e: impl std::fmt::Display) -> String {
  crate::infra::app_strings(app)
    .await
    .status
    .infra
    .youtube_decode_failed(&e.to_string())
}

/// Resolve a track to a YouTube video ID.
/// 1. If uri starts with `youtube:`, extracts video ID.
/// 2. If it's a URL or contains video ID, extracts it.
/// 3. Checks SQLite `resolution_cache` for existing manual remap or auto resolution.
/// 4. Queries YouTube InnerTube search with song title and artists (zero API keys).
/// 5. Caches the successful match in SQLite `resolution_cache` so future playback is instant!
/// 6. Falls back to yt-dlp search if needed.
async fn resolve_or_extract_video_id(
  app: &Arc<Mutex<App>>,
  track: &TrackInfo,
) -> Result<String> {
  // 1. Direct YouTube URI
  if let Some(ref uri) = track.uri {
    if let Ok(id) = video_id_from_uri(uri) {
      return Ok(id.to_string());
    }
    if let Some(id) = super::extract_youtube_video_id(uri) {
      return Ok(id);
    }
  }

  // 2. Check SQLite resolution cache
  let track_key = track.uri.clone().unwrap_or_else(|| {
    track.id.clone().unwrap_or_else(|| format!("{}:{}", track.artists.join(", "), track.name))
  });

  if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
    if let Ok(Some(entry)) = db.get_resolution(&track_key) {
      if let Some(path) = entry.local_archive_path {
        if let Some(id) = super::extract_youtube_video_id(&path) {
          return Ok(id);
        }
      }
    }
  }

  // 3. Search via YouTube InnerTube (fast, zero API keys)
  let mut track_name = track.name.clone();
  let mut track_artists = track.artists.clone();
  if track_artists.is_empty() || track_name.starts_with("spotify:") {
    if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
      if let Ok(Some((title, artist))) = db.find_track_metadata(&track_key) {
        track_name = title;
        track_artists = vec![artist];
      }
    }
  }

  let query = format!("{} {}", track_name, track_artists.join(" "));
  let clean_query = query.trim();
  if !clean_query.is_empty() {
    {
      let mut guard = app.lock().await;
      guard.set_status_message(format!("\u{1F50D} Resolving '{}' on YouTube...", track_name), 15);
    }

    let client = crate::sources::youtube_search::InnerTubeClient::new();
    // First try music scope
    let search_res = client.search(clean_query, crate::sources::youtube_search::SearchScope::Music).await;
    let found = match search_res {
      Ok(items) if !items.is_empty() => items.into_iter().find_map(|item| item.video_id),
      _ => None,
    };

    let video_id_opt = match found {
      Some(vid) => Some(vid),
      None => {
        // Fallback to All scope
        if let Ok(items) = client.search(clean_query, crate::sources::youtube_search::SearchScope::Video).await {
          items.into_iter().find_map(|item| item.video_id)
        } else {
          None
        }
      }
    };

    if let Some(vid) = video_id_opt {
      // Cache resolution in SQLite
      if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
        let entry = crate::sources::lossless_relay::db::ResolutionEntry {
          source_track_id: track_key,
          isrc: None,
          qobuz_id: None,
          tidal_id: None,
          resolution_method: "innertube_auto".to_string(),
          match_confidence: 0.95,
          local_archive_path: Some(format!("youtube:{}", vid)),
          resolved_at: chrono::Utc::now().timestamp(),
        };
        let _ = db.upsert_resolution(&entry);
      }
      return Ok(vid);
    }
  }

  // 4. Fallback to yt-dlp search
  let source = build_source(app).await;
  if let Ok(search) = source.search(clean_query).await {
    if let Some(first) = search.tracks.first() {
      if let Some(ref uri) = first.uri {
        if let Ok(vid) = video_id_from_uri(uri) {
          let vid_str = vid.to_string();
          if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
            let entry = crate::sources::lossless_relay::db::ResolutionEntry {
              source_track_id: track_key,
              isrc: None,
              qobuz_id: None,
              tidal_id: None,
              resolution_method: "ytdlp_search".to_string(),
              match_confidence: 0.90,
              local_archive_path: Some(format!("youtube:{}", vid_str)),
              resolved_at: chrono::Utc::now().timestamp(),
            };
            let _ = db.upsert_resolution(&entry);
          }
          return Ok(vid_str);
        }
      }
    }
  }

  anyhow::bail!("Could not resolve '{}' to a YouTube video. Press 'm' to remap.", track.name)
}

/// Begin playing a queue of YouTube videos, taking over the session and
/// starting at `start_idx` (clamped into range).
fn is_livestream(track: &TrackInfo) -> bool {
  track.uri.as_deref().is_some_and(|u| u.starts_with("youtube:live:") || u.contains("live=1"))
}

async fn start_youtube_queue(app: &Arc<Mutex<App>>, uris: &[String], start_idx: usize) {
  // Snapshot the track metadata under one short lock, from whichever browse
  // view the request came from.
  let mut tracks = {
    let guard = app.lock().await;
    let live_tracks: Vec<TrackInfo> = guard
      .live_search_items
      .iter()
      .map(|it| it.to_track_info())
      .collect();
    let search = guard
      .search_results()
      .tracks
      .as_ref()
      .map(|p| p.items.as_slice());
    let mut all_pool = guard.track_table.tracks.clone();
    all_pool.extend(live_tracks);
    if let crate::core::app::SearchDrilldown::AlbumDetail { ref tracks, .. } = guard.view.search_drilldown {
      all_pool.extend(tracks.clone());
    }
    if let crate::core::app::SearchDrilldown::ArtistDetail { ref top_songs, .. } = guard.view.search_drilldown {
      all_pool.extend(top_songs.clone());
    }
    snapshot_tracks(&all_pool, search, uris)
  };

  if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
    let all_downloaded = db.list_downloaded_tracks().unwrap_or_default();
    for track in tracks.iter_mut() {
      if track.artists.is_empty() || track.name.starts_with("yt:video:") || track.name.starts_with("youtube:") {
        if let Some(u) = track.uri.as_deref().or_else(|| track.id.as_deref()) {
          let vid = super::extract_youtube_video_id(u).unwrap_or_else(|| u.to_string());
          if let Ok(Some(pt)) = db.find_playlist_track_by_source_id(u)
            .or_else(|_| db.find_playlist_track_by_source_id(&vid))
            .or_else(|_| db.find_playlist_track_by_source_id(&format!("yt:video:{}", vid)))
          {
            track.name = pt.clean_title;
            track.artists = vec![pt.artist];
            track.album = pt.album.unwrap_or_default();
            track.duration_ms = pt.duration_ms as u64;
            track.image_url = pt.cover_art_url;
          } else if let Some(dt) = all_downloaded.iter().find(|d| {
            d.source_track_id == u || d.source_track_id == vid || d.file_path.contains(&vid)
          }) {
            track.name = dt.title.clone();
            track.artists = vec![dt.artist.clone()];
            track.album = dt.album.clone().unwrap_or_default();
            track.duration_ms = dt.duration_ms;
            track.image_url = dt.cover_art_url.clone();
          }
        }
      }
    }
  }

  if tracks.is_empty() {
    set_error(
      app,
      crate::infra::app_strings(app)
        .await
        .status
        .infra
        .youtube_no_tracks
        .to_string(),
    )
    .await;
    return;
  }
  let index = start_idx.min(tracks.len() - 1);

  // A livestream never finishes downloading — refuse up front.
  if is_livestream(&tracks[index]) {
    set_error(
      app,
      crate::infra::app_strings(app)
        .await
        .status
        .infra
        .youtube_no_live
        .to_string(),
    )
    .await;
    return;
  }

  let video_id = match resolve_or_extract_video_id(app, &tracks[index]).await {
    Ok(id) => {
      tracks[index].uri = Some(uri_for_video_id(&id));
      id
    }
    Err(e) => {
      set_error(app, e.to_string()).await;
      return;
    }
  };

  let source = Arc::new(build_source(app).await);

  // Only one backend owns the device at a time.
  app.lock().await.claim_decoded_sink(Source::YouTube);
  release_other_backends(app).await;

  let Some(player) = acquire_player(app).await else {
    return;
  };

  // yt-dlp can take a while; let the user know something is happening.
  {
    let mut guard = app.lock().await;
    {
      let strings = guard.strings();
      guard.set_status_message(strings.status.infra.fetching(&tracks[index].name), 30)
    };
  }

  // Download off the lock, then decode on the blocking pool.
  let (tmp, path, resolved) = match download_audio(&source, &video_id).await {
    Ok(t) => t,
    Err(e) => {
      log::error!("YouTube download failed for {video_id}: {e:#}");
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_download_failed(&e.to_string()),
      )
      .await;
      return;
    }
  };
  let play_path = path.clone();
  let decode_player = Arc::clone(&player);
  let result = tokio::task::spawn_blocking(move || decode_player.play_file(&play_path)).await;

  match result {
    Ok(Ok(())) => {
      let volume = app.lock().await.runtime_state.volume_percent;
      player.set_volume(volume);

      // An off-screen URI (#716) takes the title the download printed.
      if let Some(track) = resolved {
        apply_resolved(&mut tracks, index, &uri_for_video_id(&video_id), track);
      }
      let display = tracks[index].name.clone();
      let artists = tracks[index].artists.join(", ");
      let dur = tracks[index].duration_ms;
      let mut guard = app.lock().await;
      // Publish the session exactly once, now that the source is decoding.
      let mut state = YouTubePlaybackState {
        player,
        source,
        tracks,
        index,
        advancing: false,
        tempfile: tmp,
        audio_path: path,
        shuffle_backup: None,
        resume_at: None,
        prebuffering_id: None,
        prebuffered_id: None,
        radio_fetched_for: None,
        radio_fetching: false,
      };
      // Honor the player-global decoded shuffle for the freshly-built queue.
      if guard.decoded_shuffle {
        state.set_shuffle(true);
      }
      guard.youtube_playback = Some(state);
      guard.set_status_message(format!("\u{266a} {display}"), 4);
      drop(guard);
      spawn_karaoke_lyrics_fetch(Arc::clone(app), display, artists, dur);
      spawn_eager_queue_prebuffer(Arc::clone(app));
      spawn_infinite_autoplay_prefetch(Arc::clone(app));
    }
    Ok(Err(e)) => set_error(app, decode_hint(app, e).await).await,
    Err(e) => {
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_task_failed(&e.to_string()),
      )
      .await
    }
  }
}

/// Move the YouTube queue index in `direction` and play the new track. Returns
/// `true` if YouTube owns the session (so the event is consumed).
async fn skip(app: &Arc<Mutex<App>>, direction: Direction) -> bool {
  let target = {
    let mut guard = app.lock().await;
    // Repeat-all wraps at the ends; Off/Repeat-one clamp.
    let mode = guard.decoded_repeat;
    let Some(s) = guard.youtube_playback.as_mut() else {
      return false; // not ours
    };
    // Guard the empty-sink download window from spurious auto-advance.
    s.advancing = true;
    let forward = matches!(direction, Direction::Next);
    advance_index(s.index, s.tracks.len(), mode, forward)
  };

  match target {
    Some(idx) => play_index(app, idx).await,
    None => {
      if matches!(direction, Direction::Next) {
        let (can_autoplay, seed_id) = {
          let guard = app.lock().await;
          let ap = guard.user_config.behavior.streaming_autoplay;
          let seed = guard.youtube_playback.as_ref().and_then(|s| {
            s.tracks.get(s.index).and_then(|t| {
              t.id.clone().or_else(|| t.uri.as_deref().and_then(super::extract_youtube_video_id))
            })
          });
          (ap, seed)
        };
        if can_autoplay {
          if let Some(seed_id) = seed_id {
            let importer = crate::sources::lossless_relay::youtube_importer::YouTubeImporter::new();
            if let Ok(radio_tracks) = importer.fetch_youtube_radio_tracks(&seed_id).await {
              let next_target = {
                let mut guard = app.lock().await;
                if let Some(s) = guard.youtube_playback.as_mut() {
                  let existing_ids: std::collections::HashSet<String> = s.tracks.iter()
                    .filter_map(|t| t.id.clone().or_else(|| t.uri.as_deref().and_then(super::extract_youtube_video_id)))
                    .collect();
                  let start_len = s.tracks.len();
                  for t in radio_tracks {
                    let tid = t.id.clone()
                      .or_else(|| t.uri.as_deref().and_then(super::extract_youtube_video_id));
                    if let Some(id) = tid {
                      if !existing_ids.contains(&id) && id != seed_id {
                        s.tracks.push(t);
                      }
                    }
                  }
                  if s.tracks.len() > start_len {
                    Some(s.index + 1)
                  } else {
                    None
                  }
                } else {
                  None
                }
              };
              if let Some(idx) = next_target {
                play_index(app, idx).await;
                return true;
              }
            }
          }
        }
      }

      // Queue boundary: clear the optimistic guard so it doesn't wedge
      // auto-advance off for the rest of the track.
      if let Some(s) = app.lock().await.youtube_playback.as_mut() {
        s.advancing = false;
      }
    }
  }
  true
}

/// Replay the current YouTube video (repeat-one auto-advance) via
/// [`replay_file`]. Re-decodes the **already-downloaded tempfile** — no second
/// `yt-dlp` fetch — and on failure tears the session down (rather than leaving
/// an empty sink that would re-fire replay every tick). Returns `true` if
/// YouTube owns the session.
async fn replay_current(app: &Arc<Mutex<App>>) -> bool {
  let (player, path, resume) = {
    let mut guard = app.lock().await;
    let Some(s) = guard.youtube_playback.as_mut() else {
      return false; // not ours
    };
    s.advancing = true;
    (
      Arc::clone(&s.player),
      s.audio_path.clone(),
      s.resume_at
        .take()
        .or(Some(crate::infra::queue::ResumePoint {
          position_ms: 0,
          paused: s.player.is_paused(),
        })),
    )
  };
  if replay_file(player, path, resume).await {
    if let Some(s) = app.lock().await.youtube_playback.as_mut() {
      s.advancing = false;
    }
  } else {
    teardown_youtube(app).await;
    set_error(
      app,
      crate::infra::app_strings(app)
        .await
        .status
        .infra
        .youtube_replay_failed
        .to_string(),
    )
    .await;
  }
  true
}


/// Play the queued track at `target`, reusing the published session. Used by
/// Next/Previous and the runner tick's auto-advance.
///
/// A download failure tears the session down (same rationale as Subsonic): on
/// a network outage or a broken extractor, walking the whole queue at tick
/// speed would spray error toasts, so one failure ends playback instead.
pub(crate) async fn play_index(app: &Arc<Mutex<App>>, target: usize) {
  let (player, source, track) = {
    let guard = app.lock().await;
    match guard.youtube_playback.as_ref() {
      None => return, // session torn down between dispatch and here
      Some(s) => match s.tracks.get(target) {
        None => {
          drop(guard);
          if let Some(s) = app.lock().await.youtube_playback.as_mut() {
            s.advancing = false;
          }
          return;
        }
        Some(t) => (Arc::clone(&s.player), Arc::clone(&s.source), t.clone()),
      },
    }
  };

  if is_livestream(&track) {
    teardown_youtube(app).await;
    set_error(
      app,
      crate::infra::app_strings(app)
        .await
        .status
        .infra
        .youtube_no_live
        .to_string(),
    )
    .await;
    return;
  }

  let video_id = match resolve_or_extract_video_id(app, &track).await {
    Ok(id) => id,
    Err(e) => {
      log::warn!("Could not resolve video id for track '{}': {e:#}", track.name);
      let next_target = target + 1;
      let has_next = {
        let guard = app.lock().await;
        guard.youtube_playback.as_ref().is_some_and(|s| next_target < s.tracks.len())
      };
      if has_next {
        let mut guard = app.lock().await;
        guard.set_status_message(format!("Skipping unresolvable track '{}'...", track.name), 4);
        drop(guard);
        Box::pin(play_index(app, next_target)).await;
      } else {
        teardown_youtube(app).await;
        set_error(app, e.to_string()).await;
      }
      return;
    }
  };
  let name = track.name.clone();

  {
    let mut guard = app.lock().await;
    if let Some(s) = guard.youtube_playback.as_mut() {
      if let Some(t) = s.tracks.get_mut(target) {
        t.uri = Some(uri_for_video_id(&video_id));
      }
    }
    let strings = guard.strings();
    guard.set_status_message(strings.status.infra.fetching(&name), 30);
  }

  let (tmp, path, resolved) = match download_audio(&source, &video_id).await {
    Ok(t) => t,
    Err(e) => {
      log::error!("YouTube advance download failed for {video_id}: {e:#}");
      let next_target = target + 1;
      let has_next = {
        let guard = app.lock().await;
        guard.youtube_playback.as_ref().is_some_and(|s| next_target < s.tracks.len())
      };
      if has_next {
        let mut guard = app.lock().await;
        guard.set_status_message(format!("Skipping unavailable track '{}'...", name), 4);
        drop(guard);
        Box::pin(play_index(app, next_target)).await;
      } else {
        teardown_youtube(app).await;
        set_error(
          app,
          crate::infra::app_strings(app)
            .await
            .status
            .infra
            .youtube_download_failed(&e.to_string()),
        )
        .await;
      }
      return;
    }
  };
  let play_path = path.clone();
  // Taken after the download: a device recovery during it leaves the pause
  // to apply here.
  let resume = {
    let mut guard = app.lock().await;
    guard
      .youtube_playback
      .as_mut()
      .and_then(|s| s.resume_at.take())
  };
  let decode_player = Arc::clone(&player);
  let result = tokio::task::spawn_blocking(move || {
    crate::infra::queue::restage(&decode_player, &play_path, resume)
  })
  .await;

  match result {
    Ok(Ok(())) => {
      let resolved = resolved.map(|track| (uri_for_video_id(&video_id), track));
      commit_index(app, target, tmp, path, resolved).await
    }
    Ok(Err(e)) => {
      teardown_youtube(app).await;
      set_error(app, decode_hint(app, e).await).await;
    }
    Err(e) => {
      teardown_youtube(app).await;
      set_error(
        app,
        crate::infra::app_strings(app)
          .await
          .status
          .infra
          .youtube_task_failed(&e.to_string()),
      )
      .await;
    }
  }
}

/// Commit `target` as the live index, clear the auto-advance guard, and swap
/// in the new track's tempfile (dropping the previous one). Ordering is safe:
/// the blocking `play_file` already cleared the old source from the sink, so
/// rodio no longer holds the old file by the time it is dropped here.
///
/// `resolved` is the metadata the download printed for the URI it fetched;
/// it replaces the row only while that row is the URI's placeholder (#716).
async fn commit_index(
  app: &Arc<Mutex<App>>,
  target: usize,
  tmp: NamedTempFile,
  path: std::path::PathBuf,
  resolved: Option<(String, TrackInfo)>,
) {
  let mut guard = app.lock().await;
  let track_meta = if let Some(s) = guard.youtube_playback.as_mut() {
    s.index = target;
    s.advancing = false;
    s.tempfile = tmp;
    s.audio_path = path;
    s.prebuffering_id = None;
    s.prebuffered_id = None;
    if let Some((uri, track)) = resolved {
      apply_resolved(&mut s.tracks, target, &uri, track);
    }
    s.tracks.get(target).map(|t| (t.name.clone(), t.artists.join(", "), t.duration_ms))
  } else {
    None
  };
  if let Some((display, artists, dur)) = track_meta {
    guard.set_status_message(format!("\u{266a} {display}"), 4);
    drop(guard);
    spawn_karaoke_lyrics_fetch(Arc::clone(app), display, artists, dur);
    spawn_eager_queue_prebuffer(Arc::clone(app));
    spawn_infinite_autoplay_prefetch(Arc::clone(app));
  }
}

/// Eagerly resolve and prefetch upcoming tracks in the active queue/playlist into local cache.
pub(crate) fn spawn_eager_queue_prebuffer(app: Arc<Mutex<App>>) {
  tokio::spawn(async move {
    let upcoming = {
      let guard = app.lock().await;
      let mut targets = Vec::new();
      if let Some(ref s) = guard.youtube_playback {
        for offset in 1..=2 {
          let idx = s.index + offset;
          if let Some(t) = s.tracks.get(idx) {
            targets.push((Some(idx), t.clone()));
          }
        }
      }
      if let Some(head) = guard.native_queue.first().cloned() {
        targets.push((None, head));
      }
      targets
    };

    if upcoming.is_empty() {
      return;
    }

    let source = Arc::new(build_source(&app).await);

    for (maybe_idx, track) in upcoming {
      let vid = match resolve_or_extract_video_id(&app, &track).await {
        Ok(id) => id,
        Err(_) => continue,
      };

      if let Some(idx) = maybe_idx {
        let mut guard = app.lock().await;
        if let Some(ref mut s) = guard.youtube_playback {
          if let Some(t) = s.tracks.get_mut(idx) {
            t.uri = Some(super::uri_for_video_id(&vid));
          }
        }
      }

      if super::cached_audio_path(&vid).is_none() {
        if let Ok(_) = source.prefetch_to_cache(&vid).await {
          let mut guard = app.lock().await;
          if let Some(ref mut s) = guard.youtube_playback {
            s.prebuffered_id = Some(vid);
          }
        }
      }
    }
  });
}

/// Algorithmic infinite auto-play: automatically discovers and appends YouTube Radio tracks
/// when playback approaches or reaches the end of the active queue.
pub(crate) fn spawn_infinite_autoplay_prefetch(app: Arc<Mutex<App>>) {
  tokio::spawn(async move {
    let (should_fetch, current_video_id, current_title, current_artist) = {
      let mut guard = app.lock().await;
      if !guard.user_config.behavior.streaming_autoplay {
        return;
      }
      let Some(s) = guard.youtube_playback.as_mut() else {
        return;
      };
      // Only fetch when queue is nearing completion (remaining <= 2 tracks)
      let remaining = s.tracks.len().saturating_sub(s.index);
      if remaining > 2 {
        return;
      }
      if s.radio_fetching {
        return;
      }
      let Some(curr) = s.tracks.get(s.index) else {
        return;
      };
      let vid = curr.id.clone()
        .or_else(|| curr.uri.as_deref().and_then(super::extract_youtube_video_id));
      let Some(video_id) = vid else {
        return;
      };
      if s.radio_fetched_for.as_deref() == Some(&video_id) {
        return;
      }
      s.radio_fetching = true;
      (true, video_id, curr.name.clone(), curr.artists.join(", "))
    };

    if !should_fetch {
      return;
    }

    log::info!("Infinite Auto-Play: Fetching radio recommendations for video {current_video_id} ('{current_title}')");
    let importer = crate::sources::lossless_relay::youtube_importer::YouTubeImporter::new();
    let radio_tracks = match importer.fetch_youtube_radio_tracks(&current_video_id).await {
      Ok(tracks) if !tracks.is_empty() => tracks,
      err => {
        log::warn!("InnerTube radio fetch failed or empty ({:?}), falling back to related search...", err);
        let query = format!("{} mix music", current_artist);
        let source = build_source(&app).await;
        match source.search(&query).await {
          Ok(results) => results.tracks,
          Err(e) => {
            log::error!("Auto-Play fallback search failed: {e:#}");
            Vec::new()
          }
        }
      }
    };

    let mut guard = app.lock().await;
    let Some(s) = guard.youtube_playback.as_mut() else {
      return;
    };
    s.radio_fetching = false;
    s.radio_fetched_for = Some(current_video_id.clone());

    if radio_tracks.is_empty() {
      return;
    }

    let existing_ids: std::collections::HashSet<String> = s.tracks.iter()
      .filter_map(|t| t.id.clone().or_else(|| t.uri.as_deref().and_then(super::extract_youtube_video_id)))
      .collect();

    let mut added = 0;
    for track in radio_tracks {
      let tid = track.id.clone()
        .or_else(|| track.uri.as_deref().and_then(super::extract_youtube_video_id));
      if let Some(id) = tid {
        if !existing_ids.contains(&id) && id != current_video_id {
          s.tracks.push(track);
          added += 1;
        }
      }
    }

    if added > 0 {
      guard.set_status_message(format!("Auto-Play: Discovered and queued {} similar tracks", added), 4);
      drop(guard);
      // Trigger eager pre-buffering on the newly discovered upcoming tracks
      spawn_eager_queue_prebuffer(Arc::clone(&app));
    }
  });
}

pub(crate) fn spawn_karaoke_lyrics_fetch(
  app: Arc<Mutex<App>>,
  _title: String,
  _artist: String,
  _duration_ms: u64,
) {
  tokio::spawn(async move {
    refetch_active_karaoke_lyrics(app, false).await;
  });
}

pub(crate) async fn refetch_active_karaoke_lyrics(
  app: Arc<Mutex<App>>,
  force: bool,
) {
  let (title, artist, duration_ms, _provider_idx) = {
    let guard = app.lock().await;
    let playing = if let Some(queue_track) = if let Some(crate::infra::queue::QueueNowPlaying::Decoded(d)) = guard.queue_now.as_ref() {
      Some(&d.track)
    } else if let Some(crate::infra::queue::QueueNowPlaying::Spotify { track }) = guard.queue_now.as_ref() {
      Some(track)
    } else {
      None
    } {
      Some((queue_track.name.clone(), queue_track.artists.join(", "), if queue_track.duration_ms > 0 { Some(queue_track.duration_ms) } else { None }))
    } else if let Some(yt) = guard.youtube_playback.as_ref() {
      yt.current().map(|t| (t.name.clone(), t.artists.join(", "), if t.duration_ms > 0 { Some(t.duration_ms) } else { None }))
    } else if let Some(l) = guard.lossless_playback.as_ref() {
      l.current_track.as_ref().map(|t| (t.clean_title.clone(), t.artist.clone(), if t.duration_ms > 0 { Some(t.duration_ms as u64) } else { None }))
    } else if guard.view.track_table_index < guard.track_table.tracks.len() {
      let t = &guard.track_table.tracks[guard.view.track_table_index];
      Some((t.name.clone(), t.artists.join(", "), if t.duration_ms > 0 { Some(t.duration_ms) } else { None }))
    } else {
      None
    };

    match playing {
      Some((t, a, d)) => (t, a, d.unwrap_or(0), ()),
      None => return,
    }
  };

  let (clean_artist, clean_title) =
    crate::sources::lossless_relay::youtube_importer::clean_youtube_title(&title, Some(&artist));
  let dur_sec = if duration_ms > 0 { Some((duration_ms / 1000) as i64) } else { None };
  let track_key = format!("{}:{}", clean_artist.to_lowercase(), clean_title.to_lowercase());

  // If not forcing, check DB cache first
  if !force {
    if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
      if let Ok(Some(entry)) = db.get_lyrics(&track_key) {
        if let Ok(mut tl) = serde_json::from_str::<crate::lyrics::KaraokeTimeline>(&entry.timeline_json) {
          tl.ensure_romanization();
          let lines: Vec<(u128, String)> = tl.lines.iter().map(|l| (l.start_time_ms as u128, l.text.clone())).collect();
          let mut guard = app.lock().await;
          guard.view.active_karaoke_timeline = Some(tl.clone());
          guard.set_lyrics(crate::core::app::LyricsStatus::Found, Some(lines), true);
          guard.set_status_message(format!("Lyrics cached ({})", tl.provider), 3);
          return;
        }
      }
    }
  }

  // Auto Waterfall: Priority LRCLIB (exact get -> fuzzy search) -> Paxsenix
  let client = crate::lyrics::lrclib_elrc::LrclibClient::default();
  let mut timeline = None;

  // 1. Exact match via LRCLIB /api/get (if artist is known)
  if !clean_artist.is_empty() && clean_artist != "Unknown Artist" {
    timeline = client.fetch_lyrics(&clean_title, &clean_artist, None, dur_sec).await.ok();
  }

  // 2. Fuzzy search via LRCLIB /api/search with "clean_artist clean_title"
  if timeline.is_none() && !clean_artist.is_empty() && clean_artist != "Unknown Artist" {
    timeline = client.search_lyrics(&format!("{} {}", clean_artist, clean_title)).await.ok();
  }

  // 3. Fuzzy search via LRCLIB /api/search with clean_title
  if timeline.is_none() && !clean_title.is_empty() {
    timeline = client.search_lyrics(&clean_title).await.ok();
  }

  // 4. Fuzzy search via LRCLIB /api/search with raw YouTube artist + title
  if timeline.is_none() && !artist.is_empty() && !title.is_empty() {
    timeline = client.search_lyrics(&format!("{} {}", artist, title)).await.ok();
  }

  // 5. Fuzzy search via LRCLIB /api/search with raw YouTube title
  if timeline.is_none() && !title.is_empty() && title != clean_title {
    timeline = client.search_lyrics(&title).await.ok();
  }

  // 6. Paxsenix fallback with "clean_artist clean_title"
  if timeline.is_none() {
    let query_str = if !clean_artist.is_empty() && clean_artist != "Unknown Artist" {
      format!("{} {}", clean_artist, clean_title)
    } else {
      clean_title.clone()
    };
    let url = "https://lyrics.paxsenix.org/lyrics";
    let http = reqwest::Client::new();
    if let Ok(resp) = http.get(url).query(&[("q", &query_str)]).send().await {
      if let Ok(text) = resp.text().await {
        if text.trim_start().starts_with('<') {
          timeline = crate::lyrics::paxsenix_ttml::parse_apple_music_ttml(&text, "paxsenix_ttml").ok();
        } else {
          timeline = crate::lyrics::paxsenix_ttml::parse_musixmatch_richsync(&text, "paxsenix_richsync").ok();
        }
      }
    }
  }

  // 7. Paxsenix fallback with raw title
  if timeline.is_none() && !title.is_empty() && title != clean_title {
    let url = "https://lyrics.paxsenix.org/lyrics";
    let http = reqwest::Client::new();
    if let Ok(resp) = http.get(url).query(&[("q", &title)]).send().await {
      if let Ok(text) = resp.text().await {
        if text.trim_start().starts_with('<') {
          timeline = crate::lyrics::paxsenix_ttml::parse_apple_music_ttml(&text, "paxsenix_ttml").ok();
        } else {
          timeline = crate::lyrics::paxsenix_ttml::parse_musixmatch_richsync(&text, "paxsenix_richsync").ok();
        }
      }
    }
  }

  if let Some(mut tl) = timeline {
    tl.ensure_romanization();
    if let Ok(db) = crate::sources::lossless_relay::db::Database::open_default() {
      let json = serde_json::to_string(&tl).unwrap_or_default();
      let entry = crate::sources::lossless_relay::db::LyricsEntry {
        source_track_id: track_key,
        isrc: None,
        provider: tl.provider.clone(),
        sync_type: match tl.sync_type {
          crate::lyrics::SyncType::Syllable => "SYLLABLE".into(),
          crate::lyrics::SyncType::Line => "LINE".into(),
          crate::lyrics::SyncType::Unsynced => "UNSYNCED".into(),
        },
        timeline_json: json,
        raw_elrc: tl.raw_elrc.clone(),
        user_offset_ms: 0,
        fetched_at: chrono::Utc::now().timestamp(),
      };
      db.upsert_lyrics(&entry).ok();
    }

    let lines: Vec<(u128, String)> = tl.lines.iter().map(|l| (l.start_time_ms as u128, l.text.clone())).collect();
    let count = tl.lines.len();
    let prov = tl.provider.clone();
    let mut guard = app.lock().await;
    guard.view.active_karaoke_timeline = Some(tl);
    guard.set_lyrics(crate::core::app::LyricsStatus::Found, Some(lines), true);
    guard.set_status_message(format!("Lyrics loaded from {} ({} lines) (Auto)", prov, count), 4);
  } else {
    let mut guard = app.lock().await;
    guard.set_status_message(format!("No lyrics found for '{}' by '{}'", clean_title, clean_artist), 4);
  }
}

pub(crate) async fn fetch_album_drilldown(app: &Arc<Mutex<App>>, album_id: &str, title: &str) {
  let importer = crate::sources::lossless_relay::youtube_importer::YouTubeImporter::new();
  let mut tracks = Vec::new();
  let mut album_artist_opt = None;
  let mut year_opt = None;
  let mut cover_opt = None;

  let clean_bid = album_id
    .strip_prefix("yt:browse:")
    .or_else(|| album_id.strip_prefix("yt:playlist:"))
    .unwrap_or(album_id);

  // 1. Try dedicated album browse (handles MPREb_ album containers with twoColumnBrowseResultsRenderer)
  if let Ok((_alb_title, alb_artist, yr, cover, alb_tracks)) = importer.fetch_innertube_album(clean_bid).await {
    if !alb_tracks.is_empty() {
      tracks = alb_tracks;
      if !alb_artist.is_empty() {
        album_artist_opt = Some(alb_artist);
      }
      year_opt = yr;
      cover_opt = cover;
    }
  }

  // 2. Fallback to playlist browse (VL / PL / OLAK5uy_) if empty
  if tracks.is_empty() && (clean_bid.starts_with("VL") || clean_bid.starts_with("PL") || clean_bid.starts_with("OLAK5uy_")) {
    if let Ok((_pl, pl_tracks)) = importer.fetch_innertube_playlist(clean_bid).await {
      for t in pl_tracks {
        tracks.push(TrackInfo {
          uri: Some(t.source_track_id.clone()),
          name: t.clean_title,
          artists: vec![t.artist],
          album: title.to_string(),
          duration_ms: t.duration_ms as u64,
          id: Some(t.source_track_id),
          album_id: Some(album_id.to_string()),
          artist_refs: Vec::new(),
          is_playable: true,
          is_local: false,
          track_number: 1,
          explicit: false,
          image_url: t.cover_art_url,
          release_date: None,
          generated_name: None,
        });
      }
    }
  }

  // 3. Fallback: Search album container on YouTube Music
  if tracks.is_empty() {
    let client = crate::sources::youtube_search::InnerTubeClient::new();
    if let Ok(results) = client.search(title, crate::sources::youtube_search::types::SearchScope::Album).await {
      let matched = results.into_iter().find(|r| {
        r.playlist_id.as_ref().is_some_and(|bid| bid.starts_with("MPREb_") || bid.starts_with("VL") || bid.starts_with("OLAK5uy_"))
          || r.id.starts_with("yt:browse:MPREb_")
      });
      if let Some(r) = matched {
        let real_bid = r.playlist_id.as_deref().unwrap_or(&r.id);
        let clean = real_bid.strip_prefix("yt:browse:").unwrap_or(real_bid);
        if clean.starts_with("MPREb_") {
          if let Ok((_alb_title, alb_artist, yr, cover, alb_tracks)) = importer.fetch_innertube_album(clean).await {
            if !alb_tracks.is_empty() {
              tracks = alb_tracks;
              if !alb_artist.is_empty() { album_artist_opt = Some(alb_artist); }
              year_opt = yr;
              cover_opt = cover;
            }
          }
        } else if clean.starts_with("VL") || clean.starts_with("PL") || clean.starts_with("OLAK5uy_") {
          if let Ok((_pl, pl_tracks)) = importer.fetch_innertube_playlist(clean).await {
            for t in pl_tracks {
              tracks.push(TrackInfo {
                uri: Some(t.source_track_id.clone()),
                name: t.clean_title,
                artists: vec![t.artist],
                album: title.to_string(),
                duration_ms: t.duration_ms as u64,
                id: Some(t.source_track_id),
                album_id: Some(album_id.to_string()),
                artist_refs: Vec::new(),
                is_playable: true,
                is_local: false,
                track_number: 1,
                explicit: false,
                image_url: t.cover_art_url,
                release_date: None,
                generated_name: None,
              });
            }
          }
        }
      }
    }
  }

  let mut guard = app.lock().await;
  if let crate::core::app::SearchDrilldown::AlbumDetail {
    tracks: ref mut cur_tracks,
    album_id: ref cur_id,
    artist: ref mut cur_artist,
    year: ref mut cur_year,
    cover_art_url: ref mut cur_cover,
    ..
  } = guard.view.search_drilldown {
    if cur_id == album_id || cur_tracks.is_empty() {
      *cur_tracks = tracks;
      if let Some(a) = album_artist_opt {
        if cur_artist.is_empty() {
          *cur_artist = a;
        }
      }
      if cur_year.is_none() && year_opt.is_some() {
        *cur_year = year_opt;
      }
      if cur_cover.is_none() && cover_opt.is_some() {
        *cur_cover = cover_opt;
      }
    }
  }
}

pub(crate) async fn fetch_artist_drilldown(app: &Arc<Mutex<App>>, artist_id: &str, name: &str) {
  let importer = crate::sources::lossless_relay::youtube_importer::YouTubeImporter::new();
  let client = crate::sources::youtube_search::InnerTubeClient::new();
  let mut top_songs = Vec::new();
  let mut albums = Vec::new();

  let clean_bid = artist_id.strip_prefix("yt:browse:").unwrap_or(artist_id);

  // 1. If artist_id has a UC channel browse ID, fetch directly from InnerTube
  if clean_bid.starts_with("UC") || clean_bid.starts_with("FEmusic_") {
    if let Ok((_name, songs, albs)) = importer.fetch_innertube_artist(clean_bid).await {
      if !songs.is_empty() || !albs.is_empty() {
        top_songs = songs;
        albums = albs;
      }
    }
  }

  // 2. If browseId was not directly available, search for the artist's official channel ID
  if top_songs.is_empty() && albums.is_empty() {
    if let Ok(artist_results) = client.search(name, crate::sources::youtube_search::types::SearchScope::Artist).await {
      let matched = artist_results.into_iter().find(|r| {
        let title_lower = r.title.to_lowercase();
        let name_lower = name.to_lowercase();
        title_lower == name_lower || title_lower.contains(&name_lower) || name_lower.contains(&title_lower)
      });
      if let Some(art) = matched {
        let channel_id = art.playlist_id.as_deref().unwrap_or(&art.id);
        let clean_cid = channel_id.strip_prefix("yt:browse:").unwrap_or(channel_id);
        if clean_cid.starts_with("UC") {
          if let Ok((_name, songs, albs)) = importer.fetch_innertube_artist(clean_cid).await {
            top_songs = songs;
            albums = albs;
          }
        }
      }
    }
  }

  // 3. Fallback: Search Scope::Music and Scope::Album with STRICT ARTIST MATCHING
  if top_songs.is_empty() {
    if let Ok(results) = client.search(name, crate::sources::youtube_search::types::SearchScope::Music).await {
      for r in results.into_iter() {
        let artist_match = r.artist.to_lowercase().contains(&name.to_lowercase())
          || name.to_lowercase().contains(&r.artist.to_lowercase());
        if !artist_match {
          continue;
        }
        if let Some(vid) = r.video_id {
          let uri = format!("yt:video:{}", vid);
          top_songs.push(TrackInfo {
            uri: Some(uri.clone()),
            name: r.title,
            artists: vec![r.artist],
            album: r.album.unwrap_or_default(),
            duration_ms: r.duration_ms.max(0) as u64,
            id: Some(uri),
            album_id: None,
            artist_refs: Vec::new(),
            is_playable: true,
            is_local: false,
            track_number: 1,
            explicit: false,
            image_url: r.thumbnail_url,
            release_date: None,
            generated_name: None,
          });
          if top_songs.len() >= 25 {
            break;
          }
        }
      }
    }
  }

  if albums.is_empty() {
    if let Ok(results) = client.search(name, crate::sources::youtube_search::types::SearchScope::Album).await {
      for r in results.into_iter() {
        let artist_match = r.artist.to_lowercase().contains(&name.to_lowercase())
          || name.to_lowercase().contains(&r.artist.to_lowercase());
        if !artist_match {
          continue;
        }
        let id = r.playlist_id.unwrap_or_else(|| r.id.clone());
        if !albums.iter().any(|a| a.title.eq_ignore_ascii_case(&r.title)) {
          albums.push(crate::core::app::AlbumDetailHeader {
            id,
            title: r.title,
            year: None,
            track_count: 0,
          });
        }
        if albums.len() >= 20 {
          break;
        }
      }
    }
  }

  let mut guard = app.lock().await;
  if let crate::core::app::SearchDrilldown::ArtistDetail {
    top_songs: ref mut cur_songs,
    albums: ref mut cur_albums,
    artist_id: ref cur_id,
    ..
  } = guard.view.search_drilldown {
    if cur_id == artist_id || cur_songs.is_empty() {
      *cur_songs = top_songs;
      *cur_albums = albums;
    }
  }
}

/// End the YouTube session, releasing the output device and cleaning up the
/// current tempfile.
pub async fn teardown_youtube(app: &Arc<Mutex<App>>) {
  // Take under the lock, stop off it: a sink clear waits for the audio thread.
  let session = app.lock().await.youtube_playback.take();
  if let Some(s) = session {
    // Dropping `s` drops the tempfile (cleanup) and, if it held the last
    // reference, the keepalive thread exits and the device is released.
    Arc::clone(&s.player).stop_detached_holding(s);
  }
}

async fn set_error(app: &Arc<Mutex<App>>, message: String) {
  app.lock().await.set_status_message(message, 6);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::pagination::Paged;
  use crate::core::user_config::UserConfig;
  use std::sync::mpsc::channel;
  use std::time::SystemTime;

  fn test_app() -> App {
    let (tx, _rx) = channel();
    App::new(tx, UserConfig::new(), Some(SystemTime::now()))
  }

  fn video(uri: &str, name: &str) -> TrackInfo {
    TrackInfo {
      generated_name: None,
      uri: Some(uri.to_string()),
      name: name.to_string(),
      artists: vec!["Channel".to_string()],
      album: "YouTube".to_string(),
      duration_ms: 1000,
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

  /// Search/transport events that are not YouTube's must fall through.
  #[tokio::test]
  async fn foreign_events_fall_through_without_a_session() {
    let app = Arc::new(Mutex::new(test_app()));
    // No YouTube session: transport events are not ours.
    assert!(!route_youtube_event(&app, &IoEvent::PausePlayback).await);
    assert!(!route_youtube_event(&app, &IoEvent::NextTrack).await);
    assert!(!route_youtube_event(&app, &IoEvent::Seek(1000)).await);
    assert!(!route_youtube_event(&app, &IoEvent::StartPlayback(None, None, None)).await);
    // A Spotify start falls through (and there is nothing to tear down).
    assert!(
      !route_youtube_event(
        &app,
        &IoEvent::StartPlayback(Some("spotify:track:x".to_string()), None, None)
      )
      .await
    );
    // Other sources' URIs are not ours either.
    assert!(
      !route_youtube_event(
        &app,
        &IoEvent::StartPlayback(Some("radio:https://x.example/s".to_string()), None, None)
      )
      .await
    );
  }

  /// A youtube: start with no matching browse row (an MCP or AI DJ
  /// `play_now`) is ours and goes on to the download instead of being refused
  /// as a livestream (#716). yt-dlp points at a path that does not exist, so
  /// the start fails right after: at the download, or earlier at the audio
  /// output where there is none.
  #[tokio::test]
  async fn an_off_screen_youtube_start_is_not_refused_as_a_livestream() {
    let mut app = test_app();
    app.user_config.behavior.ytdlp_path = Some("/nonexistent/echovena-test/yt-dlp".to_string());
    let app = Arc::new(Mutex::new(app));
    assert!(
      route_youtube_event(
        &app,
        &IoEvent::StartPlayback(None, Some(vec!["youtube:abc123".to_string()]), Some(0))
      )
      .await
    );
    let guard = app.lock().await;
    assert!(guard.youtube_playback.is_none());
    let message = guard.status_message().unwrap_or_default().to_lowercase();
    assert!(
      !message.contains("live streams"),
      "an off-screen URI must not be refused as live, got {message:?}"
    );
    assert!(
      message.contains("yt-dlp") || message.contains("audio output"),
      "the start should reach the player and the download, got {message:?}"
    );
  }

  /// Livestream rows (duration 0) are refused up front with a clear message —
  /// their download never finishes, so starting one would hang "Fetching…"
  /// until the timeout.
  #[tokio::test]
  async fn livestream_rows_are_refused_without_session() {
    let app = Arc::new(Mutex::new(test_app()));
    {
      let mut guard = app.lock().await;
      let mut live = video("youtube:live12345", "24/7 Lofi Radio");
      live.duration_ms = 0;
      guard.set_search_results(crate::core::app::SearchResult {
        tracks: Some(Paged {
          items: vec![live],
          total: 1,
          ..Default::default()
        }),
        ..Default::default()
      });
    }
    assert!(
      route_youtube_event(
        &app,
        &IoEvent::StartPlayback(None, Some(vec!["youtube:live12345".to_string()]), Some(0))
      )
      .await,
      "livestream start is ours (consumed)"
    );
    let guard = app.lock().await;
    assert!(guard.youtube_playback.is_none(), "no session published");
    assert!(
      guard
        .status_message()
        .is_some_and(|m| m.contains("Live streams")),
      "user gets an actionable message, got {:?}",
      guard.status_message()
    );
  }

  /// Full local-playlist lifecycle through the dispatch, exactly as the
  /// runtime pump drives it: create → list → add (metadata resolved from a
  /// fake search view) → open → duplicate-add no-op → remove → delete. No
  /// network, no audio device — the file lives in a tempdir via the
  /// `ECHOVENA_YOUTUBE_PLAYLISTS_PATH` override. Kept as ONE test because the
  /// override is process-global env.
  #[tokio::test]
  async fn playlist_lifecycle_through_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("yt_playlists.yml");
    std::env::set_var(super::super::playlists::PATH_ENV, &path);

    let app = Arc::new(Mutex::new(test_app()));

    // Empty file → empty sidebar list.
    assert!(route_youtube_event(&app, &IoEvent::GetYouTubePlaylists).await);
    assert!(app.lock().await.youtube_playlists().is_empty());

    // Create a playlist; the sidebar refreshes.
    assert!(route_youtube_event(&app, &IoEvent::CreateYouTubePlaylist("Focus".to_string())).await);
    let playlist_uri = {
      let guard = app.lock().await;
      assert_eq!(guard.youtube_playlists().len(), 1);
      assert_eq!(guard.youtube_playlists()[0].name, "Focus");
      guard.youtube_playlists()[0].uri.clone()
    };

    // Fake a search view holding the video's metadata, then add it by id —
    // exactly what the picker dialog dispatches.
    {
      let mut guard = app.lock().await;
      // The search row's id field carries the bare video id.
      let mut row = video("youtube:vid1234", "Cool Song");
      row.id = Some("vid1234".to_string());
      guard.set_search_results(crate::core::app::SearchResult {
        tracks: Some(Paged {
          items: vec![row],
          total: 1,
          ..Default::default()
        }),
        ..Default::default()
      });
    }
    assert!(
      route_youtube_event(
        &app,
        &IoEvent::AddTrackToYouTubePlaylist(playlist_uri.clone(), "vid1234".to_string())
      )
      .await
    );
    assert_eq!(
      app.lock().await.youtube_playlists()[0].track_count,
      1,
      "sidebar count must refresh after an add"
    );

    // Adding the same video again is a friendly no-op.
    assert!(
      route_youtube_event(
        &app,
        &IoEvent::AddTrackToYouTubePlaylist(playlist_uri.clone(), "vid1234".to_string())
      )
      .await
    );
    assert_eq!(app.lock().await.youtube_playlists()[0].track_count, 1);

    // Open the playlist into the shared track table.
    assert!(route_youtube_event(&app, &IoEvent::GetYouTubeTracks(playlist_uri.clone())).await);
    {
      let guard = app.lock().await;
      assert_eq!(guard.track_table.tracks.len(), 1);
      assert_eq!(guard.track_table.tracks[0].name, "Cool Song");
      assert_eq!(
        guard.track_table.tracks[0].uri.as_deref(),
        Some("youtube:vid1234")
      );
      assert_eq!(
        guard.youtube_open_playlist.as_deref(),
        Some(playlist_uri.as_str())
      );
    }

    // Remove the track; the open table refreshes in place.
    assert!(
      route_youtube_event(
        &app,
        &IoEvent::RemoveTrackFromYouTubePlaylist(playlist_uri.clone(), "vid1234".to_string())
      )
      .await
    );
    {
      let guard = app.lock().await;
      assert!(
        guard.track_table.tracks.is_empty(),
        "open table must refresh"
      );
      assert_eq!(guard.youtube_playlists()[0].track_count, 0);
    }

    // Delete the playlist; the sidebar refreshes and the open marker clears.
    assert!(route_youtube_event(&app, &IoEvent::DeleteYouTubePlaylist(playlist_uri)).await);
    {
      let guard = app.lock().await;
      assert!(guard.youtube_playlists().is_empty());
      assert!(guard.youtube_open_playlist.is_none());
    }

    // And the file on disk reflects all of it (empty playlists list).
    let on_disk = super::super::playlists::load(&path).unwrap();
    assert!(on_disk.playlists.is_empty());
  }

  /// End-to-end dispatch test: drive `route_youtube_event` exactly as the
  /// runtime pump does — search, play a result, advance the queue, and tear
  /// down on a foreign start. Ignored (needs yt-dlp, network, AND an audio
  /// output device); run:
  /// `cargo test --features youtube -- --ignored live_dispatch`
  #[tokio::test(flavor = "multi_thread")]
  #[ignore = "requires yt-dlp on PATH, network, AND an audio output device"]
  async fn live_dispatch_search_play_and_advance() {
    let app = Arc::new(Mutex::new(test_app()));

    // Search populates the songs block.
    assert!(
      route_youtube_event(
        &app,
        &IoEvent::GetYouTubeSearchResults("daft punk get lucky".to_string())
      )
      .await
    );
    let uris: Vec<String> = app
      .lock()
      .await
      .search_results()
      .tracks
      .as_ref()
      .expect("search populated the songs block")
      .items
      .iter()
      .filter_map(|t| t.uri.clone())
      .collect();
    assert!(uris.len() >= 2, "need >=2 results to test advance");

    // Start the queue at index 0 — downloads + plays the first video.
    assert!(
      route_youtube_event(&app, &IoEvent::StartPlayback(None, Some(uris), Some(0))).await,
      "youtube StartPlayback must be consumed"
    );
    {
      let guard = app.lock().await;
      let s = guard.youtube_playback.as_ref().expect("session published");
      assert_eq!(s.index, 0);
      assert!(!s.player.is_paused(), "should be playing");
    }

    // Pause/resume are ours while the session lives.
    assert!(route_youtube_event(&app, &IoEvent::PausePlayback).await);
    assert!(app
      .lock()
      .await
      .youtube_playback
      .as_ref()
      .is_some_and(|s| s.player.is_paused()));
    assert!(route_youtube_event(&app, &IoEvent::StartPlayback(None, None, None)).await);

    // Advance — downloads + plays the next video, moving the index.
    assert!(route_youtube_event(&app, &IoEvent::NextTrack).await);
    {
      let guard = app.lock().await;
      let s = guard
        .youtube_playback
        .as_ref()
        .expect("session still active");
      assert_eq!(s.index, 1, "Next should advance the queue index");
    }

    // A foreign (Spotify) start tears the session down and falls through.
    assert!(
      !route_youtube_event(
        &app,
        &IoEvent::StartPlayback(Some("spotify:track:x".to_string()), None, None)
      )
      .await,
      "a non-youtube start must fall through to the network"
    );
    assert!(
      app.lock().await.youtube_playback.is_none(),
      "a foreign start must tear down the YouTube session (device handoff)"
    );
  }
}
