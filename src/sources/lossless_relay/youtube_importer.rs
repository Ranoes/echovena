use crate::sources::lossless_relay::db::{Database, ImportedPlaylist, PlaylistTrack};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command;

const YTM_BROWSE_URL: &str = "https://music.youtube.com/youtubei/v1/browse";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YouTubeResource {
    Playlist(String),
    Video(String),
}

/// Parses any YouTube or YouTube Music URL or ID into a canonical YouTubeResource
pub fn parse_youtube_url(input: &str) -> Option<YouTubeResource> {
    let trimmed = input.trim();

    // Direct ID or custom URI
    if let Some(id) = trimmed.strip_prefix("yt:playlist:") {
        return Some(YouTubeResource::Playlist(id.to_string()));
    }
    if let Some(id) = trimmed.strip_prefix("yt:video:") {
        return Some(YouTubeResource::Video(id.to_string()));
    }

    // YouTu.be short URL: https://youtu.be/<videoId>?...
    if trimmed.contains("youtu.be/") {
        let path = trimmed
            .split('?')
            .next()?
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_start_matches("youtu.be/")
            .trim_matches('/');
        if !path.is_empty() {
            return Some(YouTubeResource::Video(path.to_string()));
        }
    }

    // Standard YouTube / YouTube Music URL
    if trimmed.contains("youtube.com/") {
        let query_part = trimmed.split('?').nth(1)?;
        let mut video_id: Option<String> = None;
        let mut list_id: Option<String> = None;

        for pair in query_part.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "list" && !v.is_empty() {
                    list_id = Some(v.to_string());
                } else if (k == "v" || k == "video_id") && !v.is_empty() {
                    video_id = Some(v.to_string());
                }
            }
        }

        // Playlist takes precedence if present in playlist context, otherwise video
        if let Some(list) = list_id {
            return Some(YouTubeResource::Playlist(list));
        }
        if let Some(v) = video_id {
            return Some(YouTubeResource::Video(v));
        }
    }

    None
}

/// Parses duration strings like "3:45", "03:45", or "1:02:30" into milliseconds
pub fn parse_duration_str(duration_str: &str) -> i64 {
    let parts: Vec<&str> = duration_str.trim().split(':').collect();
    match parts.len() {
        1 => parts[0].parse::<i64>().unwrap_or(0) * 1000,
        2 => {
            let mins = parts[0].parse::<i64>().unwrap_or(0);
            let secs = parts[1].parse::<i64>().unwrap_or(0);
            (mins * 60 + secs) * 1000
        }
        3 => {
            let hrs = parts[0].parse::<i64>().unwrap_or(0);
            let mins = parts[1].parse::<i64>().unwrap_or(0);
            let secs = parts[2].parse::<i64>().unwrap_or(0);
            (hrs * 3600 + mins * 60 + secs) * 1000
        }
        _ => 0,
    }
}

/// Sanitizes YouTube video titles and uploader names to isolate clean Artist and Title
pub fn clean_youtube_title(raw_title: &str, uploader: Option<&str>) -> (String, String) {
    let mut title = raw_title.to_string();

    // 1. Strip common noise tags in parentheses or brackets (case-insensitive)
    let noise_patterns = [
        r"(?i)\s*\[\s*(official\s*(music\s*)?video|official\s*audio|official\s*hd\s*video|official\s*4k\s*video|lyric\s*video|official\s*lyric\s*video|mv|hd|4k|visualizer|audio|lyrics?)\s*\]",
        r"(?i)\s*\(\s*(official\s*(music\s*)?video|official\s*audio|official\s*hd\s*video|official\s*4k\s*(remaster|video)?|lyric\s*video|official\s*lyric\s*video|mv|hd|4k|visualizer|audio|lyrics?|performance\s*video|dance\s*practice)\s*\)",
        r"(?i)\s*\[\s*(explicit|clean|remastered|4k\s*remaster)\s*\]",
        r"(?i)\s*\(\s*(explicit|clean|remastered|4k\s*remaster)\s*\)",
        r"(?i)\s*\b(ft\.|feat\.)\s+[^\(\[\-]+",
    ];

    for pat in &noise_patterns {
        if let Ok(re) = Regex::new(pat) {
            title = re.replace_all(&title, "").to_string();
        }
    }

    title = title.trim().to_string();

    // 2. Clean channel/uploader name
    let clean_uploader = uploader.map(|u| {
        let mut c = u.to_string();
        let uploader_suffixes = [
            r"(?i)\s*-\s*topic\b",
            r"(?i)vevo\b",
            r"(?i)\s+official\b",
            r"(?i)\s+records\b",
            r"(?i)\s+music\b",
        ];
        for suf in &uploader_suffixes {
            if let Ok(re) = Regex::new(suf) {
                c = re.replace_all(&c, "").to_string();
            }
        }
        c.trim().to_string()
    });

    // 3. Check for hyphenated "Artist - Title" or "Artist — Title" or "Artist : Title"
    let separators = [" - ", " – ", " — ", " : "];
    for sep in &separators {
        if let Some((left, right)) = title.split_once(sep) {
            let candidate_artist = left.trim().trim_matches('"').trim_matches('\'').trim();
            let mut candidate_title = right.trim().trim_matches('"').trim_matches('\'').trim();

            // Additional cleanup on candidate title (e.g. trailing tags)
            if let Ok(re) = Regex::new(r#"(?i)\s*["']?\s*(official\s*video|mv|hd|audio)\s*["']?$"#) {
                candidate_title = candidate_title.trim();
                let cleaned = re.replace(candidate_title, "").to_string();
                if !candidate_artist.is_empty() && !cleaned.is_empty() {
                    return (candidate_artist.to_string(), cleaned.trim().to_string());
                }
            }

            if !candidate_artist.is_empty() && !candidate_title.is_empty() {
                return (
                    candidate_artist.to_string(),
                    candidate_title.to_string(),
                );
            }
        }
    }

    // 4. If no separator pattern matched, use cleaned uploader as artist (if available)
    let artist = clean_uploader.unwrap_or_else(|| "Unknown Artist".to_string());
    (artist, title)
}

#[derive(Clone)]
pub struct YouTubeImporter {
    client: reqwest::Client,
}

impl Default for YouTubeImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl YouTubeImporter {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder().build().unwrap(),
        }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Fetches a playlist and its tracks using YouTube Music's InnerTube API, falling back to yt-dlp
    pub async fn fetch_playlist(&self, playlist_id: &str) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        // Attempt Primary: InnerTube API
        match self.fetch_innertube_playlist(playlist_id).await {
            Ok(result) => {
                if !result.1.is_empty() {
                    return Ok(result);
                }
            }
            Err(e) => {
                log::warn!(
                    "InnerTube playlist fetch for {} failed: {}. Falling back to yt-dlp.",
                    playlist_id,
                    e
                );
            }
        }

        // Attempt Fallback: yt-dlp JSON dump
        self.fetch_ytdlp_playlist(playlist_id).await
    }

    /// Primary Extraction: YouTube Music InnerTube browse endpoint
    async fn fetch_innertube_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let browse_id = if playlist_id.starts_with("VL") {
            playlist_id.to_string()
        } else {
            format!("VL{}", playlist_id)
        };

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20240401.01.00",
                    "hl": "en",
                    "gl": "US"
                }
            },
            "browseId": browse_id
        });

        let resp = self
            .client
            .post(YTM_BROWSE_URL)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .context("failed to execute InnerTube browse request")?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "InnerTube browse request returned status {}",
                resp.status()
            ));
        }

        let body: Value = resp.json().await.context("failed to parse InnerTube JSON response")?;

        // Extract playlist title and author
        let header = &body["header"]["musicDetailHeaderRenderer"];
        let title = header["title"]["runs"][0]["text"]
            .as_str()
            .or_else(|| body["header"]["musicResponsiveHeaderRenderer"]["title"]["runs"][0]["text"].as_str())
            .unwrap_or("YouTube Playlist")
            .to_string();

        let author = header["subtitle"]["runs"]
            .as_array()
            .and_then(|runs| runs.first())
            .and_then(|r| r["text"].as_str())
            .map(|s| s.to_string());

        let cover_art_url = header["thumbnail"]["croppedSquareThumbnailRenderer"]["thumbnail"]["thumbnails"]
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|t| t["url"].as_str())
            .map(|s| s.to_string());

        let now = Utc::now().timestamp();
        let canonical_id = format!("yt:playlist:{}", playlist_id);

        let mut tracks = Vec::new();

        // Extract items from musicShelfRenderer or musicPlaylistShelfRenderer
        let contents = &body["contents"]["singleColumnBrowseResultsRenderer"]["tabs"][0]
            ["tabRenderer"]["content"]["sectionListRenderer"]["contents"][0];

        let items = contents["musicResponsiveHeaderRenderer"].as_object().and_then(|_| {
            contents["musicPlaylistShelfRenderer"]["contents"].as_array()
        }).or_else(|| {
            contents["musicShelfRenderer"]["contents"].as_array()
        }).or_else(|| {
            body["contents"]["twoColumnBrowseResultsRenderer"]["secondaryContents"]
                ["sectionListRenderer"]["contents"][0]["musicPlaylistShelfRenderer"]["contents"]
                .as_array()
        });

        if let Some(item_list) = items {
            for (idx, item) in item_list.iter().enumerate() {
                let renderer = &item["musicResponsiveListItemRenderer"];
                if renderer.is_null() {
                    continue;
                }

                let video_id = renderer["playlistItemData"]["videoId"]
                    .as_str()
                    .or_else(|| {
                        renderer["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]
                            ["musicPlayButtonRenderer"]["playNavigationEndpoint"]["watchEndpoint"]
                            ["videoId"]
                            .as_str()
                    });

                let Some(vid) = video_id else {
                    continue;
                };

                let flex_cols = renderer["flexColumns"].as_array();
                let raw_title = flex_cols
                    .and_then(|cols| cols.get(0))
                    .and_then(|col| {
                        col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"]
                            .as_str()
                    })
                    .unwrap_or("Unknown Title");

                let artist_name = flex_cols
                    .and_then(|cols| cols.get(1))
                    .and_then(|col| {
                        col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"]
                            .as_str()
                    });

                let duration_text = flex_cols
                    .and_then(|cols| cols.get(1))
                    .and_then(|col| {
                        col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
                            .as_array()
                    })
                    .and_then(|runs| runs.last())
                    .and_then(|r| r["text"].as_str())
                    .unwrap_or("0:00");

                let (clean_artist, clean_title) = clean_youtube_title(raw_title, artist_name);
                let duration_ms = parse_duration_str(duration_text);

                let track_cover = renderer["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]
                    ["thumbnails"]
                    .as_array()
                    .and_then(|arr| arr.last())
                    .and_then(|t| t["url"].as_str())
                    .map(|s| s.to_string());

                tracks.push(PlaylistTrack {
                    playlist_id: canonical_id.clone(),
                    position: idx as i64,
                    source_track_id: format!("yt:video:{}", vid),
                    raw_title: raw_title.to_string(),
                    clean_title,
                    artist: clean_artist,
                    album: None,
                    duration_ms,
                    isrc: None,
                    cover_art_url: track_cover,
                });
            }
        }

        let playlist = ImportedPlaylist {
            id: canonical_id,
            platform: "ytmusic".to_string(),
            title,
            author,
            source_url: format!("https://music.youtube.com/playlist?list={}", playlist_id),
            cover_art_url,
            snapshot_id: None,
            track_count: tracks.len() as i64,
            last_synced_at: now,
        };

        Ok((playlist, tracks))
    }

    /// Fallback Extraction: yt-dlp --flat-playlist --dump-json
    async fn fetch_ytdlp_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let url = format!("https://www.youtube.com/playlist?list={}", playlist_id);

        let output = Command::new("yt-dlp")
            .arg("--flat-playlist")
            .arg("--dump-json")
            .arg("--no-warnings")
            .arg(&url)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .with_context(|| format!("failed to spawn yt-dlp for playlist {}", playlist_id))?;

        if !output.status.success() {
            return Err(anyhow!(
                "yt-dlp exited with non-zero status for playlist {}",
                playlist_id
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut tracks = Vec::new();
        let canonical_id = format!("yt:playlist:{}", playlist_id);
        let now = Utc::now().timestamp();
        let mut playlist_title = "YouTube Playlist".to_string();
        let mut playlist_uploader = None;

        for (idx, line) in stdout.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Ok(entry) = serde_json::from_str::<Value>(line) {
                if idx == 0 {
                    if let Some(pt) = entry["playlist_title"].as_str() {
                        playlist_title = pt.to_string();
                    }
                    if let Some(pu) = entry["playlist_uploader"].as_str() {
                        playlist_uploader = Some(pu.to_string());
                    }
                }

                let vid = entry["id"].as_str().unwrap_or("").to_string();
                if vid.is_empty() {
                    continue;
                }

                let raw_title = entry["title"].as_str().unwrap_or("Unknown Title");
                let uploader = entry["uploader"].as_str().or_else(|| entry["channel"].as_str());
                let duration_secs = entry["duration"].as_f64().unwrap_or(0.0);
                let duration_ms = (duration_secs * 1000.0) as i64;

                let (clean_artist, clean_title) = clean_youtube_title(raw_title, uploader);

                let cover_art_url = entry["thumbnails"]
                    .as_array()
                    .and_then(|arr| arr.last())
                    .and_then(|t| t["url"].as_str())
                    .map(|s| s.to_string());

                tracks.push(PlaylistTrack {
                    playlist_id: canonical_id.clone(),
                    position: idx as i64,
                    source_track_id: format!("yt:video:{}", vid),
                    raw_title: raw_title.to_string(),
                    clean_title,
                    artist: clean_artist,
                    album: None,
                    duration_ms,
                    isrc: None,
                    cover_art_url,
                });
            }
        }

        let playlist = ImportedPlaylist {
            id: canonical_id,
            platform: "youtube".to_string(),
            title: playlist_title,
            author: playlist_uploader,
            source_url: url,
            cover_art_url: None,
            snapshot_id: None,
            track_count: tracks.len() as i64,
            last_synced_at: now,
        };

        Ok((playlist, tracks))
    }

    /// Fetches and saves a YouTube playlist into the local SQLite database
    pub async fn import_playlist(
        &self,
        db: &Database,
        url_or_id: &str,
    ) -> Result<ImportedPlaylist> {
        let resource = parse_youtube_url(url_or_id)
            .ok_or_else(|| anyhow!("invalid YouTube URL or ID: {}", url_or_id))?;

        let playlist_id = match resource {
            YouTubeResource::Playlist(id) => id,
            YouTubeResource::Video(_) => {
                return Err(anyhow!("expected a YouTube playlist URL, found single video"));
            }
        };

        let (playlist, tracks) = self.fetch_playlist(&playlist_id).await?;

        db.upsert_imported_playlist(&playlist)
            .context("failed to insert imported playlist into database")?;
        db.replace_playlist_tracks(&playlist.id, &tracks)
            .context("failed to insert playlist tracks into database")?;

        Ok(playlist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_youtube_urls() {
        assert_eq!(
            parse_youtube_url("https://music.youtube.com/playlist?list=PL4fGSI1pDJn5kI81J1S6LUVbZbTflVfI7"),
            Some(YouTubeResource::Playlist("PL4fGSI1pDJn5kI81J1S6LUVbZbTflVfI7".to_string()))
        );

        assert_eq!(
            parse_youtube_url("https://www.youtube.com/playlist?list=PLMC9KNkIncKtPcgxx-5W46g1aX8b-b8r4"),
            Some(YouTubeResource::Playlist("PLMC9KNkIncKtPcgxx-5W46g1aX8b-b8r4".to_string()))
        );

        assert_eq!(
            parse_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLMC9KNkIncKtPcgxx-5W46g1aX8b-b8r4"),
            Some(YouTubeResource::Playlist("PLMC9KNkIncKtPcgxx-5W46g1aX8b-b8r4".to_string()))
        );

        assert_eq!(
            parse_youtube_url("https://youtu.be/dQw4w9WgXcQ"),
            Some(YouTubeResource::Video("dQw4w9WgXcQ".to_string()))
        );

        assert_eq!(
            parse_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some(YouTubeResource::Video("dQw4w9WgXcQ".to_string()))
        );

        assert_eq!(
            parse_youtube_url("yt:playlist:PL12345"),
            Some(YouTubeResource::Playlist("PL12345".to_string()))
        );

        assert_eq!(parse_youtube_url("https://open.spotify.com/track/12345"), None);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration_str("3:45"), 225_000);
        assert_eq!(parse_duration_str("03:45"), 225_000);
        assert_eq!(parse_duration_str("1:02:30"), 3_750_000);
        assert_eq!(parse_duration_str("45"), 45_000);
    }

    #[test]
    fn test_clean_youtube_title_sanitizer() {
        // PRD requirement: "Daft Punk - Get Lucky (Official Audio)" -> ("Daft Punk", "Get Lucky")
        let (artist, title) = clean_youtube_title(
            "Daft Punk - Get Lucky (Official Audio)",
            Some("DaftPunkVEVO"),
        );
        assert_eq!(artist, "Daft Punk");
        assert_eq!(title, "Get Lucky");

        // Noise tag [Official Music Video] and VEVO uploader
        let (artist, title) = clean_youtube_title(
            "Rick Astley - Never Gonna Give You Up [Official Music Video]",
            Some("RickAstleyVEVO"),
        );
        assert_eq!(artist, "Rick Astley");
        assert_eq!(title, "Never Gonna Give You Up");

        // Em-dash separator and [HD]
        let (artist, title) = clean_youtube_title(
            "Queen – Bohemian Rhapsody [HD]",
            Some("Queen Official"),
        );
        assert_eq!(artist, "Queen");
        assert_eq!(title, "Bohemian Rhapsody");

        // Topic uploader without hyphen
        let (artist, title) = clean_youtube_title(
            "Starboy",
            Some("The Weeknd - Topic"),
        );
        assert_eq!(artist, "The Weeknd");
        assert_eq!(title, "Starboy");

        // Visualizer tag
        let (artist, title) = clean_youtube_title(
            "Radiohead - Creep (Visualizer)",
            Some("Radiohead"),
        );
        assert_eq!(artist, "Radiohead");
        assert_eq!(title, "Creep");
    }
}
