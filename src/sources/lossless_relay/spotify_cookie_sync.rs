use crate::sources::lossless_relay::db::{Database, ImportedPlaylist, PlaylistTrack};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, COOKIE, USER_AGENT};
use ring::aead::{
    Aad, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey, UnboundKey, CHACHA20_POLY1305,
    NONCE_LEN,
};
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use std::fs;
use std::num::NonZeroU32;
use std::path::PathBuf;

const SPOTIFY_WEB_TOKEN_URL: &str =
    "https://open.spotify.com/get_access_token?reason=transport&productType=web_player";
const SPOTIFY_API_BASE: &str = "https://api.spotify.com/v1";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const SALT: &[u8] = b"EchoVena-sp_dc-credentials-salt-v1";

/// Resolve path to encrypted credentials file: ~/.local/share/echovena/credentials.enc
pub fn credentials_path() -> Option<PathBuf> {
    crate::core::paths::app_data_dir().map(|d| d.join("credentials.enc"))
}

struct OneNonce(Option<[u8; NONCE_LEN]>);

impl NonceSequence for OneNonce {
    fn advance(&mut self) -> Result<Nonce, ring::error::Unspecified> {
        let bytes = self.0.take().ok_or(ring::error::Unspecified)?;
        Nonce::try_assume_unique_for_key(&bytes)
    }
}

/// Derives a deterministic 256-bit ChaCha20-Poly1305 key using system username and app salt
fn derive_storage_key() -> [u8; 32] {
    let seed = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "echovena_default_user".to_string());

    let mut key = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(10_000).unwrap(),
        SALT,
        seed.as_bytes(),
        &mut key,
    );
    key
}

/// Encrypts an `sp_dc` cookie string and saves it to ~/.local/share/echovena/credentials.enc
pub fn save_encrypted_sp_dc(sp_dc: &str) -> Result<()> {
    let path = credentials_path().context("failed to resolve credentials.enc path")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating directory {}", parent.display()))?;
    }

    let key_bytes = derive_storage_key();
    let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, &key_bytes)
        .map_err(|_| anyhow!("failed initializing chacha20 unbound key"))?;

    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow!("failed generating random nonce"))?;

    let nonce_seq = OneNonce(Some(nonce_bytes));
    let mut sealing_key = SealingKey::new(unbound_key, nonce_seq);

    let mut in_out = sp_dc.as_bytes().to_vec();
    sealing_key
        .seal_in_place_append_tag(Aad::empty(), &mut in_out)
        .map_err(|_| anyhow!("failed sealing sp_dc ciphertext"))?;

    let mut payload = nonce_bytes.to_vec();
    payload.extend_from_slice(&in_out);

    fs::write(&path, payload)
        .with_context(|| format!("failed writing credentials to {}", path.display()))?;
    Ok(())
}

/// Loads and decrypts the `sp_dc` cookie from ~/.local/share/echovena/credentials.enc
pub fn load_encrypted_sp_dc() -> Result<Option<String>> {
    let Some(path) = credentials_path() else {
        return Ok(None);
    };

    if !path.exists() {
        return Ok(None);
    }

    let payload = fs::read(&path)
        .with_context(|| format!("failed reading credentials from {}", path.display()))?;

    if payload.len() <= NONCE_LEN {
        return Ok(None);
    }

    let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
    let mut nonce_arr = [0u8; NONCE_LEN];
    nonce_arr.copy_from_slice(nonce_bytes);

    let key_bytes = derive_storage_key();
    let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, &key_bytes)
        .map_err(|_| anyhow!("failed initializing chacha20 unbound key"))?;

    let nonce_seq = OneNonce(Some(nonce_arr));
    let mut opening_key = OpeningKey::new(unbound_key, nonce_seq);

    let mut in_out = ciphertext.to_vec();
    let plaintext = opening_key
        .open_in_place(Aad::empty(), &mut in_out)
        .map_err(|_| anyhow!("failed decrypting credentials ciphertext"))?;

    let sp_dc = String::from_utf8(plaintext.to_vec())
        .context("credentials plaintext is not valid utf-8")?;

    Ok(Some(sp_dc))
}

/// Resolves the active `sp_dc` cookie either from env variable or from local encrypted storage
pub fn get_active_sp_dc(env_name: &str) -> Option<String> {
    if let Ok(val) = std::env::var(env_name) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    load_encrypted_sp_dc().ok().flatten()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpotifyAuthToken {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "accessTokenExpirationTimestampMs")]
    pub expiration_timestamp_ms: i64,
    #[serde(default, rename = "isAnonymous")]
    pub is_anonymous: bool,
}

#[derive(Clone)]
pub struct SpotifyCookieSync {
    client: reqwest::Client,
    sp_dc: String,
}

impl SpotifyCookieSync {
    pub fn new(sp_dc: String) -> Self {
        Self {
            client: reqwest::Client::builder().build().unwrap(),
            sp_dc,
        }
    }

    /// Exchanges the `sp_dc` cookie for an authenticated Spotify bearer token
    pub async fn exchange_token(&self) -> Result<SpotifyAuthToken> {
        let cookie_header = format!("sp_dc={}", self.sp_dc);

        let resp = self
            .client
            .get(SPOTIFY_WEB_TOKEN_URL)
            .header(COOKIE, cookie_header)
            .header(USER_AGENT, DEFAULT_USER_AGENT)
            .send()
            .await
            .context("failed to exchange sp_dc for spotify bearer token")?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "spotify token exchange returned HTTP {}",
                resp.status()
            ));
        }

        let token: SpotifyAuthToken = resp
            .json()
            .await
            .context("failed to parse spotify auth token response")?;

        if token.is_anonymous {
            return Err(anyhow!(
                "sp_dc cookie was rejected or expired by Spotify (returned anonymous token)"
            ));
        }

        Ok(token)
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

    /// Fetches the user's Liked Songs library
    pub async fn fetch_liked_songs(&self) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let token = self.exchange_token().await?;
        let playlist_id = "spotify:personal:liked_songs".to_string();

        let mut tracks = Vec::new();
        let mut offset = 0;
        let limit = 50;
        let mut total = 0;

        loop {
            let url = format!("{}/me/tracks?limit={}&offset={}", SPOTIFY_API_BASE, limit, offset);

            let resp = self
                .client
                .get(&url)
                .headers(Self::auth_headers(&token.access_token))
                .send()
                .await
                .context("failed querying /me/tracks endpoint")?;

            if !resp.status().is_success() {
                break;
            }

            let page: SavedTracksPage = resp
                .json()
                .await
                .context("failed parsing /me/tracks page json")?;

            total = page.total;
            let page_items_count = page.items.len();

            for item in page.items {
                let track = item.track;
                if track.id.is_empty() {
                    continue;
                }

                let artists = track
                    .artists
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                let cover_art = track
                    .album
                    .as_ref()
                    .and_then(|a| a.images.first().map(|img| img.url.clone()));
                let album_name = track.album.as_ref().map(|a| a.name.clone());
                let isrc = track.external_ids.and_then(|ids| ids.isrc);

                tracks.push(PlaylistTrack {
                    playlist_id: playlist_id.clone(),
                    position: tracks.len() as i64,
                    source_track_id: format!("spotify:track:{}", track.id),
                    raw_title: track.name.clone(),
                    clean_title: track.name,
                    artist: artists,
                    album: album_name,
                    duration_ms: track.duration_ms,
                    isrc,
                    cover_art_url: cover_art,
                });
            }

            offset += page_items_count;
            if page_items_count < limit || offset >= total {
                break;
            }
        }

        let playlist_record = ImportedPlaylist {
            id: playlist_id,
            platform: "spotify".to_string(),
            title: "Liked Songs".to_string(),
            author: Some("Personal Library".to_string()),
            source_url: "https://open.spotify.com/collection/tracks".to_string(),
            cover_art_url: None,
            snapshot_id: None,
            track_count: total as i64,
            last_synced_at: Utc::now().timestamp(),
        };

        Ok((playlist_record, tracks))
    }

    /// Fetches all playlists owned or followed by the user
    pub async fn fetch_user_playlists(&self) -> Result<Vec<ImportedPlaylist>> {
        let token = self.exchange_token().await?;
        let mut playlists = Vec::new();
        let mut offset = 0;
        let limit = 50;

        loop {
            let url = format!("{}/me/playlists?limit={}&offset={}", SPOTIFY_API_BASE, limit, offset);

            let resp = self
                .client
                .get(&url)
                .headers(Self::auth_headers(&token.access_token))
                .send()
                .await
                .context("failed querying /me/playlists endpoint")?;

            if !resp.status().is_success() {
                break;
            }

            let page: UserPlaylistsPage = resp
                .json()
                .await
                .context("failed parsing /me/playlists page json")?;

            let page_items_count = page.items.len();
            for pl in page.items {
                let cover_art = pl.images.first().map(|img| img.url.clone());
                let author = pl.owner.map(|o| o.display_name.unwrap_or(o.id));

                playlists.push(ImportedPlaylist {
                    id: format!("spotify:playlist:{}", pl.id),
                    platform: "spotify".to_string(),
                    title: pl.name,
                    author,
                    source_url: format!("https://open.spotify.com/playlist/{}", pl.id),
                    cover_art_url: cover_art,
                    snapshot_id: Some(pl.snapshot_id),
                    track_count: pl.tracks.total as i64,
                    last_synced_at: Utc::now().timestamp(),
                });
            }

            offset += page_items_count;
            if page_items_count < limit || offset >= page.total {
                break;
            }
        }

        Ok(playlists)
    }

    /// Synchronizes the personal library (Liked Songs) into the local SQLite database
    pub async fn sync_to_db(&self, db: &Database) -> Result<(usize, usize)> {
        let (liked_playlist, liked_tracks) = self.fetch_liked_songs().await?;
        let track_count = liked_tracks.len();

        db.upsert_imported_playlist(&liked_playlist)?;
        db.replace_playlist_tracks(&liked_playlist.id, &liked_tracks)?;

        let playlists = self.fetch_user_playlists().await.unwrap_or_default();
        let pl_count = playlists.len();
        for pl in &playlists {
            let _ = db.upsert_imported_playlist(pl);
        }

        Ok((1 + pl_count, track_count))
    }
}

// Internal DTOs

#[derive(Debug, Deserialize)]
struct SavedTrackItem {
    track: RawTrack,
}

#[derive(Debug, Deserialize)]
struct RawArtist {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawImage {
    url: String,
}

#[derive(Debug, Deserialize)]
struct RawAlbum {
    name: String,
    #[serde(default)]
    images: Vec<RawImage>,
}

#[derive(Debug, Deserialize)]
struct RawExternalIds {
    isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTrack {
    id: String,
    name: String,
    duration_ms: i64,
    #[serde(default)]
    artists: Vec<RawArtist>,
    album: Option<RawAlbum>,
    external_ids: Option<RawExternalIds>,
}

#[derive(Debug, Deserialize)]
struct SavedTracksPage {
    total: usize,
    #[serde(default)]
    items: Vec<SavedTrackItem>,
}

#[derive(Debug, Deserialize)]
struct RawOwner {
    id: String,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTracksTotal {
    total: usize,
}

#[derive(Debug, Deserialize)]
struct RawUserPlaylist {
    id: String,
    name: String,
    snapshot_id: String,
    owner: Option<RawOwner>,
    #[serde(default)]
    images: Vec<RawImage>,
    tracks: RawTracksTotal,
}

#[derive(Debug, Deserialize)]
struct UserPlaylistsPage {
    total: usize,
    #[serde(default)]
    items: Vec<RawUserPlaylist>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_encryption_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("credentials.enc");

        let test_sp_dc = "AQD_example_spotify_cookie_token_1234567890abcdef";

        // Derive key
        let key_bytes = derive_storage_key();
        let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, &key_bytes).unwrap();

        // Encrypt
        let rng = SystemRandom::new();
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng.fill(&mut nonce_bytes).unwrap();

        let nonce_seq = OneNonce(Some(nonce_bytes));
        let mut sealing_key = SealingKey::new(unbound_key, nonce_seq);

        let mut in_out = test_sp_dc.as_bytes().to_vec();
        sealing_key
            .seal_in_place_append_tag(Aad::empty(), &mut in_out)
            .unwrap();

        let mut payload = nonce_bytes.to_vec();
        payload.extend_from_slice(&in_out);
        fs::write(&test_file, payload).unwrap();

        // Decrypt
        let read_payload = fs::read(&test_file).unwrap();
        let (read_nonce, ciphertext) = read_payload.split_at(NONCE_LEN);
        let mut read_nonce_arr = [0u8; NONCE_LEN];
        read_nonce_arr.copy_from_slice(read_nonce);

        let read_unbound = UnboundKey::new(&CHACHA20_POLY1305, &key_bytes).unwrap();
        let read_seq = OneNonce(Some(read_nonce_arr));
        let mut opening_key = OpeningKey::new(read_unbound, read_seq);

        let mut decrypt_buf = ciphertext.to_vec();
        let decrypted = opening_key
            .open_in_place(Aad::empty(), &mut decrypt_buf)
            .unwrap();

        let decrypted_str = String::from_utf8(decrypted.to_vec()).unwrap();
        assert_eq!(decrypted_str, test_sp_dc);
    }
}
