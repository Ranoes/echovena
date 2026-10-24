use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRegistry {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_waterfall_timeout_ms")]
    pub waterfall_timeout_ms: u64,
    #[serde(default = "default_max_concurrent_manifest_requests")]
    pub max_concurrent_manifest_requests: usize,
    #[serde(default)]
    pub audio_providers: Vec<AudioProviderConfig>,
    #[serde(default)]
    pub lyric_providers: Vec<LyricProviderConfig>,
}

fn default_version() -> u32 {
    1
}

fn default_waterfall_timeout_ms() -> u64 {
    4500
}

fn default_max_concurrent_manifest_requests() -> usize {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioProviderConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub catalog_url: Option<String>,
    #[serde(default)]
    pub playback_url: Option<String>,
    #[serde(default)]
    pub supported_formats: Vec<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password_env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricProviderConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: u32,
    pub base_url: String,
}

fn default_true() -> bool {
    true
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}

impl ProviderRegistry {
    pub fn default_registry() -> Self {
        Self {
            version: 1,
            waterfall_timeout_ms: 4500,
            max_concurrent_manifest_requests: 1,
            audio_providers: vec![
                AudioProviderConfig {
                    id: "qobuz-primary".to_string(),
                    provider_type: "qobuz-rest-api".to_string(),
                    enabled: true,
                    priority: 1,
                    base_url: Some("http://127.0.0.1:8081".to_string()),
                    catalog_url: None,
                    playback_url: None,
                    supported_formats: vec!["FLAC_HIRES".to_string(), "FLAC_CD".to_string()],
                    username: None,
                    password_env: None,
                },
                AudioProviderConfig {
                    id: "tidal-hifi-primary".to_string(),
                    provider_type: "hifi-api".to_string(),
                    enabled: true,
                    priority: 2,
                    base_url: None,
                    catalog_url: Some("http://127.0.0.1:8000".to_string()),
                    playback_url: Some("http://127.0.0.1:8000".to_string()),
                    supported_formats: vec![
                        "FLAC_HIRES".to_string(),
                        "FLAC_CD".to_string(),
                        "AAC_320".to_string(),
                    ],
                    username: None,
                    password_env: None,
                },
                AudioProviderConfig {
                    id: "local-hifi-subsonic".to_string(),
                    provider_type: "opensubsonic".to_string(),
                    enabled: false,
                    priority: 3,
                    base_url: Some("http://127.0.0.1:8080".to_string()),
                    catalog_url: None,
                    playback_url: None,
                    supported_formats: vec![],
                    username: Some("user".to_string()),
                    password_env: Some("ECHOVENA_SUBSONIC_PASSWORD".to_string()),
                },
                AudioProviderConfig {
                    id: "ytdlp-fallback".to_string(),
                    provider_type: "yt-dlp".to_string(),
                    enabled: true,
                    priority: 99,
                    base_url: None,
                    catalog_url: None,
                    playback_url: None,
                    supported_formats: vec!["OPUS_160".to_string()],
                    username: None,
                    password_env: None,
                },
            ],
            lyric_providers: vec![
                LyricProviderConfig {
                    id: "paxsenix-word-sync".to_string(),
                    provider_type: "paxsenix-karaoke".to_string(),
                    enabled: true,
                    priority: 1,
                    base_url: "https://lyrics.paxsenix.org".to_string(),
                },
                LyricProviderConfig {
                    id: "spotify-color-lyrics-internal".to_string(),
                    provider_type: "spotify-spclient".to_string(),
                    enabled: true,
                    priority: 2,
                    base_url: "https://spclient.wg.spotify.com/color-lyrics/v2/track".to_string(),
                },
                LyricProviderConfig {
                    id: "spotify-lyrics-bypass-bridge".to_string(),
                    provider_type: "spotify-lyrics-bridge".to_string(),
                    enabled: true,
                    priority: 3,
                    base_url: "http://127.0.0.1:8082".to_string(),
                },
                LyricProviderConfig {
                    id: "lrclib-open".to_string(),
                    provider_type: "lrclib".to_string(),
                    enabled: true,
                    priority: 4,
                    base_url: "https://lrclib.net/api".to_string(),
                },
                LyricProviderConfig {
                    id: "netease-yrc".to_string(),
                    provider_type: "netease".to_string(),
                    enabled: true,
                    priority: 5,
                    base_url: "https://music.163.com/api".to_string(),
                },
            ],
        }
    }

    pub fn default_path() -> Option<PathBuf> {
        crate::core::paths::app_config_dir().map(|d| d.join("providers.json"))
    }

    pub fn load_or_create_default() -> Result<Self> {
        let Some(path) = Self::default_path() else {
            return Ok(Self::default_registry());
        };

        if path.exists() {
            Self::load_from_file(&path)
        } else {
            let default_reg = Self::default_registry();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed creating directory {}", parent.display()))?;
            }
            default_reg.save_to_file(&path)?;
            Ok(default_reg)
        }
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed reading providers config from {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed parsing providers json from {}", path.display()))
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .context("failed serializing provider registry to json")?;
        fs::write(path, content)
            .with_context(|| format!("failed writing providers json to {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_serialization() {
        let default_reg = ProviderRegistry::default_registry();
        let json = serde_json::to_string_pretty(&default_reg).unwrap();
        let parsed: ProviderRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(default_reg, parsed);
        assert_eq!(parsed.audio_providers.len(), 4);
        assert_eq!(parsed.lyric_providers.len(), 5);
    }
}
