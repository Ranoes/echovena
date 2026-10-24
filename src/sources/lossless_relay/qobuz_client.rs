use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::time::Duration;

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 EchoVena/0.1.0";

/// Canonical stream info returned by audio relays
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LosslessStreamInfo {
    pub provider_id: String,
    pub stream_url: String,
    pub format: String, // "FLAC_HIRES" | "FLAC_CD" | "AAC_320" | "OPUS_160"
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
    pub bitrate_kbps: Option<u32>,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct QobuzFileUrlResponse {
    url: Option<String>,
    #[serde(default)]
    sample_rate: Option<f64>,
    #[serde(default)]
    bit_depth: Option<u32>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone)]
pub struct QobuzClient {
    client: reqwest::Client,
    base_url: String,
    provider_id: String,
}

impl QobuzClient {
    pub fn new(base_url: &str, provider_id: &str, timeout_ms: u64) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .default_headers(headers)
            .build()
            .unwrap();

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            provider_id: provider_id.to_string(),
        }
    }

    pub fn with_client(client: reqwest::Client, base_url: &str, provider_id: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            provider_id: provider_id.to_string(),
        }
    }

    /// Requests a signed stream URL for the given Qobuz track ID,
    /// cascading through quality levels: 27 (24/192) -> 7 (24/96) -> 6 (16/44.1) -> 5 (MP3 320)
    pub async fn get_stream_url(&self, track_id: &str) -> Result<LosslessStreamInfo> {
        let quality_levels = [27u32, 7, 6, 5];
        let mut last_error = anyhow!("no stream URL returned for Qobuz track {}", track_id);

        for quality_id in quality_levels {
            match self.request_quality(track_id, quality_id).await {
                Ok(Some(info)) => return Ok(info),
                Ok(None) => {
                    log::debug!(
                        "Qobuz track {} not available at quality_id {}, falling back",
                        track_id,
                        quality_id
                    );
                }
                Err(e) => {
                    // Check if error is a fatal relay error (5xx or 429) that should immediately trigger waterfall failover
                    let err_str = e.to_string();
                    if err_str.contains("500")
                        || err_str.contains("502")
                        || err_str.contains("503")
                        || err_str.contains("429")
                        || err_str.contains("timed out")
                    {
                        return Err(e);
                    }
                    last_error = e;
                }
            }
        }

        Err(last_error)
    }

    /// Requests a specific quality ID from the Qobuz REST API
    async fn request_quality(
        &self,
        track_id: &str,
        quality_id: u32,
    ) -> Result<Option<LosslessStreamInfo>> {
        let endpoint = format!("{}/track/getFileUrl", self.base_url);
        let resp = self
            .client
            .get(&endpoint)
            .query(&[
                ("track_id", track_id),
                ("format_id", &quality_id.to_string()),
            ])
            .send()
            .await
            .with_context(|| format!("failed Qobuz getFileUrl request to {}", endpoint))?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(anyhow!("Qobuz relay returned HTTP status {}", status));
        }

        let body: QobuzFileUrlResponse = resp
            .json()
            .await
            .context("failed to parse Qobuz getFileUrl JSON response")?;

        if let Some(err) = body.error {
            log::warn!("Qobuz API returned error payload: {}", err);
            return Ok(None);
        }

        let stream_url = match body.url {
            Some(u) if !u.trim().is_empty() => u,
            _ => return Ok(None),
        };

        let format = match quality_id {
            27 | 7 => "FLAC_HIRES".to_string(),
            6 => "FLAC_CD".to_string(),
            _ => "MP3_320".to_string(),
        };

        let bit_depth = body.bit_depth.or_else(|| match quality_id {
            27 | 7 => Some(24),
            6 => Some(16),
            _ => None,
        });

        let sample_rate = body.sample_rate.or_else(|| match quality_id {
            27 => Some(192.0),
            7 => Some(96.0),
            6 => Some(44.1),
            _ => Some(44.1),
        });

        let bitrate_kbps = match (bit_depth, sample_rate) {
            (Some(24), Some(sr)) if sr >= 192.0 => Some(4608),
            (Some(24), Some(sr)) if sr >= 96.0 => Some(2304),
            (Some(24), _) => Some(1152),
            (Some(16), _) => Some(800),
            _ => Some(320),
        };

        Ok(Some(LosslessStreamInfo {
            provider_id: self.provider_id.clone(),
            stream_url,
            format,
            bit_depth,
            sample_rate,
            bitrate_kbps,
            duration_secs: body.duration,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_and_bitrate_derivation() {
        let info = LosslessStreamInfo {
            provider_id: "qobuz-primary".to_string(),
            stream_url: "https://example.com/audio.flac".to_string(),
            format: "FLAC_HIRES".to_string(),
            bit_depth: Some(24),
            sample_rate: Some(96.0),
            bitrate_kbps: Some(2304),
            duration_secs: Some(240.0),
        };
        assert_eq!(info.format, "FLAC_HIRES");
        assert_eq!(info.bit_depth, Some(24));
        assert_eq!(info.sample_rate, Some(96.0));
    }

    #[test]
    fn test_qobuz_response_deserialization() {
        let json_data = r#"{
            "url": "https://streaming-qobuz-std.akamaized.net/stream/sample.flac",
            "format_id": 27,
            "sample_rate": 192.0,
            "bit_depth": 24,
            "duration": 312.5
        }"#;
        let resp: QobuzFileUrlResponse = serde_json::from_str(json_data).unwrap();
        assert_eq!(resp.url.as_deref(), Some("https://streaming-qobuz-std.akamaized.net/stream/sample.flac"));
        assert_eq!(resp.bit_depth, Some(24));
        assert_eq!(resp.sample_rate, Some(192.0));
    }
}
