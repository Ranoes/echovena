use crate::lyrics::{KaraokeLine, KaraokeTimeline, KaraokeWord, SyncType, VocalRole};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::Deserialize;

const NETEASE_LYRIC_URL: &str = "https://music.163.com/api/song/lyric";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 EchoVena/0.1.0";

/// Parses NetEase .yrc word-sync lyrics into KaraokeTimeline
pub fn parse_netease_yrc(yrc_text: &str, provider_name: &str) -> Result<KaraokeTimeline> {
    let line_re = Regex::new(r"^\[(\d+),(\d+)\](.*)$").unwrap();
    let word_re = Regex::new(r"\((\d+),(\d+),\d+\)([^\(\]]*)").unwrap();

    let mut lines = Vec::new();

    for raw_line in yrc_text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(caps) = line_re.captures(trimmed) {
            let line_start_ms = caps[1].parse::<i64>().unwrap_or(0);
            let line_dur_ms = caps[2].parse::<i64>().unwrap_or(3000);
            let line_end_ms = line_start_ms + line_dur_ms;

            let line_content = &caps[3];
            let mut words = Vec::new();
            let mut full_text = String::new();

            for w_cap in word_re.captures_iter(line_content) {
                let w_start_ms = w_cap[1].parse::<i64>().unwrap_or(line_start_ms);
                let w_dur_ms = w_cap[2].parse::<i64>().unwrap_or(300);
                let w_text = &w_cap[3];

                full_text.push_str(w_text);
                words.push(KaraokeWord::new(w_text, w_start_ms, w_start_ms + w_dur_ms));
            }

            if words.is_empty() {
                full_text = line_content.to_string();
                words.push(KaraokeWord::new(&full_text, line_start_ms, line_end_ms));
            }

            lines.push(KaraokeLine {
                text: full_text,
                start_time_ms: line_start_ms,
                end_time_ms: line_end_ms,
                words,
                vocal_role: VocalRole::Lead1,
                romanization: None,
                translation: None,
            });
        }
    }

    if lines.is_empty() {
        return Err(anyhow!("no .yrc lines parsed"));
    }

    Ok(KaraokeTimeline {
        provider: provider_name.to_string(),
        sync_type: SyncType::Syllable,
        lines,
        raw_elrc: None,
    })
}

#[derive(Debug, Deserialize)]
struct NeteaseLyricResponse {
    #[serde(default)]
    yrc: Option<NeteaseLyricField>,
    #[serde(default)]
    lrc: Option<NeteaseLyricField>,
    #[serde(default)]
    tlyric: Option<NeteaseLyricField>,
}

#[derive(Debug, Deserialize)]
struct NeteaseLyricField {
    lyric: Option<String>,
}

#[derive(Clone)]
pub struct NeteaseLyricsClient {
    client: reqwest::Client,
    base_url: String,
}

impl Default for NeteaseLyricsClient {
    fn default() -> Self {
        Self::new(NETEASE_LYRIC_URL)
    }
}

impl NeteaseLyricsClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::builder().build().unwrap(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Fetches lyrics from NetEase API for a given NetEase track ID
    pub async fn fetch_lyrics(&self, song_id: i64) -> Result<KaraokeTimeline> {
        let resp = self
            .client
            .get(&self.base_url)
            .query(&[
                ("id", &song_id.to_string()),
                ("lv", &"1".to_string()),
                ("kv", &"1".to_string()),
                ("tv", &"1".to_string()),
                ("yv", &"1".to_string()),
            ])
            .header(reqwest::header::USER_AGENT, DEFAULT_USER_AGENT)
            .send()
            .await
            .context("failed sending NetEase lyric request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("NetEase API returned status {}", resp.status()));
        }

        let body: NeteaseLyricResponse = resp.json().await?;

        // 1. Try word-sync .yrc first
        if let Some(yrc_field) = body.yrc {
            if let Some(yrc_txt) = yrc_field.lyric {
                if !yrc_txt.trim().is_empty() {
                    let mut timeline = parse_netease_yrc(&yrc_txt, "netease_yrc")?;

                    // Inject translation lines if present
                    if let Some(t_field) = body.tlyric {
                        if let Some(t_txt) = t_field.lyric {
                            merge_translations(&mut timeline, &t_txt);
                        }
                    }

                    return Ok(timeline);
                }
            }
        }

        // 2. Fall back to standard LRC
        if let Some(lrc_field) = body.lrc {
            if let Some(lrc_txt) = lrc_field.lyric {
                if !lrc_txt.trim().is_empty() {
                    return crate::lyrics::lrclib_elrc::parse_enhanced_lrc(&lrc_txt, "netease_lrc");
                }
            }
        }

        Err(anyhow!("no lyrics found on NetEase for song {}", song_id))
    }
}

/// Helper to merge translated lines into timeline based on line timestamp proximity
fn merge_translations(timeline: &mut KaraokeTimeline, trans_lrc: &str) {
    let re = Regex::new(r"^\[(\d{1,2}:\d{2}(?:\.\d{1,3})?)\](.*)$").unwrap();
    let mut trans_map = Vec::new();

    for line in trans_lrc.lines() {
        if let Some(caps) = re.captures(line.trim()) {
            if let Some(ms) = crate::lyrics::lrclib_elrc::parse_lrc_timestamp(&caps[1]) {
                let text = caps[2].trim().to_string();
                if !text.is_empty() {
                    trans_map.push((ms, text));
                }
            }
        }
    }

    for line in &mut timeline.lines {
        if let Some((_, text)) = trans_map
            .iter()
            .find(|(ms, _)| (*ms - line.start_time_ms).abs() <= 1000)
        {
            line.translation = Some(text.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_netease_yrc() {
        let sample_yrc = r#"
        [12340,3200](12340,360,0)Ne(12700,400,0)ver (13100,300,0)gon(13400,300,0)na
        [16000,2500](16000,500,0)give (16500,500,0)you (17000,600,0)up
        "#;

        let timeline = parse_netease_yrc(sample_yrc, "netease_yrc").unwrap();
        assert_eq!(timeline.sync_type, SyncType::Syllable);
        assert_eq!(timeline.lines.len(), 2);

        // Line 1
        assert_eq!(timeline.lines[0].start_time_ms, 12_340);
        assert_eq!(timeline.lines[0].end_time_ms, 15_540);
        assert_eq!(timeline.lines[0].words.len(), 4);
        assert_eq!(timeline.lines[0].words[0].text, "Ne");
        assert_eq!(timeline.lines[0].words[0].start_time_ms, 12_340);
        assert_eq!(timeline.lines[0].words[0].end_time_ms, 12_700);

        // Line 2
        assert_eq!(timeline.lines[1].start_time_ms, 16_000);
        assert_eq!(timeline.lines[1].words.len(), 3);
        assert_eq!(timeline.lines[1].words[0].text, "give ");
    }
}
