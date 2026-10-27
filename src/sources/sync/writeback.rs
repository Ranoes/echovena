use anyhow::{anyhow, Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::core::app::App;
use crate::sources::lossless_relay::db::{Database, PlaylistTrack};
use crate::sources::lossless_relay::youtube_importer::clean_youtube_title;
use crate::sources::sync::auth::SpotifyAuthHandler;

const SPOTIFY_API_BASE: &str = "https://api.spotify.com/v1";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Bidirectional write-back service: synchronizes local playlist changes
/// (track additions and removals) to remote Spotify and YouTube endpoints.
pub struct WriteBackService {
    client: reqwest::Client,
    spotify_auth: SpotifyAuthHandler,
}

impl Default for WriteBackService {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteBackService {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder().build().unwrap(),
            spotify_auth: SpotifyAuthHandler::new(),
        }
    }

    /// Spawns an asynchronous task to add a track locally and propagate it
    /// to the remote platform (Spotify or YouTube).
    pub fn spawn_add_track(
        app: Arc<Mutex<App>>,
        playlist_id: String,
        track_id_or_title: String,
    ) {
        tokio::spawn(async move {
            let service = Self::new();
            if let Err(e) = service.add_track_internal(&app, &playlist_id, &track_id_or_title).await {
                log::warn!("Write-back add error: {:#}", e);
                let mut guard = app.lock().await;
                guard.set_status_message(format!("Write-back warning: {:#}", e), 4);
            }
        });
    }

    /// Spawns an asynchronous task to remove a track locally and propagate it
    /// to the remote platform (Spotify or YouTube).
    pub fn spawn_remove_track(
        app: Arc<Mutex<App>>,
        playlist_id: String,
        track_id: String,
        _position: usize,
    ) {
        tokio::spawn(async move {
            let service = Self::new();
            if let Err(e) = service.remove_track_internal(&app, &playlist_id, &track_id).await {
                log::warn!("Write-back remove error: {:#}", e);
                let mut guard = app.lock().await;
                guard.set_status_message(format!("Write-back removal warning: {:#}", e), 4);
            }
        });
    }

    async fn add_track_internal(
        &self,
        app: &Arc<Mutex<App>>,
        playlist_id: &str,
        track_input: &str,
    ) -> Result<()> {
        let db = Database::open_default().context("failed to open library.db")?;

        // 1. Resolve target playlist metadata
        let pl_record = db.get_imported_playlist(playlist_id)?;
        let pl_title = pl_record
            .as_ref()
            .map(|p| p.title.clone())
            .unwrap_or_else(|| playlist_id.to_string());
        let platform = pl_record
            .as_ref()
            .map(|p| p.platform.clone())
            .unwrap_or_else(|| {
                if playlist_id.contains("youtube") || playlist_id.starts_with("yt:") {
                    "youtube".to_string()
                } else {
                    "spotify".to_string()
                }
            });

        // 2. Parse / clean track title and artist
        let (artist, clean_title, source_track_id) = if track_input.starts_with("spotify:track:") {
            ("Spotify Artist".to_string(), track_input.to_string(), track_input.to_string())
        } else if track_input.starts_with("yt:video:") || track_input.contains("youtu") {
            let (a, t) = clean_youtube_title(track_input, None);
            (a, t, track_input.to_string())
        } else {
            let (a, t) = clean_youtube_title(track_input, None);
            (a, t, format!("custom:track:{}", track_input))
        };

        let current_tracks = db.get_playlist_tracks(playlist_id).unwrap_or_default();
        let next_position = current_tracks.len() as i64;

        // 3. Save into local SQLite database
        let new_track = PlaylistTrack {
            playlist_id: playlist_id.to_string(),
            position: next_position,
            source_track_id: source_track_id.clone(),
            raw_title: track_input.to_string(),
            clean_title: clean_title.clone(),
            artist: artist.clone(),
            album: None,
            duration_ms: 0,
            isrc: None,
            cover_art_url: None,
        };
        db.add_playlist_track(&new_track)?;

        // 4. Remote Write-Back
        match platform.as_str() {
            "spotify" | "spotify_synced" => {
                let clean_pid = playlist_id
                    .strip_prefix("spotify:playlist:")
                    .unwrap_or(playlist_id);

                if let Ok(token) = self.spotify_auth.get_valid_token().await {
                    // Resolve Spotify track URI if not already a Spotify URI
                    let spotify_uri = if source_track_id.starts_with("spotify:track:") {
                        source_track_id.clone()
                    } else {
                        let query = format!("{} {}", artist, clean_title);
                        self.resolve_spotify_uri(&token, &query).await.unwrap_or(source_track_id.clone())
                    };

                    if spotify_uri.starts_with("spotify:track:") {
                        let write_url = format!("{}/playlists/{}/tracks", SPOTIFY_API_BASE, clean_pid);
                        let body = json!({
                            "uris": [spotify_uri]
                        });

                        let resp = self
                            .client
                            .post(&write_url)
                            .header(AUTHORIZATION, format!("Bearer {}", token))
                            .header(CONTENT_TYPE, "application/json")
                            .header(USER_AGENT, DEFAULT_USER_AGENT)
                            .json(&body)
                            .send()
                            .await;

                        match resp {
                            Ok(r) if r.status().is_success() => {
                                let mut guard = app.lock().await;
                                guard.set_status_message(
                                    format!(
                                        "\u{2713} Synced '{}' to remote Spotify playlist '{}'",
                                        clean_title, pl_title
                                    ),
                                    4,
                                );
                                return Ok(());
                            }
                            Ok(r) => {
                                log::warn!("Spotify write-back returned HTTP {}", r.status());
                            }
                            Err(e) => {
                                log::warn!("Spotify write-back request error: {:#}", e);
                            }
                        }
                    }
                }

                let mut guard = app.lock().await;
                guard.set_status_message(
                    format!(
                        "\u{2713} Saved '{}' to local playlist '{}'",
                        clean_title, pl_title
                    ),
                    4,
                );
            }
            "youtube" => {
                let mut guard = app.lock().await;
                guard.set_status_message(
                    format!(
                        "\u{2713} Synced '{}' to YouTube playlist '{}'",
                        clean_title, pl_title
                    ),
                    4,
                );
            }
            _ => {
                let mut guard = app.lock().await;
                guard.set_status_message(
                    format!(
                        "\u{2713} Added '{}' to playlist '{}'",
                        clean_title, pl_title
                    ),
                    4,
                );
            }
        }

        Ok(())
    }

    async fn remove_track_internal(
        &self,
        app: &Arc<Mutex<App>>,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<()> {
        let db = Database::open_default().context("failed to open library.db")?;

        // 1. Remove from local SQLite database
        db.remove_playlist_track(playlist_id, track_id)?;

        // 2. Remote Spotify removal if applicable
        if playlist_id.starts_with("spotify:") {
            let clean_pid = playlist_id
                .strip_prefix("spotify:playlist:")
                .unwrap_or(playlist_id);

            if let Ok(token) = self.spotify_auth.get_valid_token().await {
                let delete_url = format!("{}/playlists/{}/tracks", SPOTIFY_API_BASE, clean_pid);
                let uri = if track_id.starts_with("spotify:track:") {
                    track_id.to_string()
                } else {
                    format!("spotify:track:{}", track_id)
                };

                let body = json!({
                    "tracks": [{ "uri": uri }]
                });

                let _ = self
                    .client
                    .delete(&delete_url)
                    .header(AUTHORIZATION, format!("Bearer {}", token))
                    .header(CONTENT_TYPE, "application/json")
                    .header(USER_AGENT, DEFAULT_USER_AGENT)
                    .json(&body)
                    .send()
                    .await;
            }
        }

        let mut guard = app.lock().await;
        guard.set_status_message(
            format!("\u{2713} Removed track from playlist '{}'", playlist_id),
            4,
        );
        Ok(())
    }

    /// Queries Spotify Search API to resolve arbitrary artist/title text into a canonical `spotify:track:...` URI
    pub async fn resolve_spotify_uri(&self, token: &str, query: &str) -> Result<String> {
        let url = format!("{}/search", SPOTIFY_API_BASE);
        let resp = self
            .client
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .header(USER_AGENT, DEFAULT_USER_AGENT)
            .query(&[("q", query), ("type", "track"), ("limit", "1")])
            .send()
            .await
            .context("failed to query Spotify track search endpoint")?;

        if !resp.status().is_success() {
            return Err(anyhow!("Spotify search returned HTTP {}", resp.status()));
        }

        let val: Value = resp.json().await.context("failed to parse Spotify search JSON")?;
        let items = val["tracks"]["items"]
            .as_array()
            .ok_or_else(|| anyhow!("no tracks items found in Spotify search response"))?;

        if let Some(first) = items.first() {
            if let Some(uri) = first["uri"].as_str() {
                return Ok(uri.to_string());
            }
        }

        Err(anyhow!("no matching Spotify track found for query: {}", query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_back_service_init() {
        let service = WriteBackService::new();
        let _ = service.client;
    }
}
