use crate::lyrics::{KaraokeLine, KaraokeTimeline, KaraokeWord, SyncType, VocalRole};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde_json::Value;

/// Decodes common XML/HTML entities
fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

/// Parses TTML timestamp formats like "00:12.340", "01:23:45.678", "12.34s", or "12345ms" into milliseconds
pub fn parse_ttml_timestamp(s: &str) -> i64 {
    let trimmed = s.trim();

    if let Some(sec_str) = trimmed.strip_suffix('s') {
        if let Ok(secs) = sec_str.parse::<f64>() {
            return (secs * 1000.0) as i64;
        }
    }
    if let Some(ms_str) = trimmed.strip_suffix("ms") {
        if let Ok(ms) = ms_str.parse::<i64>() {
            return ms;
        }
    }

    let parts: Vec<&str> = trimmed.split(':').collect();
    match parts.len() {
        2 => {
            let mins = parts[0].parse::<f64>().unwrap_or(0.0);
            let secs = parts[1].parse::<f64>().unwrap_or(0.0);
            ((mins * 60.0 + secs) * 1000.0) as i64
        }
        3 => {
            let hrs = parts[0].parse::<f64>().unwrap_or(0.0);
            let mins = parts[1].parse::<f64>().unwrap_or(0.0);
            let secs = parts[2].parse::<f64>().unwrap_or(0.0);
            ((hrs * 3600.0 + mins * 60.0 + secs) * 1000.0) as i64
        }
        _ => trimmed.parse::<i64>().unwrap_or(0),
    }
}

/// Parses Apple Music TTML XML lyrics into unified KaraokeTimeline
pub fn parse_apple_music_ttml(xml: &str, provider_name: &str) -> Result<KaraokeTimeline> {
    let p_regex = Regex::new(
        r#"(?s)<p\s+[^>]*begin="([^"]+)"\s+[^>]*end="([^"]+)"([^>]*)>(.*?)</p>"#,
    )
    .map_err(|e| anyhow!("regex error: {}", e))?;

    let span_regex = Regex::new(
        r#"(?s)<span\s+[^>]*begin="([^"]+)"\s+[^>]*end="([^"]+)"[^>]*>(.*?)</span>"#,
    )
    .map_err(|e| anyhow!("regex error: {}", e))?;

    let mut lines = Vec::new();

    for p_cap in p_regex.captures_iter(xml) {
        let p_begin = parse_ttml_timestamp(&p_cap[1]);
        let p_end = parse_ttml_timestamp(&p_cap[2]);
        let p_attrs = &p_cap[3];
        let p_content = &p_cap[4];

        let vocal_role = if p_attrs.contains(r#"ttm:agent="v2""#) {
            VocalRole::Lead2
        } else if p_attrs.contains(r#"ttm:role="x-bg""#) || p_content.contains(r#"ttm:role="x-bg""#) {
            VocalRole::Background
        } else {
            VocalRole::Lead1
        };

        let mut words = Vec::new();
        let mut line_text = String::new();

        for span_cap in span_regex.captures_iter(p_content) {
            let s_begin = parse_ttml_timestamp(&span_cap[1]);
            let s_end = parse_ttml_timestamp(&span_cap[2]);
            let s_text = decode_html_entities(&span_cap[3]);

            line_text.push_str(&s_text);
            words.push(KaraokeWord::new(&s_text, s_begin, s_end));
        }

        // If no spans, strip inner xml tags and use full line text
        if words.is_empty() {
            let tag_regex = Regex::new(r"<[^>]*>").unwrap();
            line_text = tag_regex.replace_all(p_content, "").trim().to_string();
            line_text = decode_html_entities(&line_text);
            words.push(KaraokeWord::new(&line_text, p_begin, p_end));
        }

        lines.push(KaraokeLine {
            text: line_text.trim().to_string(),
            start_time_ms: p_begin,
            end_time_ms: p_end,
            words,
            vocal_role,
            romanization: None,
            translation: None,
        });
    }

    if lines.is_empty() {
        return Err(anyhow!("no lyric lines could be extracted from TTML XML"));
    }

    let has_syllables = lines.iter().any(|l| l.words.len() > 1);

    Ok(KaraokeTimeline {
        provider: provider_name.to_string(),
        sync_type: if has_syllables {
            SyncType::Syllable
        } else {
            SyncType::Line
        },
        lines,
        raw_elrc: None,
    })
}

/// Parses Musixmatch RichSync JSON lyrics into unified KaraokeTimeline
pub fn parse_musixmatch_richsync(json_str: &str, provider_name: &str) -> Result<KaraokeTimeline> {
    let parsed: Value =
        serde_json::from_str(json_str).context("invalid Musixmatch RichSync JSON")?;

    let array = parsed
        .as_array()
        .ok_or_else(|| anyhow!("expected JSON array for RichSync"))?;

    let mut lines = Vec::new();

    for item in array {
        let ts = item["ts"].as_f64().unwrap_or(0.0);
        let te = item["te"].as_f64().unwrap_or(ts + 2.0);

        let line_start_ms = (ts * 1000.0) as i64;
        let line_end_ms = (te * 1000.0) as i64;

        let role_str = item["x"].as_str().unwrap_or("v1");
        let vocal_role = match role_str {
            "v2" => VocalRole::Lead2,
            "x-bg" | "bg" => VocalRole::Background,
            _ => VocalRole::Lead1,
        };

        let mut words = Vec::new();
        let mut line_text = String::new();

        if let Some(word_list) = item["l"].as_array() {
            for (idx, w_item) in word_list.iter().enumerate() {
                let chunk_text = w_item["c"].as_str().unwrap_or("");
                let offset_sec = w_item["o"].as_f64().unwrap_or(0.0);

                let word_start_ms = ((ts + offset_sec) * 1000.0) as i64;

                let word_end_ms = if idx + 1 < word_list.len() {
                    let next_offset = word_list[idx + 1]["o"].as_f64().unwrap_or(offset_sec + 0.3);
                    ((ts + next_offset) * 1000.0) as i64
                } else {
                    line_end_ms
                };

                line_text.push_str(chunk_text);
                words.push(KaraokeWord::new(chunk_text, word_start_ms, word_end_ms));
            }
        }

        lines.push(KaraokeLine {
            text: line_text.trim().to_string(),
            start_time_ms: line_start_ms,
            end_time_ms: line_end_ms,
            words,
            vocal_role,
            romanization: None,
            translation: None,
        });
    }

    if lines.is_empty() {
        return Err(anyhow!("no lyric lines could be extracted from RichSync JSON"));
    }

    Ok(KaraokeTimeline {
        provider: provider_name.to_string(),
        sync_type: SyncType::Syllable,
        lines,
        raw_elrc: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ttml_timestamps() {
        assert_eq!(parse_ttml_timestamp("00:12.340"), 12_340);
        assert_eq!(parse_ttml_timestamp("01:12.500"), 72_500);
        assert_eq!(parse_ttml_timestamp("12.34s"), 12_340);
        assert_eq!(parse_ttml_timestamp("5000ms"), 5_000);
    }

    #[test]
    fn test_parse_apple_music_ttml_with_duet_and_background() {
        let ttml_sample = r#"
        <tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata">
          <body>
            <div>
              <p begin="00:10.000" end="00:14.000" ttm:agent="v1">
                <span begin="00:10.000" end="00:11.500">Never </span>
                <span begin="00:11.500" end="00:14.000">gonna</span>
              </p>
              <p begin="00:15.000" end="00:18.000" ttm:agent="v2">
                <span begin="00:15.000" end="00:18.000">Give you up</span>
              </p>
              <p begin="00:19.000" end="00:22.000" ttm:role="x-bg">
                <span begin="00:19.000" end="00:22.000">(Never gonna give)</span>
              </p>
            </div>
          </body>
        </tt>
        "#;

        let timeline = parse_apple_music_ttml(ttml_sample, "paxsenix_apple_music").unwrap();
        assert_eq!(timeline.sync_type, SyncType::Syllable);
        assert_eq!(timeline.lines.len(), 3);

        // Line 1: Lead1
        assert_eq!(timeline.lines[0].vocal_role, VocalRole::Lead1);
        assert_eq!(timeline.lines[0].words.len(), 2);
        assert_eq!(timeline.lines[0].words[0].text, "Never ");
        assert_eq!(timeline.lines[0].words[0].start_time_ms, 10_000);

        // Line 2: Lead2
        assert_eq!(timeline.lines[1].vocal_role, VocalRole::Lead2);

        // Line 3: Background
        assert_eq!(timeline.lines[2].vocal_role, VocalRole::Background);
    }

    #[test]
    fn test_parse_musixmatch_richsync() {
        let richsync_sample = r#"
        [
          {
            "ts": 12.34,
            "te": 15.67,
            "l": [
              {"c": "Never ", "o": 0.0},
              {"c": "gonna ", "o": 0.5},
              {"c": "give", "o": 1.0}
            ],
            "x": "v1"
          }
        ]
        "#;

        let timeline = parse_musixmatch_richsync(richsync_sample, "musixmatch_richsync").unwrap();
        assert_eq!(timeline.sync_type, SyncType::Syllable);
        assert_eq!(timeline.lines.len(), 1);
        assert_eq!(timeline.lines[0].words.len(), 3);
        assert_eq!(timeline.lines[0].words[0].start_time_ms, 12_340);
        assert_eq!(timeline.lines[0].words[1].start_time_ms, 12_840);
    }
}
