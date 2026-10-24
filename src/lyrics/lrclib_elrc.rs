use crate::lyrics::{KaraokeLine, KaraokeTimeline, KaraokeWord, SyncType, VocalRole};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::Deserialize;

const LRCLIB_API_URL: &str = "https://lrclib.net/api/get";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 EchoVena/0.1.0";

/// Converts an LRC timestamp string like "01:23.45" or "01:23.456" into milliseconds
pub fn parse_lrc_timestamp(ts: &str) -> Option<i64> {
    let clean = ts.trim_matches(|c| c == '[' || c == ']' || c == '<' || c == '>');
    let (min_str, sec_str) = clean.split_once(':')?;

    let mins = min_str.parse::<i64>().ok()?;
    let secs = sec_str.parse::<f64>().ok()?;

    Some((mins * 60_000) + (secs * 1000.0).round() as i64)
}

/// Parses Enhanced LRC (.elrc) or standard line-synced .lrc text into KaraokeTimeline
pub fn parse_enhanced_lrc(lrc_text: &str, provider_name: &str) -> Result<KaraokeTimeline> {
    let line_tag_re = Regex::new(r"^\[(\d{1,2}:\d{2}(?:\.\d{1,3})?)\](.*)$").unwrap();
    let word_tag_re = Regex::new(r"<(\d{1,2}:\d{2}(?:\.\d{1,3})?)>([^<]*)").unwrap();

    let mut lines = Vec::new();
    let mut has_syllables = false;

    for raw_line in lrc_text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(caps) = line_tag_re.captures(trimmed) {
            let line_start_ms = match parse_lrc_timestamp(&caps[1]) {
                Some(ms) => ms,
                None => continue,
            };

            let line_content = caps[2].trim();
            let mut words = Vec::new();
            let mut full_text = String::new();

            for w_cap in word_tag_re.captures_iter(line_content) {
                if let Some(w_start) = parse_lrc_timestamp(&w_cap[1]) {
                    let w_text = &w_cap[2];
                    full_text.push_str(w_text);
                    words.push(KaraokeWord::new(w_text, w_start, w_start + 400));
                    has_syllables = true;
                }
            }

            // Fix word end times to match the start of the subsequent word
            if !words.is_empty() {
                for i in 0..words.len() - 1 {
                    words[i].end_time_ms = words[i + 1].start_time_ms;
                }
            } else {
                full_text = line_content.to_string();
                words.push(KaraokeWord::new(&full_text, line_start_ms, line_start_ms + 3000));
            }

            lines.push(KaraokeLine {
                text: full_text,
                start_time_ms: line_start_ms,
                end_time_ms: line_start_ms + 3000,
                words,
                vocal_role: VocalRole::Lead1,
                romanization: None,
                translation: None,
            });
        }
    }

    if lines.is_empty() {
        return Err(anyhow!("no timestamped lines parsed from LRC"));
    }

    // Fix line end times to match start of next line
    for i in 0..lines.len() - 1 {
        let next_start = lines[i + 1].start_time_ms;
        lines[i].end_time_ms = next_start;
        let words_len = lines[i].words.len();
        if let Some(last_word) = lines[i].words.last_mut() {
            if last_word.end_time_ms > next_start || words_len == 1 {
                last_word.end_time_ms = next_start;
            }
        }
    }

    Ok(KaraokeTimeline {
        provider: provider_name.to_string(),
        sync_type: if has_syllables {
            SyncType::Syllable
        } else {
            SyncType::Line
        },
        lines,
        raw_elrc: Some(lrc_text.to_string()),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibResponse {
    synced_lyrics: Option<String>,
}

#[derive(Clone)]
pub struct LrclibClient {
    client: reqwest::Client,
    base_url: String,
}

impl Default for LrclibClient {
    fn default() -> Self {
        Self::new(LRCLIB_API_URL)
    }
}

impl LrclibClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::builder().build().unwrap(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Fetches synced lyrics from LRCLIB API by title, artist, album, and duration
    pub async fn fetch_lyrics(
        &self,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration_sec: Option<i64>,
    ) -> Result<KaraokeTimeline> {
        let mut query = vec![("track_name", title), ("artist_name", artist)];
        let album_str;
        if let Some(a) = album {
            album_str = a;
            query.push(("album_name", album_str));
        }
        let dur_str;
        if let Some(d) = duration_sec {
            dur_str = d.to_string();
            query.push(("duration", &dur_str));
        }

        let resp = self
            .client
            .get(&self.base_url)
            .query(&query)
            .header(reqwest::header::USER_AGENT, DEFAULT_USER_AGENT)
            .send()
            .await
            .context("failed sending LRCLIB request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("LRCLIB API returned status {}", resp.status()));
        }

        let body: LrclibResponse = resp.json().await?;
        if let Some(synced) = body.synced_lyrics {
            if !synced.trim().is_empty() {
                return parse_enhanced_lrc(&synced, "lrclib");
            }
        }

        Err(anyhow!("no synced lyrics found on LRCLIB"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_enhanced_lrc_syllables() {
        let elrc = r#"
        [00:12.34] <00:12.34>Ne<00:12.60>ver <00:12.90>gon<00:13.10>na <00:13.40>give
        [00:15.00] <00:15.00>you <00:15.50>up
        "#;

        let timeline = parse_enhanced_lrc(elrc, "lrclib").unwrap();
        assert_eq!(timeline.sync_type, SyncType::Syllable);
        assert_eq!(timeline.lines.len(), 2);
        assert_eq!(timeline.lines[0].start_time_ms, 12_340);
        assert_eq!(timeline.lines[0].words.len(), 5);
        assert_eq!(timeline.lines[0].words[0].text, "Ne");
        assert_eq!(timeline.lines[0].words[0].start_time_ms, 12_340);
        assert_eq!(timeline.lines[0].words[0].end_time_ms, 12_600);
        assert_eq!(timeline.lines[0].words[1].text, "ver ");
        assert_eq!(timeline.lines[0].words[1].start_time_ms, 12_600);
    }

    #[test]
    fn test_parse_standard_lrc_lines() {
        let lrc = r#"
        [00:10.50] First line of song
        [00:14.20] Second line of song
        "#;

        let timeline = parse_enhanced_lrc(lrc, "lrclib").unwrap();
        assert_eq!(timeline.sync_type, SyncType::Line);
        assert_eq!(timeline.lines.len(), 2);
        assert_eq!(timeline.lines[0].start_time_ms, 10_500);
        assert_eq!(timeline.lines[0].end_time_ms, 14_200);
        assert_eq!(timeline.lines[0].text, "First line of song");
    }
}
