pub mod providers;
pub use providers::{AudioProviderConfig, LyricProviderConfig, ProviderRegistry};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LosslessRelayConfig {
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,
    #[serde(default = "default_sp_dc_cookie_env")]
    pub sp_dc_cookie_env: String,
    #[serde(default = "default_prefer_quality")]
    pub prefer_quality: String,
    #[serde(default = "default_true")]
    pub exclude_dolby_atmos: bool,
    #[serde(default = "default_pre_resolve_seconds")]
    pub pre_resolve_next_track_seconds: u32,
    #[serde(default)]
    pub playlist_import: PlaylistImportConfig,
    #[serde(default)]
    pub karaoke: KaraokeConfig,
    #[serde(default)]
    pub archiving: ArchivingConfig,
}

fn default_auth_mode() -> String {
    "cookie".to_string()
}
fn default_sp_dc_cookie_env() -> String {
    "ECHOVENA_SP_DC".to_string()
}
fn default_prefer_quality() -> String {
    "FLAC_HIRES".to_string()
}
fn default_true() -> bool {
    true
}
fn default_pre_resolve_seconds() -> u32 {
    20
}

impl Default for LosslessRelayConfig {
    fn default() -> Self {
        Self {
            auth_mode: default_auth_mode(),
            sp_dc_cookie_env: default_sp_dc_cookie_env(),
            prefer_quality: default_prefer_quality(),
            exclude_dolby_atmos: true,
            pre_resolve_next_track_seconds: default_pre_resolve_seconds(),
            playlist_import: PlaylistImportConfig::default(),
            karaoke: KaraokeConfig::default(),
            archiving: ArchivingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistImportConfig {
    #[serde(default = "default_true")]
    pub auto_background_resolve: bool,
    #[serde(default = "default_resolve_rate_limit_ms")]
    pub background_resolve_rate_limit_ms: u64,
    #[serde(default = "default_yt_tolerance_ms")]
    pub youtube_fuzzy_duration_tolerance_ms: u64,
    #[serde(default)]
    pub sync_playlists_on_startup: bool,
}

fn default_resolve_rate_limit_ms() -> u64 {
    500
}
fn default_yt_tolerance_ms() -> u64 {
    4000
}

impl Default for PlaylistImportConfig {
    fn default() -> Self {
        Self {
            auto_background_resolve: true,
            background_resolve_rate_limit_ms: 500,
            youtube_fuzzy_duration_tolerance_ms: 4000,
            sync_playlists_on_startup: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KaraokeConfig {
    #[serde(default = "default_true")]
    pub prefer_word_sync: bool,
    #[serde(default = "default_true")]
    pub show_romanization: bool,
    #[serde(default)]
    pub show_translation: bool,
    #[serde(default = "default_instrumental_ms")]
    pub instrumental_countdown_threshold_ms: u64,
    #[serde(default)]
    pub default_offset_ms: i32,
}

fn default_instrumental_ms() -> u64 {
    5000
}

impl Default for KaraokeConfig {
    fn default() -> Self {
        Self {
            prefer_word_sync: true,
            show_romanization: true,
            show_translation: false,
            instrumental_countdown_threshold_ms: 5000,
            default_offset_ms: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivingConfig {
    #[serde(default)]
    pub archive_completed_streams: bool,
    #[serde(default = "default_batch_delay_ms")]
    pub batch_download_delay_ms: u64,
    #[serde(default = "default_archive_dir")]
    pub archive_directory: String,
    #[serde(default = "default_archive_template")]
    pub archive_filename_template: String,
    #[serde(default = "default_true")]
    pub embed_karaoke_elrc: bool,
    #[serde(default = "default_true")]
    pub generate_m3u8_for_playlists: bool,
}

fn default_batch_delay_ms() -> u64 {
    3000
}
fn default_archive_dir() -> String {
    "~/Music/EchoVena".to_string()
}
fn default_archive_template() -> String {
    "{artist}/{album}/{track_num} - {title}".to_string()
}

impl Default for ArchivingConfig {
    fn default() -> Self {
        Self {
            archive_completed_streams: false,
            batch_download_delay_ms: 3000,
            archive_directory: default_archive_dir(),
            archive_filename_template: default_archive_template(),
            embed_karaoke_elrc: true,
            generate_m3u8_for_playlists: true,
        }
    }
}
