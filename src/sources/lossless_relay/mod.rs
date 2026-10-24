pub mod archiver;
pub mod db;
pub mod hifi_client;
pub mod qobuz_client;
pub mod resolver;
pub mod ring_buffer;
pub mod session;
pub mod spotify_cookie_sync;
pub mod spotify_scraper;
pub mod waterfall;
pub mod youtube_importer;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LosslessPlaybackState {
    pub current_track: Option<db::PlaylistTrack>,
    pub stream_info: Option<qobuz_client::LosslessStreamInfo>,
    pub is_playing: bool,
    pub position_ms: u128,
    pub next_track_pre_resolved: bool,
    pub origin_platform: Option<String>,
    pub lyrics_sync_mode: Option<String>,
    pub next_track: Option<db::PlaylistTrack>,
    pub next_stream_info: Option<qobuz_client::LosslessStreamInfo>,
}

impl LosslessPlaybackState {
    pub fn new(current_track: Option<db::PlaylistTrack>, origin_platform: Option<String>) -> Self {
        Self {
            current_track,
            stream_info: None,
            is_playing: true,
            position_ms: 0,
            next_track_pre_resolved: false,
            origin_platform,
            lyrics_sync_mode: None,
            next_track: None,
            next_stream_info: None,
        }
    }

    /// Origin badge, e.g. "[Origin: Spotify -> Relay: Qobuz]"
    pub fn origin_badge(&self) -> String {
        let origin = self.origin_platform.as_deref().unwrap_or("Spotify");
        let relay = self
            .stream_info
            .as_ref()
            .map(|s| s.provider_id.as_str())
            .unwrap_or("LosslessRelay");
        format!("[Origin: {} -> Relay: {}]", origin, relay)
    }

    /// Codec telemetry badge, e.g. "[FLAC 24-bit / 96.0 kHz | 2304 kbps]"
    pub fn codec_badge(&self) -> String {
        if let Some(info) = &self.stream_info {
            let bd = info.bit_depth.unwrap_or(24);
            let sr = info.sample_rate.unwrap_or(96.0);
            let br = info.bitrate_kbps.unwrap_or(2304);
            format!("[FLAC {}-bit / {:.1} kHz | {} kbps]", bd, sr, br)
        } else {
            "[FLAC 24-bit / 96.0 kHz | 2304 kbps]".to_string()
        }
    }

    /// Karaoke sync mode badge, e.g. "[Lyrics: Syllable Karaoke]"
    pub fn lyrics_badge(&self) -> String {
        let mode = self
            .lyrics_sync_mode
            .as_deref()
            .unwrap_or("Syllable Karaoke");
        format!("[Lyrics: {}]", mode)
    }

    /// Combined full telemetry line
    pub fn telemetry_string(&self) -> String {
        format!("{} {} {}", self.origin_badge(), self.codec_badge(), self.lyrics_badge())
    }

    /// Returns true when track has 20s or less remaining and pre-resolution has not yet run
    pub fn should_pre_resolve_next(&self) -> bool {
        if self.next_track_pre_resolved {
            return false;
        }
        if let Some(track) = &self.current_track {
            let dur = track.duration_ms as u128;
            if dur > 0 && self.position_ms + 20_000 >= dur {
                return true;
            }
        }
        false
    }
}
