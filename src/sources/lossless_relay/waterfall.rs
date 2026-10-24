use crate::config::providers::ProviderRegistry;
use crate::sources::lossless_relay::db::PlaylistTrack;
use crate::sources::lossless_relay::hifi_client::HifiClient;
use crate::sources::lossless_relay::qobuz_client::{LosslessStreamInfo, QobuzClient};
use crate::sources::lossless_relay::resolver::ResolvedTrack;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::timeout;

const COOLDOWN_DURATION_SECONDS: i64 = 600; // 10 minutes cooldown

#[derive(Debug, Default, Clone)]
pub struct ProviderHealthTracker {
    failures: HashMap<String, u32>,
    cooldown_until: HashMap<String, i64>,
}

impl ProviderHealthTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a successful request for a provider, resetting failure counts
    pub fn record_success(&mut self, provider_id: &str) {
        self.failures.remove(provider_id);
        self.cooldown_until.remove(provider_id);
    }

    /// Records a failure for a provider at a specific timestamp. If failures >= 2, puts provider in 10-min cooldown
    pub fn record_failure_at(&mut self, provider_id: &str, now: i64) {
        let count = self.failures.entry(provider_id.to_string()).or_insert(0);
        *count += 1;
        if *count >= 2 {
            let cooldown = now + COOLDOWN_DURATION_SECONDS;
            self.cooldown_until.insert(provider_id.to_string(), cooldown);
            log::warn!(
                "Provider {} hit {} consecutive failures. Cooled down for 10 minutes.",
                provider_id,
                *count
            );
        }
    }

    /// Records a failure for a provider. If failures >= 2, puts provider in 10-min cooldown
    pub fn record_failure(&mut self, provider_id: &str) {
        self.record_failure_at(provider_id, Utc::now().timestamp());
    }

    /// Puts a provider into immediate cooldown at a specific timestamp
    pub fn trigger_immediate_cooldown_at(&mut self, provider_id: &str, now: i64) {
        let cooldown = now + COOLDOWN_DURATION_SECONDS;
        self.cooldown_until.insert(provider_id.to_string(), cooldown);
        self.failures.insert(provider_id.to_string(), 2);
    }

    /// Puts a provider into immediate cooldown (e.g. for fatal 503 or 429 quota exhaustion)
    pub fn trigger_immediate_cooldown(&mut self, provider_id: &str) {
        self.trigger_immediate_cooldown_at(provider_id, Utc::now().timestamp());
    }

    /// Checks whether the provider is currently cooling down
    pub fn is_cooled_down(&self, provider_id: &str, now: i64) -> bool {
        if let Some(until) = self.cooldown_until.get(provider_id) {
            if now < *until {
                return true;
            }
        }
        false
    }
}

#[derive(Clone)]
pub struct LosslessWaterfall {
    registry: ProviderRegistry,
    health: Arc<Mutex<ProviderHealthTracker>>,
}

impl LosslessWaterfall {
    pub fn new(registry: ProviderRegistry) -> Self {
        Self {
            registry,
            health: Arc::new(Mutex::new(ProviderHealthTracker::new())),
        }
    }

    pub fn with_health(registry: ProviderRegistry, health: Arc<Mutex<ProviderHealthTracker>>) -> Self {
        Self { registry, health }
    }

    /// Resolves an audio stream for a track via the priority waterfall:
    /// Qobuz (FLAC Hi-Res/CD) -> Tidal (FLAC Hi-Res/CD/AAC) -> yt-dlp (Opus 160)
    pub async fn resolve_stream(
        &self,
        track: &PlaylistTrack,
        resolved: &ResolvedTrack,
    ) -> Result<LosslessStreamInfo> {
        let now = Utc::now().timestamp();

        // Direct YouTube optimization: bypass relays for uncataloged remixes/mixes
        if resolved.resolution_method == "yt_direct_fallback" {
            log::debug!(
                "Track {} flagged as yt_direct_fallback, streaming directly via yt-dlp",
                track.source_track_id
            );
            return self.stream_via_ytdlp(track).await;
        }

        let mut providers = self.registry.audio_providers.clone();
        providers.sort_by_key(|p| p.priority);

        let timeout_dur = Duration::from_millis(self.registry.waterfall_timeout_ms);

        for provider in &providers {
            if !provider.enabled {
                continue;
            }

            // Check health cooldown
            {
                let health_guard = self.health.lock().await;
                if health_guard.is_cooled_down(&provider.id, now) {
                    log::info!(
                        "Skipping provider {} because it is currently in health cooldown",
                        provider.id
                    );
                    continue;
                }
            }

            match provider.provider_type.as_str() {
                "qobuz-rest-api" => {
                    if let Some(ref qobuz_id) = resolved.qobuz_id {
                        let base = provider.base_url.as_deref().unwrap_or("http://127.0.0.1:8081");
                        let client = QobuzClient::new(base, &provider.id, self.registry.waterfall_timeout_ms);

                        match timeout(timeout_dur, client.get_stream_url(qobuz_id)).await {
                            Ok(Ok(info)) => {
                                let mut h = self.health.lock().await;
                                h.record_success(&provider.id);
                                return Ok(info);
                            }
                            Ok(Err(e)) => {
                                log::warn!("Qobuz relay {} failed: {}. Falling over...", provider.id, e);
                                let mut h = self.health.lock().await;
                                h.record_failure(&provider.id);
                            }
                            Err(_) => {
                                log::warn!("Qobuz relay {} timed out. Falling over...", provider.id);
                                let mut h = self.health.lock().await;
                                h.record_failure(&provider.id);
                            }
                        }
                    }
                }
                "hifi-api" => {
                    if let Some(ref tidal_id) = resolved.tidal_id {
                        let playback = provider
                            .playback_url
                            .as_deref()
                            .or_else(|| provider.catalog_url.as_deref())
                            .unwrap_or("http://127.0.0.1:8000");
                        let client = HifiClient::new(playback, &provider.id, self.registry.waterfall_timeout_ms);

                        match timeout(timeout_dur, client.get_stream_manifest(tidal_id)).await {
                            Ok(Ok(info)) => {
                                let mut h = self.health.lock().await;
                                h.record_success(&provider.id);
                                return Ok(info);
                            }
                            Ok(Err(e)) => {
                                log::warn!("Tidal hifi-api {} failed: {}. Falling over...", provider.id, e);
                                let mut h = self.health.lock().await;
                                h.record_failure(&provider.id);
                            }
                            Err(_) => {
                                log::warn!("Tidal hifi-api {} timed out. Falling over...", provider.id);
                                let mut h = self.health.lock().await;
                                h.record_failure(&provider.id);
                            }
                        }
                    }
                }
                "yt-dlp" => {
                    match timeout(timeout_dur, self.stream_via_ytdlp(track)).await {
                        Ok(Ok(info)) => {
                            let mut h = self.health.lock().await;
                            h.record_success(&provider.id);
                            return Ok(info);
                        }
                        Ok(Err(e)) => {
                            log::warn!("yt-dlp provider {} failed: {}", provider.id, e);
                        }
                        Err(_) => {
                            log::warn!("yt-dlp provider {} timed out", provider.id);
                        }
                    }
                }
                _ => {}
            }
        }

        // Final fallback: try yt-dlp even if not explicitly enabled in audio_providers
        self.stream_via_ytdlp(track).await
    }

    /// Streams audio via yt-dlp (Opus 160 kbps)
    pub async fn stream_via_ytdlp(&self, track: &PlaylistTrack) -> Result<LosslessStreamInfo> {
        let target = if let Some(vid) = track.source_track_id.strip_prefix("yt:video:") {
            format!("https://www.youtube.com/watch?v={}", vid)
        } else {
            format!("ytsearch1:{} {}", track.artist, track.clean_title)
        };

        let output = Command::new("yt-dlp")
            .arg("-g")
            .arg("-f")
            .arg("bestaudio[ext=webm]/bestaudio[acodec=opus]/bestaudio")
            .arg("--no-warnings")
            .arg(&target)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .with_context(|| format!("failed to invoke yt-dlp for track {}", track.source_track_id))?;

        if !output.status.success() {
            return Err(anyhow!("yt-dlp failed to extract audio stream for {}", target));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stream_url = stdout
            .lines()
            .find(|line| line.starts_with("http://") || line.starts_with("https://"))
            .ok_or_else(|| anyhow!("yt-dlp did not return a valid stream URL"))?
            .trim()
            .to_string();

        Ok(LosslessStreamInfo {
            provider_id: "ytdlp-fallback".to_string(),
            stream_url,
            format: "OPUS_160".to_string(),
            bit_depth: None,
            sample_rate: Some(48.0),
            bitrate_kbps: Some(160),
            duration_secs: Some(track.duration_ms as f64 / 1000.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_tracker_cooldown_lifecycle() {
        let mut tracker = ProviderHealthTracker::new();
        let provider = "test-provider";
        let now = 1_000_000i64;

        assert!(!tracker.is_cooled_down(provider, now));

        // 1st failure: not yet cooled down
        tracker.record_failure_at(provider, now);
        assert!(!tracker.is_cooled_down(provider, now));

        // 2nd failure: enters 10-minute cooldown
        tracker.record_failure_at(provider, now);
        assert!(tracker.is_cooled_down(provider, now));
        assert!(tracker.is_cooled_down(provider, now + 599));
        assert!(!tracker.is_cooled_down(provider, now + 601)); // Cooldown expired

        // Success resets failure count and clears cooldown
        tracker.record_success(provider);
        assert!(!tracker.is_cooled_down(provider, now));
    }

    #[test]
    fn test_waterfall_priority_ordering() {
        let registry = ProviderRegistry::default_registry();
        let mut providers = registry.audio_providers.clone();
        providers.sort_by_key(|p| p.priority);

        assert_eq!(providers[0].provider_type, "qobuz-rest-api");
        assert_eq!(providers[1].provider_type, "hifi-api");
        assert_eq!(providers.last().unwrap().provider_type, "yt-dlp");
    }

    #[tokio::test]
    async fn test_waterfall_failover_qobuz_503_to_hifi_api_202_then_200() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        // 1. Qobuz server returns 503 Service Unavailable
        let qobuz_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let qobuz_addr = qobuz_listener.local_addr().unwrap();
        let qobuz_url = format!("http://{}", qobuz_addr);

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = qobuz_listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let response = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });

        // 2. Tidal server: request 1 returns 202 Accepted; request 2 (polling) returns 200 OK
        let tidal_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tidal_addr = tidal_listener.local_addr().unwrap();
        let tidal_url = format!("http://{}", tidal_addr);

        tokio::spawn(async move {
            // First request: /trackManifests/... -> 202 Accepted
            if let Ok((mut stream1, _)) = tidal_listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream1.read(&mut buf).await;
                let body = r#"{"requestId":"test-poll-1","queuePosition":1}"#;
                let response = format!(
                    "HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nX-Playback-Queue-Position: 1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream1.write_all(response.as_bytes()).await;
            }

            // Second request: /playback/requests/test-poll-1 -> 200 OK
            if let Ok((mut stream2, _)) = tidal_listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream2.read(&mut buf).await;
                let body = r#"{"audioQuality":"HI_RES","bitDepth":24,"sampleRate":96000.0,"urls":["https://streaming.example.com/stream.flac"]}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream2.write_all(response.as_bytes()).await;
            }
        });

        // 3. Configure waterfall with these two mock providers
        let registry = ProviderRegistry {
            version: 1,
            waterfall_timeout_ms: 2000,
            max_concurrent_manifest_requests: 1,
            audio_providers: vec![
                crate::config::providers::AudioProviderConfig {
                    id: "qobuz-mock".to_string(),
                    provider_type: "qobuz-rest-api".to_string(),
                    enabled: true,
                    priority: 1,
                    base_url: Some(qobuz_url),
                    catalog_url: None,
                    playback_url: None,
                    supported_formats: vec!["FLAC_HIRES".to_string()],
                    username: None,
                    password_env: None,
                },
                crate::config::providers::AudioProviderConfig {
                    id: "tidal-mock".to_string(),
                    provider_type: "hifi-api".to_string(),
                    enabled: true,
                    priority: 2,
                    base_url: None,
                    catalog_url: Some(tidal_url.clone()),
                    playback_url: Some(tidal_url),
                    supported_formats: vec!["FLAC_HIRES".to_string()],
                    username: None,
                    password_env: None,
                },
            ],
            lyric_providers: vec![],
        };

        let waterfall = LosslessWaterfall::new(registry);
        let track = PlaylistTrack {
            playlist_id: "pl:1".to_string(),
            position: 0,
            source_track_id: "spotify:track:123".to_string(),
            raw_title: "Track".to_string(),
            clean_title: "Track".to_string(),
            artist: "Artist".to_string(),
            album: None,
            duration_ms: 240_000,
            isrc: None,
            cover_art_url: None,
        };
        let resolved = ResolvedTrack {
            source_track_id: "spotify:track:123".to_string(),
            isrc: None,
            qobuz_id: Some("qobuz_123".to_string()),
            tidal_id: Some("tidal_456".to_string()),
            resolution_method: "songlink".to_string(),
            match_confidence: 0.95,
        };

        let stream_info = waterfall.resolve_stream(&track, &resolved).await.unwrap();

        assert_eq!(stream_info.provider_id, "tidal-mock");
        assert_eq!(stream_info.format, "FLAC_HIRES");
        assert_eq!(stream_info.stream_url, "https://streaming.example.com/stream.flac");
        assert_eq!(stream_info.bit_depth, Some(24));
    }
}
