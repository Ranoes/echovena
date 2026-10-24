use crate::sources::lossless_relay::qobuz_client::LosslessStreamInfo;
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep;

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 EchoVena/0.1.0";
const MAX_QUEUE_POLL_ATTEMPTS: usize = 12;

/// Global Semaphore(1) ensuring strictly one in-flight manifest resolution across the application
pub fn global_hifi_semaphore() -> Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(1))).clone()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HifiManifestResponse {
    #[serde(default, alias = "track_id")]
    pub track_id: Option<i64>,
    #[serde(default, alias = "audio_quality")]
    pub audio_quality: Option<String>, // "HI_RES" | "LOSSLESS" | "HIGH" | "LOW"
    #[serde(default, alias = "audio_mode")]
    pub audio_mode: Option<String>,    // "STEREO" (EAC3_JOC / Dolby Atmos strictly filtered)
    #[serde(default, alias = "mime_type")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub manifest: Option<String>,      // Base64 encoded DASH MPD or BTS manifest
    #[serde(default, alias = "bit_depth")]
    pub bit_depth: Option<u32>,
    #[serde(default, alias = "sample_rate")]
    pub sample_rate: Option<f64>,
    #[serde(default, alias = "request_id")]
    pub request_id: Option<String>,
    #[serde(default, alias = "queue_position")]
    pub queue_position: Option<usize>,
}

#[derive(Clone)]
pub struct HifiClient {
    client: reqwest::Client,
    playback_url: String,
    provider_id: String,
    semaphore: Arc<Semaphore>,
}

impl HifiClient {
    pub fn new(playback_url: &str, provider_id: &str, timeout_ms: u64) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .default_headers(headers)
            .build()
            .unwrap();

        Self {
            client,
            playback_url: playback_url.trim_end_matches('/').to_string(),
            provider_id: provider_id.to_string(),
            semaphore: global_hifi_semaphore(),
        }
    }

    pub fn with_client(client: reqwest::Client, playback_url: &str, provider_id: &str) -> Self {
        Self {
            client,
            playback_url: playback_url.trim_end_matches('/').to_string(),
            provider_id: provider_id.to_string(),
            semaphore: global_hifi_semaphore(),
        }
    }

    /// Requests a playback stream manifest for the given Tidal track ID.
    /// Strictly excludes `EAC3_JOC` (Dolby Atmos), acquires the `Semaphore(1)` lock,
    /// and handles HTTP `202 Accepted` queue polling.
    pub async fn get_stream_manifest(&self, track_id: &str) -> Result<LosslessStreamInfo> {
        // Enforce Architectural Guardrail #3: Single-In-Flight Queue Semaphore(1) lock
        let _permit = self
            .semaphore
            .acquire()
            .await
            .context("failed to acquire global hifi-api semaphore lock")?;

        let endpoint = format!("{}/trackManifests/{}", self.playback_url, track_id);

        // Explicitly request FLAC_HIRES, FLAC, and HIGH formats; exclude EAC3_JOC
        let resp = self
            .client
            .get(&endpoint)
            .query(&[
                ("formats", "FLAC_HIRES,FLAC,HIGH"),
                ("excludeFormats", "EAC3_JOC"),
            ])
            .send()
            .await
            .with_context(|| format!("failed hifi-api manifest request to {}", endpoint))?;

        let status = resp.status();

        if status == reqwest::StatusCode::ACCEPTED {
            // Handle HTTP 202 Accepted: inspect X-Playback-Queue-Position and poll request status
            let queue_pos = resp
                .headers()
                .get("X-Playback-Queue-Position")
                .and_then(|val| val.to_str().ok())
                .and_then(|s| s.parse::<usize>().ok());

            let body: HifiManifestResponse = resp.json().await.unwrap_or(HifiManifestResponse {
                track_id: None,
                audio_quality: None,
                audio_mode: None,
                mime_type: None,
                urls: vec![],
                manifest: None,
                bit_depth: None,
                sample_rate: None,
                request_id: None,
                queue_position: queue_pos,
            });

            let req_id = body
                .request_id
                .unwrap_or_else(|| format!("req_{}", track_id));

            return self.poll_queue_request(&req_id, queue_pos).await;
        }

        if !status.is_success() {
            return Err(anyhow!("hifi-api relay returned HTTP status {}", status));
        }

        let body: HifiManifestResponse = resp
            .json()
            .await
            .context("failed to parse hifi-api manifest response")?;

        self.extract_stream_info(body)
    }

    /// Polls `/playback/requests/{requestId}` with exponential backoff until `200 OK`
    async fn poll_queue_request(
        &self,
        request_id: &str,
        initial_queue_pos: Option<usize>,
    ) -> Result<LosslessStreamInfo> {
        if let Some(pos) = initial_queue_pos {
            log::info!("[Queue Position: #{}] Polling hifi-api request {}", pos, request_id);
        }

        let mut backoff_ms = 500u64;
        let poll_endpoint = format!("{}/playback/requests/{}", self.playback_url, request_id);

        for attempt in 1..=MAX_QUEUE_POLL_ATTEMPTS {
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(2000);

            let resp = self
                .client
                .get(&poll_endpoint)
                .send()
                .await
                .with_context(|| format!("failed polling hifi-api queue for request {}", request_id))?;

            let status = resp.status();
            if status == reqwest::StatusCode::ACCEPTED {
                let current_pos = resp
                    .headers()
                    .get("X-Playback-Queue-Position")
                    .and_then(|val| val.to_str().ok())
                    .and_then(|s| s.parse::<usize>().ok());

                if let Some(pos) = current_pos {
                    log::debug!(
                        "[Queue Position: #{}] Polling attempt {}/{}",
                        pos,
                        attempt,
                        MAX_QUEUE_POLL_ATTEMPTS
                    );
                }
                continue;
            }

            if status.is_success() {
                let body: HifiManifestResponse = resp
                    .json()
                    .await
                    .context("failed to parse completed hifi-api manifest")?;
                return self.extract_stream_info(body);
            }

            return Err(anyhow!(
                "hifi-api queue request {} failed with status {}",
                request_id,
                status
            ));
        }

        Err(anyhow!(
            "hifi-api queue polling timed out after {} attempts for request {}",
            MAX_QUEUE_POLL_ATTEMPTS,
            request_id
        ))
    }

    /// Parses manifest and formats into canonical `LosslessStreamInfo`
    fn extract_stream_info(&self, body: HifiManifestResponse) -> Result<LosslessStreamInfo> {
        // Enforce strict exclusion of EAC3_JOC (Dolby Atmos)
        if let Some(ref mode) = body.audio_mode {
            let m = mode.to_uppercase();
            if m.contains("EAC3") || m.contains("JOC") || m.contains("ATMOS") {
                return Err(anyhow!("hifi-api stream excluded: EAC3_JOC / Dolby Atmos unsupported"));
            }
        }

        let mut stream_url = body.urls.first().cloned();

        // If manifest is a Base64 encoded DASH/BTS payload, decode and extract primary audio URL
        if stream_url.is_none() {
            if let Some(ref b64_manifest) = body.manifest {
                if let Ok(decoded_bytes) = BASE64.decode(b64_manifest) {
                    let manifest_str = String::from_utf8_lossy(&decoded_bytes);
                    stream_url = parse_manifest_stream_url(&manifest_str);
                }
            }
        }

        let final_url = stream_url.ok_or_else(|| {
            anyhow!("no audio stream URL could be extracted from hifi-api manifest")
        })?;

        let quality = body.audio_quality.unwrap_or_else(|| "LOSSLESS".to_string()).to_uppercase();

        let (format, default_bit_depth, default_sample_rate) = match quality.as_str() {
            "HI_RES" | "LOSSLESS_HIRES" => ("FLAC_HIRES".to_string(), 24, 96.0),
            "LOSSLESS" => ("FLAC_CD".to_string(), 16, 44.1),
            "HIGH" => ("AAC_320".to_string(), 16, 44.1),
            _ => ("FLAC_CD".to_string(), 16, 44.1),
        };

        let bit_depth = body.bit_depth.or(Some(default_bit_depth));
        let sample_rate = body.sample_rate.or(Some(default_sample_rate));

        let bitrate_kbps = match format.as_str() {
            "FLAC_HIRES" => Some(2304),
            "FLAC_CD" => Some(800),
            "AAC_320" => Some(320),
            _ => Some(160),
        };

        Ok(LosslessStreamInfo {
            provider_id: self.provider_id.clone(),
            stream_url: final_url,
            format,
            bit_depth,
            sample_rate,
            bitrate_kbps,
            duration_secs: None,
        })
    }
}

/// Helper to parse audio segment URL from decoded DASH XML / BTS manifest
pub fn parse_manifest_stream_url(manifest_str: &str) -> Option<String> {
    // If it contains http URL directly, grab it
    for word in manifest_str.split(&['"', '\'', '<', '>', ' '][..]) {
        if (word.starts_with("http://") || word.starts_with("https://"))
            && (word.contains(".flac") || word.contains(".mp4") || word.contains(".m4a") || word.contains("audio"))
        {
            // Verify no EAC3 / Atmos in the candidate segment URL
            let w_lower = word.to_lowercase();
            if !w_lower.contains("eac3") && !w_lower.contains("joc") && !w_lower.contains("atmos") {
                return Some(word.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semaphore_singleton() {
        let sem1 = global_hifi_semaphore();
        let sem2 = global_hifi_semaphore();
        assert_eq!(Arc::as_ptr(&sem1), Arc::as_ptr(&sem2));
        assert_eq!(sem1.available_permits(), 1);
    }

    #[test]
    fn test_eac3_joc_exclusion() {
        let client = HifiClient::new("http://127.0.0.1:8000", "tidal-primary", 4500);
        let resp = HifiManifestResponse {
            track_id: Some(123),
            audio_quality: Some("HI_RES".to_string()),
            audio_mode: Some("EAC3_JOC".to_string()),
            mime_type: Some("audio/mp4".to_string()),
            urls: vec!["https://example.com/atmos.mp4".to_string()],
            manifest: None,
            bit_depth: Some(24),
            sample_rate: Some(48000.0),
            request_id: None,
            queue_position: None,
        };

        let result = client.extract_stream_info(resp);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("EAC3_JOC"));
    }

    #[test]
    fn test_manifest_stream_url_extraction() {
        let sample_dash = r#"
            <MPD xmlns="urn:mpeg:dash:schema:mpd:2011">
                <Period>
                    <AdaptationSet mimeType="audio/flac">
                        <Representation id="1" codecs="flac" audioSamplingRate="96000">
                            <BaseURL>https://sp-tidal.akamaized.net/stream/track_hi_res.flac</BaseURL>
                        </Representation>
                    </AdaptationSet>
                </Period>
            </MPD>
        "#;

        let url = parse_manifest_stream_url(sample_dash);
        assert_eq!(
            url,
            Some("https://sp-tidal.akamaized.net/stream/track_hi_res.flac".to_string())
        );
    }
}
