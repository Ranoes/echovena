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
