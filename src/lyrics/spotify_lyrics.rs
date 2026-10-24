use crate::lyrics::{KaraokeLine, KaraokeTimeline, KaraokeWord, SyncType, VocalRole};
use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, COOKIE, USER_AGENT};
use serde::Deserialize;
use serde_json::Value;

const SPOTIFY_COLOR_LYRICS_URL: &str =
    "https://spclient.wg.spotify.com/color-lyrics/v2/track";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug, Deserialize)]
struct SpotifyColorLyricsResponse {
    lyrics: Option<SpotifyLyricsBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotifyLyricsBody {
    sync_type: Option<String>,
    #[serde(default)]
    lines: Vec<SpotifyLyricLine>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotifyLyricLine {
    start_time_ms: String,
    #[serde(default)]
    words: String,
    #[serde(default)]
    syllables: Vec<SpotifySyllable>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotifySyllable {
    start_time_ms: String,
    num_chars: String,
}

/// Parses Spotify color-lyrics JSON into unified KaraokeTimeline
pub fn parse_spotify_color_lyrics(json_str: &str, provider_name: &str) -> Result<KaraokeTimeline> {
    let body: SpotifyColorLyricsResponse =
        serde_json::from_str(json_str).context("invalid Spotify color-lyrics JSON")?;

    let lyrics_data = body
        .lyrics
        .ok_or_else(|| anyhow!("no lyrics field in Spotify response"))?;

    let sync_type_str = lyrics_data
        .sync_type
        .unwrap_or_else(|| "LINE_SYNCED".to_string())
        .to_uppercase();

    let is_syllable = sync_type_str == "SYLLABLE_SYNCED";

    let mut lines = Vec::new();

    for (idx, line_item) in lyrics_data.lines.iter().enumerate() {
        let line_start_ms = line_item.start_time_ms.parse::<i64>().unwrap_or(0);
        let next_start_ms = lyrics_data
            .lines
            .get(idx + 1)
            .and_then(|l| l.start_time_ms.parse::<i64>().ok())
            .unwrap_or(line_start_ms + 3000);

        let line_end_ms = next_start_ms;
        let mut words = Vec::new();

        if is_syllable && !line_item.syllables.is_empty() {
            let full_text = &line_item.words;
            let mut char_offset = 0usize;
            let char_vec: Vec<char> = full_text.chars().collect();

            for (s_idx, syl) in line_item.syllables.iter().enumerate() {
                let s_start_ms = syl.start_time_ms.parse::<i64>().unwrap_or(line_start_ms);
                let num_chars = syl.num_chars.parse::<usize>().unwrap_or(1);

                let s_end_ms = if s_idx + 1 < line_item.syllables.len() {
                    line_item.syllables[s_idx + 1]
                        .start_time_ms
                        .parse::<i64>()
                        .unwrap_or(s_start_ms + 300)
                } else {
                    line_end_ms
                };

                let end_char = (char_offset + num_chars).min(char_vec.len());
                let chunk_str: String = char_vec[char_offset..end_char].iter().collect();
                char_offset = end_char;

                if !chunk_str.is_empty() {
                    words.push(KaraokeWord::new(&chunk_str, s_start_ms, s_end_ms));
                }
            }
        }

        if words.is_empty() {
            words.push(KaraokeWord::new(&line_item.words, line_start_ms, line_end_ms));
        }

        lines.push(KaraokeLine {
            text: line_item.words.clone(),
            start_time_ms: line_start_ms,
            end_time_ms: line_end_ms,
            words,
            vocal_role: VocalRole::Lead1,
            romanization: None,
            translation: None,
        });
    }

    if lines.is_empty() {
        return Err(anyhow!("no lines in Spotify lyrics payload"));
    }

    Ok(KaraokeTimeline {
        provider: provider_name.to_string(),
        sync_type: if is_syllable {
            SyncType::Syllable
        } else {
            SyncType::Line
        },
        lines,
        raw_elrc: None,
    })
}

#[derive(Clone)]
pub struct SpotifyLyricsClient {
    client: reqwest::Client,
}

impl Default for SpotifyLyricsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SpotifyLyricsClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder().build().unwrap(),
        }
    }

    /// Fetches lyrics from Spotify's internal color-lyrics endpoint using access token and sp_dc cookie
    pub async fn fetch_internal_lyrics(
        &self,
        spotify_track_id: &str,
        access_token: &str,
        sp_dc: &str,
    ) -> Result<KaraokeTimeline> {
        let url = format!(
            "{}/{}?format=json&vocalRemoval=false&market=from_token",
            SPOTIFY_COLOR_LYRICS_URL, spotify_track_id
        );

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", access_token))?,
        );
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("sp_dc={}", sp_dc))?,
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("app-platform"),
            HeaderValue::from_static("WebPlayer"),
        );

        let resp = self
            .client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .context("failed sending Spotify color-lyrics request")?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Spotify color-lyrics returned status {}",
                resp.status()
            ));
        }

        let json_text = resp.text().await?;
        parse_spotify_color_lyrics(&json_text, "spotify_color_lyrics")
    }

    /// Fetches lyrics from a public open-source spotify-lyrics-api bypass bridge
    pub async fn fetch_bypass_lyrics(
        &self,
        bridge_base_url: &str,
        spotify_track_id: &str,
    ) -> Result<KaraokeTimeline> {
        let url = format!(
            "{}?trackid={}&format=json",
            bridge_base_url.trim_end_matches('/'),
            spotify_track_id
        );

        let resp = self
            .client
            .get(&url)
            .header(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT))
            .send()
            .await
            .context("failed sending bypass lyrics request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("bypass lyrics relay returned {}", resp.status()));
        }

        let body: Value = resp.json().await?;

        // If returned in Spotify color-lyrics shape
        if body.get("lyrics").is_some() {
            return parse_spotify_color_lyrics(&body.to_string(), "spotify_bypass_bridge");
        }

        // If returned in lines array shape
        if let Some(lines_arr) = body.get("lines").and_then(|l| l.as_array()) {
            let mut lines = Vec::new();
            for item in lines_arr {
                let start_ms = item["startTimeMs"]
                    .as_str()
                    .and_then(|s| s.parse::<i64>().ok())
                    .or_else(|| item["startTimeMs"].as_i64())
                    .unwrap_or(0);
                let text = item["words"].as_str().unwrap_or("").to_string();

                lines.push(KaraokeLine {
                    text: text.clone(),
                    start_time_ms: start_ms,
                    end_time_ms: start_ms + 3000,
                    words: vec![KaraokeWord::new(&text, start_ms, start_ms + 3000)],
                    vocal_role: VocalRole::Lead1,
                    romanization: None,
                    translation: None,
                });
            }

            if !lines.is_empty() {
                return Ok(KaraokeTimeline {
                    provider: "spotify_bypass_bridge".to_string(),
                    sync_type: SyncType::Line,
                    lines,
                    raw_elrc: None,
                });
            }
        }

        Err(anyhow!("unable to parse bypass bridge lyric response"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_spotify_syllable_synced() {
        let sample = r#"{
            "lyrics": {
                "syncType": "SYLLABLE_SYNCED",
                "lines": [
                    {
                        "startTimeMs": "12340",
                        "words": "Never gonna give",
                        "syllables": [
                            {"startTimeMs": "12340", "numChars": "6"},
                            {"startTimeMs": "12900", "numChars": "6"},
                            {"startTimeMs": "13500", "numChars": "4"}
                        ]
                    }
                ]
            }
        }"#;

        let timeline = parse_spotify_color_lyrics(sample, "spotify_color").unwrap();
        assert_eq!(timeline.sync_type, SyncType::Syllable);
        assert_eq!(timeline.lines.len(), 1);
        assert_eq!(timeline.lines[0].words.len(), 3);
        assert_eq!(timeline.lines[0].words[0].text, "Never ");
        assert_eq!(timeline.lines[0].words[0].start_time_ms, 12340);
        assert_eq!(timeline.lines[0].words[1].text, "gonna ");
        assert_eq!(timeline.lines[0].words[2].text, "give");
    }
}
