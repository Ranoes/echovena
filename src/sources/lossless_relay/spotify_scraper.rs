use crate::sources::lossless_relay::db::{ImportedPlaylist, PlaylistTrack};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

const SPOTIFY_WEB_TOKEN_URL: &str =
    "https://open.spotify.com/get_access_token?reason=transport&productType=web_player";
const SPOTIFY_API_BASE: &str = "https://api.spotify.com/v1";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotifyResource {
    Track(String),
    Album(String),
    Playlist(String),
}

/// Parses Spotify web URLs or URIs into a canonical resource type and ID
pub fn parse_spotify_url(input: &str) -> Option<SpotifyResource> {
    let trimmed = input.trim();

    // URI format: spotify:track:..., spotify:album:..., spotify:playlist:...
    if let Some(rest) = trimmed.strip_prefix("spotify:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() >= 2 {
            return match parts[0] {
                "track" => Some(SpotifyResource::Track(parts[1].to_string())),
                "album" => Some(SpotifyResource::Album(parts[1].to_string())),
                "playlist" => Some(SpotifyResource::Playlist(parts[1].to_string())),
                _ => None,
            };
        }
    }

    // Web URL format: https://open.spotify.com/{type}/{id}...
    if trimmed.contains("open.spotify.com/") {
        let url_path = trimmed
            .split('?')
            .next()?
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_start_matches("open.spotify.com/");

        let parts: Vec<&str> = url_path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            let id = parts[1].to_string();
            return match parts[0] {
                "track" => Some(SpotifyResource::Track(id)),
                "album" => Some(SpotifyResource::Album(id)),
                "playlist" => Some(SpotifyResource::Playlist(id)),
                _ => None,
            };
        }
    }

    None
}

#[derive(Debug, Clone, Deserialize)]
struct WebTokenResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "accessTokenExpirationTimestampMs")]
    expiration_timestamp_ms: i64,
}

#[derive(Clone)]
pub struct SpotifyScraper {
    client: reqwest::Client,
    cached_token: Arc<Mutex<Option<(String, i64)>>>,
}

impl Default for SpotifyScraper {
    fn default() -> Self {
        Self::new()
    }
}

impl SpotifyScraper {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder().build().unwrap(),
            cached_token: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            client,
            cached_token: Arc::new(Mutex::new(None)),
        }
    }

    /// Fetches a valid anonymous bearer token, using cached token if not expired
    pub async fn get_token(&self) -> Result<String> {
        let mut cache = self.cached_token.lock().await;
        let now_ms = Utc::now().timestamp_millis();

        if let Some((token, expires_at)) = &*cache {
            if now_ms + 60_000 < *expires_at {
                return Ok(token.clone());
            }
        }

        let resp = self
            .client
            .get(SPOTIFY_WEB_TOKEN_URL)
            .header(USER_AGENT, DEFAULT_USER_AGENT)
            .send()
            .await
            .context("failed to request spotify anonymous token")?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "spotify web token endpoint returned HTTP {}",
                resp.status()
            ));
        }

        let token_data: WebTokenResponse = resp
            .json()
            .await
            .context("failed to parse spotify web token response")?;

        *cache = Some((
            token_data.access_token.clone(),
            token_data.expiration_timestamp_ms,
        ));

        Ok(token_data.access_token)
    }

    fn auth_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers
    }

    /// Fetches a single track's full metadata including ISRC
    pub async fn fetch_track(&self, track_id: &str) -> Result<PlaylistTrack> {
        let token = self.get_token().await?;
        let url = format!("{}/tracks/{}", SPOTIFY_API_BASE, track_id);

        let resp = self
            .client
            .get(&url)
            .headers(Self::auth_headers(&token))
            .send()
            .await
            .context("failed to query spotify track endpoint")?;

        if !resp.status().is_success() {
            return Err(anyhow!("spotify track api returned HTTP {}", resp.status()));
        }

        let raw: SpotifyTrackApi = resp.json().await.context("failed to parse track json")?;

        let isrc = raw.external_ids.and_then(|ids| ids.isrc);
        let artist_names = raw
            .artists
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let cover_art = raw
            .album
            .as_ref()
            .and_then(|a| a.images.first().map(|img| img.url.clone()));
        let album_name = raw.album.as_ref().map(|a| a.name.clone());

        Ok(PlaylistTrack {
            playlist_id: "direct_track".to_string(),
            position: 0,
            source_track_id: format!("spotify:track:{}", track_id),
            raw_title: raw.name.clone(),
            clean_title: raw.name,
            artist: artist_names,
            album: album_name,
            duration_ms: raw.duration_ms,
            isrc,
            cover_art_url: cover_art,
        })
    }

    /// Fetches an album and its full list of tracks
    pub async fn fetch_album(
        &self,
        album_id: &str,
    ) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let token = self.get_token().await?;
        let url = format!("{}/albums/{}", SPOTIFY_API_BASE, album_id);

        let resp = self
            .client
            .get(&url)
            .headers(Self::auth_headers(&token))
            .send()
            .await
            .context("failed to query spotify album endpoint")?;

        if !resp.status().is_success() {
            return Err(anyhow!("spotify album api returned HTTP {}", resp.status()));
        }

        let album_raw: SpotifyAlbumApi = resp.json().await.context("failed to parse album json")?;

        let cover_art = album_raw.images.first().map(|img| img.url.clone());
        let artist_name = album_raw
            .artists
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
            .join(", ");

        let playlist_record = ImportedPlaylist {
            id: format!("spotify:album:{}", album_id),
            platform: "spotify".to_string(),
            title: album_raw.name.clone(),
            author: Some(artist_name.clone()),
            source_url: format!("https://open.spotify.com/album/{}", album_id),
            cover_art_url: cover_art.clone(),
            snapshot_id: None,
            track_count: album_raw.total_tracks as i64,
            last_synced_at: Utc::now().timestamp(),
        };

        let mut tracks = Vec::new();
        let mut offset = 0;
        let limit = 50;

        loop {
            let page_url = format!(
                "{}/albums/{}/tracks?limit={}&offset={}",
                SPOTIFY_API_BASE, album_id, limit, offset
            );

            let page_resp = self
                .client
                .get(&page_url)
                .headers(Self::auth_headers(&token))
                .send()
                .await
                .context("failed to query album tracks page")?;

            if !page_resp.status().is_success() {
                break;
            }

            let page: SpotifyTrackPage = page_resp
                .json()
                .await
                .context("failed to parse album track page json")?;

            let page_items_count = page.items.len();
            for item in page.items {
                let artists = item
                    .artists
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");

                tracks.push(PlaylistTrack {
                    playlist_id: playlist_record.id.clone(),
                    position: tracks.len() as i64,
                    source_track_id: format!("spotify:track:{}", item.id),
                    raw_title: item.name.clone(),
                    clean_title: item.name,
                    artist: artists,
                    album: Some(album_raw.name.clone()),
                    duration_ms: item.duration_ms,
                    isrc: None,
                    cover_art_url: cover_art.clone(),
                });
            }

            offset += page_items_count;
            if page_items_count < limit || offset >= page.total {
                break;
            }
        }

        Ok((playlist_record, tracks))
    }

    /// Fetches a public playlist and all its tracks with pagination (100 tracks per page)
    pub async fn fetch_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let token = self.get_token().await?;
        let url = format!("{}/playlists/{}", SPOTIFY_API_BASE, playlist_id);

        let resp = self
            .client
            .get(&url)
            .headers(Self::auth_headers(&token))
            .send()
            .await
            .context("failed to query spotify playlist endpoint")?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "spotify playlist api returned HTTP {}",
                resp.status()
            ));
        }

        let pl_raw: SpotifyPlaylistApi =
            resp.json().await.context("failed to parse playlist json")?;

        let cover_art = pl_raw.images.first().map(|img| img.url.clone());
        let author = pl_raw.owner.map(|o| o.display_name.unwrap_or(o.id));

        let playlist_record = ImportedPlaylist {
            id: format!("spotify:playlist:{}", playlist_id),
            platform: "spotify".to_string(),
            title: pl_raw.name,
            author,
            source_url: format!("https://open.spotify.com/playlist/{}", playlist_id),
            cover_art_url: cover_art,
            snapshot_id: Some(pl_raw.snapshot_id),
            track_count: pl_raw.tracks.total as i64,
            last_synced_at: Utc::now().timestamp(),
        };

        let mut tracks = Vec::new();
        let mut offset = 0;
        let limit = 100;

        loop {
            let page_url = format!(
                "{}/playlists/{}/tracks?limit={}&offset={}",
                SPOTIFY_API_BASE, playlist_id, limit, offset
            );

            let page_resp = self
                .client
                .get(&page_url)
                .headers(Self::auth_headers(&token))
                .send()
                .await
                .context("failed to query playlist tracks page")?;

            if !page_resp.status().is_success() {
                break;
            }

            let page: SpotifyPlaylistTrackPage = page_resp
                .json()
                .await
                .context("failed to parse playlist track page json")?;

            let page_items_count = page.items.len();
            for item in page.items {
                if let Some(track) = item.track {
                    if track.id.is_empty() {
                        continue;
                    }

                    let artists = track
                        .artists
                        .iter()
                        .map(|a| a.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let track_cover = track
                        .album
                        .as_ref()
                        .and_then(|a| a.images.first().map(|img| img.url.clone()));
                    let album_name = track.album.as_ref().map(|a| a.name.clone());
                    let isrc = track.external_ids.and_then(|ids| ids.isrc);

                    tracks.push(PlaylistTrack {
                        playlist_id: playlist_record.id.clone(),
                        position: tracks.len() as i64,
                        source_track_id: format!("spotify:track:{}", track.id),
                        raw_title: track.name.clone(),
                        clean_title: track.name,
                        artist: artists,
                        album: album_name,
                        duration_ms: track.duration_ms,
                        isrc,
                        cover_art_url: track_cover,
                    });
                }
            }

            offset += page_items_count;
            if page_items_count < limit || offset >= page.total {
                break;
            }
        }

        Ok((playlist_record, tracks))
    }
}

// Internal Spotify API DTOs

#[derive(Debug, Deserialize)]
struct SpotifyArtist {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SpotifyImage {
    url: String,
}

#[derive(Debug, Deserialize)]
struct SpotifyExternalIds {
    isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyAlbumRef {
    name: String,
    #[serde(default)]
    images: Vec<SpotifyImage>,
}

#[derive(Debug, Deserialize)]
struct SpotifyTrackApi {
    id: String,
    name: String,
    duration_ms: i64,
    #[serde(default)]
    artists: Vec<SpotifyArtist>,
    album: Option<SpotifyAlbumRef>,
    external_ids: Option<SpotifyExternalIds>,
}

#[derive(Debug, Deserialize)]
struct SpotifyAlbumApi {
    name: String,
    total_tracks: usize,
    #[serde(default)]
    artists: Vec<SpotifyArtist>,
    #[serde(default)]
    images: Vec<SpotifyImage>,
}

#[derive(Debug, Deserialize)]
struct SpotifyTrackPage {
    total: usize,
    #[serde(default)]
    items: Vec<SpotifyTrackApi>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPlaylistOwner {
    id: String,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPlaylistTrackItem {
    track: Option<SpotifyTrackApi>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPlaylistTrackPage {
    total: usize,
    #[serde(default)]
    items: Vec<SpotifyPlaylistTrackItem>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPlaylistApi {
    name: String,
    snapshot_id: String,
    owner: Option<SpotifyPlaylistOwner>,
    #[serde(default)]
    images: Vec<SpotifyImage>,
    tracks: SpotifyTracksSummary,
}

#[derive(Debug, Deserialize)]
struct SpotifyTracksSummary {
    total: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_spotify_urls_and_uris() {
        assert_eq!(
            parse_spotify_url("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=abc"),
            Some(SpotifyResource::Track("4cOdK2wGLETKBW3PvgPWqT".to_string()))
        );

        assert_eq!(
            parse_spotify_url("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M"),
            Some(SpotifyResource::Playlist(
                "37i9dQZF1DXcBWIGoYBM5M".to_string()
            ))
        );

        assert_eq!(
            parse_spotify_url("https://open.spotify.com/album/4m2880jivSbbyEGAKfITCa"),
            Some(SpotifyResource::Album("4m2880jivSbbyEGAKfITCa".to_string()))
        );

        assert_eq!(
            parse_spotify_url("spotify:track:4cOdK2wGLETKBW3PvgPWqT"),
            Some(SpotifyResource::Track("4cOdK2wGLETKBW3PvgPWqT".to_string()))
        );

        assert_eq!(
            parse_spotify_url("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
            Some(SpotifyResource::Playlist(
                "37i9dQZF1DXcBWIGoYBM5M".to_string()
            ))
        );

        assert_eq!(parse_spotify_url("https://youtube.com/watch?v=123"), None);
    }
}
