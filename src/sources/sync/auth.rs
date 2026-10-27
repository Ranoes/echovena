use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::header::{COOKIE, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::config::providers::ProviderRegistry;
use crate::sources::lossless_relay::spotify_cookie_sync::{
    load_encrypted_sp_dc, save_encrypted_sp_dc, SpotifyAuthToken,
};

const SPOTIFY_WEB_TOKEN_URL: &str =
    "https://open.spotify.com/get_access_token?reason=transport&productType=web_player";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Spotify authentication handler supporting sp_dc cookie resolution,
/// secure ChaCha20 storage, web token exchange, and in-memory token caching.
#[derive(Clone)]
pub struct SpotifyAuthHandler {
    client: reqwest::Client,
    cached_token: Arc<Mutex<Option<CachedSpotifyToken>>>,
}

#[derive(Debug, Clone)]
struct CachedSpotifyToken {
    token: String,
    expires_at_ms: i64,
}

impl Default for SpotifyAuthHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SpotifyAuthHandler {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder().build().unwrap(),
            cached_token: Arc::new(Mutex::new(None)),
        }
    }

    /// Resolves `sp_dc` cookie in order of precedence:
    /// 1. SPOTIFY_SP_DC or ECHOVENA_SP_DC env variable
    /// 2. ~/.local/share/echovena/credentials.enc (encrypted)
    /// 3. ~/.config/echovena/providers.json (`spotify_sp_dc`)
    pub fn resolve_sp_dc() -> Option<String> {
        if let Ok(val) = std::env::var("SPOTIFY_SP_DC") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Ok(val) = std::env::var("ECHOVENA_SP_DC") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        if let Ok(Some(sp_dc)) = load_encrypted_sp_dc() {
            let trimmed = sp_dc.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        if let Ok(reg) = ProviderRegistry::load_or_create_default() {
            if let Some(sp_dc) = reg.spotify_sp_dc {
                let trimmed = sp_dc.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }

        None
    }

    /// Persists `sp_dc` cookie securely to encrypted storage and providers.json
    pub fn save_sp_dc(sp_dc: &str) -> Result<()> {
        let trimmed = sp_dc.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("cannot save empty sp_dc cookie"));
        }

        // 1. Save to encrypted credentials.enc
        save_encrypted_sp_dc(trimmed).context("failed to write encrypted sp_dc")?;

        // 2. Also record in providers.json for explicit config persistence
        if let Ok(mut reg) = ProviderRegistry::load_or_create_default() {
            reg.spotify_sp_dc = Some(trimmed.to_string());
            if let Some(path) = ProviderRegistry::default_path() {
                let _ = reg.save_to_file(&path);
            }
        }

        Ok(())
    }

    /// Retrieves a valid Spotify bearer access token, reusing cached token if unexpired
    pub async fn get_valid_token(&self) -> Result<String> {
        let now_ms = Utc::now().timestamp_millis();

        {
            let cache = self.cached_token.lock().unwrap();
            if let Some(cached) = &*cache {
                // Keep 60 seconds grace period
                if cached.expires_at_ms > now_ms + 60_000 {
                    return Ok(cached.token.clone());
                }
            }
        }

        let sp_dc = Self::resolve_sp_dc().ok_or_else(|| {
            anyhow!("no Spotify sp_dc cookie found in env, encrypted credentials, or providers.json")
        })?;

        let token_resp = self.exchange_token(&sp_dc).await?;

        {
            let mut cache = self.cached_token.lock().unwrap();
            *cache = Some(CachedSpotifyToken {
                token: token_resp.access_token.clone(),
                expires_at_ms: token_resp.expiration_timestamp_ms,
            });
        }

        Ok(token_resp.access_token)
    }

    /// Exchanges the `sp_dc` cookie for an authenticated Spotify bearer token
    pub async fn exchange_token(&self, sp_dc: &str) -> Result<SpotifyAuthToken> {
        let cookie_header = format!("sp_dc={}", sp_dc.trim());

        let resp = self
            .client
            .get(SPOTIFY_WEB_TOKEN_URL)
            .header(COOKIE, cookie_header)
            .header(USER_AGENT, DEFAULT_USER_AGENT)
            .send()
            .await
            .context("failed to query spotify web token exchange endpoint")?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "spotify web token exchange returned HTTP status {}",
                resp.status()
            ));
        }

        let token: SpotifyAuthToken = resp
            .json()
            .await
            .context("failed parsing spotify auth token JSON")?;

        if token.is_anonymous {
            return Err(anyhow!(
                "sp_dc cookie was rejected or expired by Spotify (anonymous session)"
            ));
        }

        Ok(token)
    }

    /// Validates if active Spotify credentials are operational
    pub async fn validate_auth(&self) -> Result<bool> {
        match self.get_valid_token().await {
            Ok(_) => Ok(true),
            Err(e) => {
                log::warn!("Spotify auth validation failed: {:#}", e);
                Ok(false)
            }
        }
    }
}

/// YouTube authentication handler resolving cookies from file, environment, or providers.json
#[derive(Debug, Clone, Default)]
pub struct YouTubeAuthHandler;

impl YouTubeAuthHandler {
    pub fn new() -> Self {
        Self
    }

    /// Default path for youtube cookies: ~/.config/echovena/youtube_cookies.txt
    pub fn default_cookies_path() -> Option<PathBuf> {
        crate::core::paths::app_config_dir().map(|d| d.join("youtube_cookies.txt"))
    }

    /// Resolves path to YouTube cookies file:
    /// 1. YOUTUBE_COOKIES_PATH env var
    /// 2. ~/.config/echovena/providers.json (`youtube_cookies_path`)
    /// 3. ~/.config/echovena/youtube_cookies.txt (if exists)
    pub fn resolve_cookies_path() -> Option<PathBuf> {
        if let Ok(env_path) = std::env::var("YOUTUBE_COOKIES_PATH") {
            let path = PathBuf::from(env_path.trim());
            if path.exists() {
                return Some(path);
            }
        }

        if let Ok(reg) = ProviderRegistry::load_or_create_default() {
            if let Some(cp) = reg.youtube_cookies_path {
                let path = PathBuf::from(cp.trim());
                if path.exists() {
                    return Some(path);
                }
            }
        }

        if let Some(default_path) = Self::default_cookies_path() {
            if default_path.exists() {
                return Some(default_path);
            }
        }

        None
    }

    /// Resolves YouTube Cookie HTTP header string:
    /// 1. YOUTUBE_COOKIE env var
    /// 2. ~/.config/echovena/providers.json (`youtube_cookie`)
    /// 3. Parsed from Netscape format cookies file
    pub fn resolve_cookie_header() -> Option<String> {
        if let Ok(c) = std::env::var("YOUTUBE_COOKIE") {
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        if let Ok(reg) = ProviderRegistry::load_or_create_default() {
            if let Some(c) = reg.youtube_cookie {
                let trimmed = c.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }

        if let Some(path) = Self::resolve_cookies_path() {
            if let Ok(contents) = fs::read_to_string(&path) {
                let parsed = Self::parse_netscape_cookies(&contents);
                if !parsed.is_empty() {
                    return Some(parsed);
                }
            }
        }

        None
    }

    /// Parses Netscape format cookies into standard `key1=val1; key2=val2` Cookie header
    pub fn parse_netscape_cookies(content: &str) -> String {
        let mut pairs = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') && !trimmed.starts_with("#HttpOnly_") {
                continue;
            }

            let clean_line = trimmed.strip_prefix("#HttpOnly_").unwrap_or(trimmed);
            let parts: Vec<&str> = clean_line.split('\t').collect();
            if parts.len() >= 7 {
                let name = parts[5].trim();
                let value = parts[6].trim();
                if !name.is_empty() && !value.is_empty() {
                    pairs.push(format!("{}={}", name, value));
                }
            }
        }
        pairs.join("; ")
    }

    /// Saves YouTube cookies to ~/.config/echovena/youtube_cookies.txt
    pub fn save_cookies_file(content: &str) -> Result<PathBuf> {
        let path = Self::default_cookies_path()
            .ok_or_else(|| anyhow!("failed resolving youtube_cookies.txt destination path"))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating directory {}", parent.display()))?;
        }

        fs::write(&path, content)
            .with_context(|| format!("failed writing youtube cookies to {}", path.display()))?;

        if let Ok(mut reg) = ProviderRegistry::load_or_create_default() {
            reg.youtube_cookies_path = Some(path.to_string_lossy().to_string());
            if let Some(p) = ProviderRegistry::default_path() {
                let _ = reg.save_to_file(&p);
            }
        }

        Ok(path)
    }

    /// Returns `["--cookies", <path>]` command arguments for yt-dlp if a cookies file exists
    pub fn ytdlp_cookie_args() -> Vec<String> {
        if let Some(path) = Self::resolve_cookies_path() {
            vec!["--cookies".to_string(), path.to_string_lossy().to_string()]
        } else {
            Vec::new()
        }
    }

    /// Checks if any YouTube authentication credentials (cookie file or header) are available
    pub fn has_auth() -> bool {
        Self::resolve_cookie_header().is_some() || Self::resolve_cookies_path().is_some()
    }
}

/// Unified authentication credentials status for sync engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAuthStatus {
    pub spotify_authenticated: bool,
    pub spotify_sp_dc_source: Option<String>,
    pub youtube_authenticated: bool,
    pub youtube_cookies_source: Option<String>,
}

impl SyncAuthStatus {
    pub fn detect() -> Self {
        let (sp_auth, sp_src) = if std::env::var("SPOTIFY_SP_DC").is_ok()
            || std::env::var("ECHOVENA_SP_DC").is_ok()
        {
            (true, Some("environment variable".to_string()))
        } else if load_encrypted_sp_dc().ok().flatten().is_some() {
            (true, Some("encrypted storage (credentials.enc)".to_string()))
        } else if let Ok(reg) = ProviderRegistry::load_or_create_default() {
            if reg.spotify_sp_dc.is_some() {
                (true, Some("providers.json".to_string()))
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };

        let (yt_auth, yt_src) = if std::env::var("YOUTUBE_COOKIE").is_ok()
            || std::env::var("YOUTUBE_COOKIES_PATH").is_ok()
        {
            (true, Some("environment variable".to_string()))
        } else if let Some(path) = YouTubeAuthHandler::resolve_cookies_path() {
            (true, Some(format!("cookies file ({})", path.display())))
        } else if let Ok(reg) = ProviderRegistry::load_or_create_default() {
            if reg.youtube_cookie.is_some() {
                (true, Some("providers.json".to_string()))
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };

        Self {
            spotify_authenticated: sp_auth,
            spotify_sp_dc_source: sp_src,
            youtube_authenticated: yt_auth,
            youtube_cookies_source: yt_src,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_netscape_cookies() {
        let sample = r#"
# Netscape HTTP Cookie File
# http://curl.haxx.se/rfc/cookie_spec.html
.youtube.com	TRUE	/	TRUE	1767139200	LOGIN_INFO	AFmmF2swRQIhAM5678:v1
#HttpOnly_.youtube.com	TRUE	/	TRUE	1767139200	__Secure-3PSID	sample_secure_sid_value
.youtube.com	TRUE	/	FALSE	1767139200	PREF	f4=4000000&hl=en
"#;
        let header = YouTubeAuthHandler::parse_netscape_cookies(sample);
        assert!(header.contains("LOGIN_INFO=AFmmF2swRQIhAM5678:v1"));
        assert!(header.contains("__Secure-3PSID=sample_secure_sid_value"));
        assert!(header.contains("PREF=f4=4000000&hl=en"));
        assert!(header.contains("; "));
    }

    #[test]
    fn test_ytdlp_cookie_args_empty_when_no_file() {
        // When no file exists at non-existent path, args are empty or reflect existing file
        let args = YouTubeAuthHandler::ytdlp_cookie_args();
        // If system has no cookies, it returns empty vec
        if YouTubeAuthHandler::resolve_cookies_path().is_none() {
            assert!(args.is_empty());
        }
    }

    #[test]
    fn test_sync_auth_status_detect() {
        let status = SyncAuthStatus::detect();
        // Check struct shape
        let _ = format!("{:?}", status);
    }
}
