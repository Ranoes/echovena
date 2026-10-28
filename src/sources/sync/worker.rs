use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::core::app::App;
use crate::sources::lossless_relay::db::Database;
use crate::sources::lossless_relay::spotify_cookie_sync::SpotifyCookieSync;
use crate::sources::lossless_relay::youtube_importer::YouTubeImporter;
use crate::sources::sync::auth::SpotifyAuthHandler;

/// Summary report returned after a playlist sync execution pass
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncReport {
    pub playlists_synced: usize,
    pub total_tracks_synced: usize,
    pub new_tracks_added: usize,
    pub errors: Vec<String>,
}

/// Background worker managing bidirectional synchronization of remote
/// Spotify and YouTube playlists into the local SQLite database.
pub struct SyncWorker {
    db: Database,
}

impl SyncWorker {
    pub fn new() -> Result<Self> {
        let db = Database::open_default().context("failed to open default library.db")?;
        Ok(Self { db })
    }

    pub fn with_db(db: Database) -> Self {
        Self { db }
    }

    /// Database handle for query access
    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Runs a full sync pass across all authenticated remote sources
    pub async fn run_sync(&self) -> SyncReport {
        let mut report = SyncReport::default();

        // 1. Spotify Sync (Liked Songs + User Playlists)
        if let Some(sp_dc) = SpotifyAuthHandler::resolve_sp_dc() {
            log::info!("SyncWorker: Starting authenticated Spotify library sync...");
            let sp_sync = SpotifyCookieSync::new(sp_dc);

            // Liked songs library
            match sp_sync.fetch_liked_songs().await {
                Ok((liked_pl, liked_tracks)) => {
                    let tracks_count = liked_tracks.len();
                    let _ = self.db.upsert_imported_playlist(&liked_pl);
                    let _ = self.db.replace_playlist_tracks(&liked_pl.id, &liked_tracks);
                    report.playlists_synced += 1;
                    report.total_tracks_synced += tracks_count;
                    log::info!("SyncWorker: Synced Spotify Liked Songs ({} tracks)", tracks_count);
                }
                Err(e) => {
                    let err = format!("Spotify Liked Songs sync error: {:#}", e);
                    log::warn!("{}", err);
                    report.errors.push(err);
                }
            }

            // User playlists & their tracks
            match sp_sync.fetch_user_playlists().await {
                Ok(playlists) => {
                    for pl in playlists {
                        let _ = self.db.upsert_imported_playlist(&pl);
                        report.playlists_synced += 1;

                        match sp_sync.fetch_playlist_tracks(&pl.id).await {
                            Ok(tracks) => {
                                let count = tracks.len();
                                let _ = self.db.replace_playlist_tracks(&pl.id, &tracks);
                                report.total_tracks_synced += count;
                            }
                            Err(e) => {
                                log::warn!("SyncWorker: Failed fetching tracks for {}: {:#}", pl.title, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    let err = format!("Spotify user playlists sync error: {:#}", e);
                    log::warn!("{}", err);
                    report.errors.push(err);
                }
            }
        }

        // 2. YouTube Sync (Refresh previously imported YouTube playlists)
        let yt_importer = YouTubeImporter::new();
        if let Ok(playlists) = self.db.list_imported_playlists() {
            for pl in playlists {
                if pl.platform == "youtube" {
                    log::info!("SyncWorker: Refreshing YouTube playlist: {}", pl.title);
                    match yt_importer.import_playlist(&self.db, &pl.source_url).await {
                        Ok(updated) => {
                            report.playlists_synced += 1;
                            report.total_tracks_synced += updated.track_count as usize;
                        }
                        Err(e) => {
                            let err = format!("YouTube playlist '{}' sync error: {:#}", pl.title, e);
                            log::warn!("{}", err);
                            report.errors.push(err);
                        }
                    }
                }
            }
        }

        report
    }

    /// Spawns an asynchronous background worker task that executes the sync
    /// pass and reports live progress back through the App status message.
    pub fn spawn_background_sync(app: Arc<Mutex<App>>) {
        tokio::spawn(async move {
            {
                let mut guard = app.lock().await;
                guard.set_status_message(
                    "Syncing remote Spotify & YouTube playlists in background...".to_string(),
                    4,
                );
            }

            match SyncWorker::new() {
                Ok(worker) => {
                    let report = worker.run_sync().await;
                    let mut guard = app.lock().await;
                    let msg = if report.playlists_synced > 0 {
                        format!(
                            "\u{2713} Sync complete: {} playlists, {} tracks updated",
                            report.playlists_synced, report.total_tracks_synced
                        )
                    } else if !report.errors.is_empty() {
                        format!("Sync warning: {}", report.errors.join("; "))
                    } else {
                        "Sync complete: No remote Spotify or YouTube credentials configured".to_string()
                    };
                    guard.set_status_message(msg, 5);
                }
                Err(e) => {
                    let mut guard = app.lock().await;
                    guard.set_status_message(format!("Sync error: {:#}", e), 5);
                }
            }
        });
    }

    /// Spawns an asynchronous background import of a Spotify or YouTube URL,
    /// delivering live progress context to the bottom status bar and modal.
    pub async fn import_url_background(app: Arc<Mutex<App>>, url_to_import: String) {
        let trimmed = url_to_import.trim();
        {
            let mut guard = app.lock().await;
            guard.set_status_message(
                format!("Importing playlist: connecting to source for {}...", trimmed),
                15,
            );
            guard.view.url_import_in_progress = true;
            guard.view.url_import_status = Some("Connecting and resolving playlist metadata...".to_string());
        }

        let db = match Database::open_default() {
            Ok(db) => db,
            Err(e) => {
                let mut guard = app.lock().await;
                guard.set_status_message(format!("\u{274c} Import error: {:#}", e), 6);
                guard.view.url_import_status = Some(format!("Error: {:#}", e));
                guard.view.url_import_in_progress = false;
                return;
            }
        };

        // Check Spotify URL / URI
        if let Some(sp) = crate::sources::lossless_relay::spotify_scraper::parse_spotify_url(trimmed) {
            let scraper = crate::sources::lossless_relay::spotify_scraper::SpotifyScraper::new();
            match sp {
                crate::sources::lossless_relay::spotify_scraper::SpotifyResource::Playlist(id) => {
                    {
                        let mut guard = app.lock().await;
                        guard.set_status_message(format!("Fetching Spotify playlist {} metadata...", id), 15);
                        guard.view.url_import_status = Some("Fetching tracks from Spotify...".to_string());
                    }
                    match scraper.fetch_playlist(&id).await {
                        Ok((pl, tracks)) => {
                            let track_count = tracks.len();
                            let title = pl.title.clone();
                            let _ = db.upsert_imported_playlist(&pl);
                            let _ = db.replace_playlist_tracks(&pl.id, &tracks);

                            let resolver = crate::sources::lossless_relay::resolver::Resolver::new(db.clone());
                            let (worker, rx) = crate::sources::lossless_relay::resolver::BatchResolutionWorker::new(resolver.clone());
                            let _ = worker.enqueue_batch(tracks).await;
                            tokio::spawn(crate::sources::lossless_relay::resolver::BatchResolutionWorker::run_loop(resolver, rx));

                            let mut guard = app.lock().await;
                            guard.set_status_message(
                                format!("\u{2713} Imported Spotify playlist '{}' ({} tracks) into library!", title, track_count),
                                6,
                            );
                            guard.view.url_import_status = Some(format!("\u{2713} Imported '{}' ({} tracks)", title, track_count));
                            guard.view.url_import_in_progress = false;
                        }
                        Err(e) => {
                            let mut guard = app.lock().await;
                            guard.set_status_message(format!("\u{274c} Spotify import failed: {:#}", e), 6);
                            guard.view.url_import_status = Some(format!("Error: {:#}", e));
                            guard.view.url_import_in_progress = false;
                        }
                    }
                }
                crate::sources::lossless_relay::spotify_scraper::SpotifyResource::Album(id) => {
                    match scraper.fetch_album(&id).await {
                        Ok((pl, tracks)) => {
                            let track_count = tracks.len();
                            let title = pl.title.clone();
                            let _ = db.upsert_imported_playlist(&pl);
                            let _ = db.replace_playlist_tracks(&pl.id, &tracks);
                            let resolver = crate::sources::lossless_relay::resolver::Resolver::new(db.clone());
                            let (worker, rx) = crate::sources::lossless_relay::resolver::BatchResolutionWorker::new(resolver.clone());
                            let _ = worker.enqueue_batch(tracks).await;
                            tokio::spawn(crate::sources::lossless_relay::resolver::BatchResolutionWorker::run_loop(resolver, rx));

                            let mut guard = app.lock().await;
                            guard.set_status_message(
                                format!("\u{2713} Imported Spotify album '{}' ({} tracks) into library!", title, track_count),
                                6,
                            );
                            guard.view.url_import_status = Some(format!("\u{2713} Imported '{}' ({} tracks)", title, track_count));
                            guard.view.url_import_in_progress = false;
                        }
                        Err(e) => {
                            let mut guard = app.lock().await;
                            guard.set_status_message(format!("\u{274c} Spotify album import failed: {:#}", e), 6);
                            guard.view.url_import_status = Some(format!("Error: {:#}", e));
                            guard.view.url_import_in_progress = false;
                        }
                    }
                }
                crate::sources::lossless_relay::spotify_scraper::SpotifyResource::Track(id) => {
                    match scraper.fetch_track(&id).await {
                        Ok(track) => {
                            let resolver = crate::sources::lossless_relay::resolver::Resolver::new(db.clone());
                            let _ = resolver.resolve_track(&track).await;
                            let mut guard = app.lock().await;
                            guard.set_status_message(
                                format!("\u{2713} Imported Spotify track '{}' into library!", track.clean_title),
                                6,
                            );
                            guard.view.url_import_status = Some(format!("\u{2713} Imported track '{}'", track.clean_title));
                            guard.view.url_import_in_progress = false;
                        }
                        Err(e) => {
                            let mut guard = app.lock().await;
                            guard.set_status_message(format!("\u{274c} Spotify track import failed: {:#}", e), 6);
                            guard.view.url_import_status = Some(format!("Error: {:#}", e));
                            guard.view.url_import_in_progress = false;
                        }
                    }
                }
            }
            return;
        }

        // Check YouTube URL / URI
        if let Some(yt) = crate::sources::lossless_relay::youtube_importer::parse_youtube_url(trimmed) {
            let importer = crate::sources::lossless_relay::youtube_importer::YouTubeImporter::new();
            match yt {
                crate::sources::lossless_relay::youtube_importer::YouTubeResource::Playlist(id) => {
                    {
                        let mut guard = app.lock().await;
                        guard.set_status_message(format!("Fetching YouTube playlist {} via InnerTube/yt-dlp...", id), 15);
                        guard.view.url_import_status = Some("Querying YouTube for tracks...".to_string());
                    }
                    match importer.fetch_playlist(&id).await {
                        Ok((pl, tracks)) => {
                            let track_count = tracks.len();
                            let title = pl.title.clone();
                            let _ = db.upsert_imported_playlist(&pl);
                            let _ = db.replace_playlist_tracks(&pl.id, &tracks);

                            let resolver = crate::sources::lossless_relay::resolver::Resolver::new(db.clone());
                            let (worker, rx) = crate::sources::lossless_relay::resolver::BatchResolutionWorker::new(resolver.clone());
                            let _ = worker.enqueue_batch(tracks).await;
                            tokio::spawn(crate::sources::lossless_relay::resolver::BatchResolutionWorker::run_loop(resolver, rx));

                            let mut guard = app.lock().await;
                            guard.set_status_message(
                                format!("\u{2713} Imported YouTube playlist '{}' ({} tracks) into library!", title, track_count),
                                6,
                            );
                            guard.view.url_import_status = Some(format!("\u{2713} Imported '{}' ({} tracks)", title, track_count));
                            guard.view.url_import_in_progress = false;
                        }
                        Err(e) => {
                            let mut guard = app.lock().await;
                            guard.set_status_message(format!("\u{274c} YouTube import failed: {:#}", e), 6);
                            guard.view.url_import_status = Some(format!("Error: {:#}", e));
                            guard.view.url_import_in_progress = false;
                        }
                    }
                }
                crate::sources::lossless_relay::youtube_importer::YouTubeResource::Video(vid) => {
                    let mut guard = app.lock().await;
                    guard.set_status_message(format!("YouTube single video: https://youtu.be/{}", vid), 5);
                    guard.view.url_import_status = Some(format!("YouTube video ID: {}", vid));
                    guard.view.url_import_in_progress = false;
                }
            }
            return;
        }

        // If neither parsed:
        let mut guard = app.lock().await;
        guard.set_status_message(format!("\u{274c} Invalid or unsupported playlist URL: {}", trimmed), 5);
        guard.view.url_import_status = Some("Invalid URL. Supported: Spotify & YouTube playlist/track links.".to_string());
        guard.view.url_import_in_progress = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sync_worker_in_memory_run() {
        let db = Database::open_in_memory().unwrap();
        let worker = SyncWorker::with_db(db);
        // In test environment without tokens, worker runs without crashing
        let report = worker.run_sync().await;
        assert_eq!(report.playlists_synced, 0);
        assert_eq!(report.total_tracks_synced, 0);
    }
}
