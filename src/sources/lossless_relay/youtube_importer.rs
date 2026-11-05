use crate::core::app::AlbumDetailHeader;
use crate::core::plugin_api::TrackInfo;
use crate::sources::lossless_relay::db::{Database, ImportedPlaylist, PlaylistTrack};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command;

const YTM_BROWSE_URL: &str = "https://music.youtube.com/youtubei/v1/browse";
const YTM_NEXT_URL: &str = "https://music.youtube.com/youtubei/v1/next";
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
    if let Some(id) = trimmed
        .strip_prefix("youtube:playlist:")
        .or_else(|| trimmed.strip_prefix("yt:playlist:"))
    {
        return Some(YouTubeResource::Playlist(id.to_string()));
    }
    if let Some(id) = trimmed
        .strip_prefix("youtube:video:")
        .or_else(|| trimmed.strip_prefix("yt:video:"))
        .or_else(|| trimmed.strip_prefix("youtube:"))
        .or_else(|| trimmed.strip_prefix("yt:"))
    {
        return Some(YouTubeResource::Video(id.to_string()));
    }
    if (trimmed.starts_with("PL") || trimmed.starts_with("OLAK5uy_") || trimmed.starts_with("RDCLAK5uy_") || trimmed.starts_with("VLPL"))
        && !trimmed.contains('/') && !trimmed.contains(' ')
    {
        return Some(YouTubeResource::Playlist(trimmed.to_string()));
    }

    // YouTu.be short URL: https://youtu.be/<videoId>?list=...
    if trimmed.contains("youtu.be") {
        let parse_target = if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            format!("https://{}", trimmed)
        } else {
            trimmed.to_string()
        };
        if let Ok(url) = url::Url::parse(&parse_target) {
            for (k, v) in url.query_pairs() {
                if k == "list" && !v.is_empty() {
                    return Some(YouTubeResource::Playlist(v.to_string()));
                }
            }
            let path = url.path().trim_start_matches('/');
            let video_id = path.split('?').next().unwrap_or(path).trim_matches('/');
            if !video_id.is_empty() {
                return Some(YouTubeResource::Video(video_id.to_string()));
            }
        }
    }

    // Standard YouTube / YouTube Music URL
    if trimmed.contains("youtube.com") {
        let parse_target = if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            format!("https://{}", trimmed)
        } else {
            trimmed.to_string()
        };
        if let Ok(url) = url::Url::parse(&parse_target) {
            let mut video_id: Option<String> = None;
            let mut list_id: Option<String> = None;

            for (k, v) in url.query_pairs() {
                if k == "list" && !v.is_empty() {
                    list_id = Some(v.to_string());
                } else if (k == "v" || k == "video_id") && !v.is_empty() {
                    video_id = Some(v.to_string());
                }
            }

            // Also check path prefixes: /playlist/<id> or /browse/VL<id>
            let path = url.path().trim_start_matches('/');
            if list_id.is_none() {
                if let Some(rest) = path.strip_prefix("playlist/") {
                    let clean = rest.split('?').next().unwrap_or(rest).trim_matches('/');
                    if !clean.is_empty() {
                        list_id = Some(clean.to_string());
                    }
                } else if let Some(rest) = path.strip_prefix("browse/VL") {
                    let clean = rest.split('?').next().unwrap_or(rest).trim_matches('/');
                    if !clean.is_empty() {
                        list_id = Some(clean.to_string());
                    }
                }
            }

            if let Some(list) = list_id {
                return Some(YouTubeResource::Playlist(list));
            }
            if let Some(v) = video_id {
                return Some(YouTubeResource::Video(v));
            }
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
    let mut title = raw_title.trim().to_string();

    // 1. Strip common noise tags in any brackets: (), [], 【】, （）, ［］, 《》, 〔〕
    let noise_patterns = [
        r"(?i)\s*[\[\(\【\（\［\《\〔]\s*(official\s*(music\s*)?video|official\s*audio|official\s*hd\s*video|official\s*4k\s*(remaster|video)?|lyric\s*video|official\s*lyric\s*video|mv|pv|hd|4k|visualizer|audio|lyrics?|performance\s*video|dance\s*practice|dance\s*video|special\s*video|teaser|explicit|clean|remastered|4k\s*remaster|music\s*video|公式|歌詞|clip|side\s*[a-z0-9]+|ver\.|version)\s*[\]\)\】\）\］\》\〕]",
        r"(?i)\s*\b(ft\.|feat\.)\s+[^\(\[\-【（]+",
    ];

    for pat in &noise_patterns {
        if let Ok(re) = Regex::new(pat) {
            title = re.replace_all(&title, "").to_string();
        }
    }

    // 2. Strip trailing / leading unbracketed noise tags
    if let Ok(re) = Regex::new(r"(?i)\s*[-–—:/]?\s*(official\s*(music\s*)?video|official\s*audio|official\s*hd\s*video|performance\s*video|dance\s*practice|music\s*video|lyric\s*video|visualizer|-?music\s*clip-?|mv|pv|special\s*video)\s*$") {
        title = re.replace(&title, "").to_string();
    }
    if let Ok(re) = Regex::new(r"^(?i)\s*(mv|pv|official\s*video)\s*[-–—:/／]?\s*") {
        title = re.replace(&title, "").to_string();
    }

    title = title.trim().to_string();

    // 3. Clean channel/uploader name
    let clean_uploader = uploader.map(|u| {
        let mut c = u.to_string();
        let uploader_suffixes = [
            r"(?i)\s*-\s*topic\b",
            r"(?i)vevo\b",
            r"(?i)\s+official(\s+(youtube\s+)?channel)?\b",
            r"(?i)\s+channel\b",
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

    // 4. Japanese quote patterns: Artist「Title」 or Artist『Title』
    if let Ok(re) = Regex::new(r#"^([^「『【\(\[\s]+.*?)\s*[「『]([^」』]+)[」』]"#) {
        if let Some(caps) = re.captures(&title) {
            let candidate_artist = caps[1].trim().trim_matches(|c: char| c == '-' || c == '—' || c == '–' || c == '/' || c == '／' || c == '"' || c == '\'').trim();
            let candidate_title = caps[2].trim().trim_matches(|c: char| c == '"' || c == '\'' || c == '【' || c == '】').trim();
            if !candidate_artist.is_empty() && !candidate_title.is_empty() {
                return (candidate_artist.to_string(), candidate_title.to_string());
            }
        }
    }

    // Japanese quote reverse pattern: 「Title」/ Artist or 『Title』- Artist
    if let Ok(re) = Regex::new(r#"^[「『]([^」』]+)[」』]\s*[-–—:/／]\s*(.*)$"#) {
        if let Some(caps) = re.captures(&title) {
            let candidate_title = caps[1].trim().trim_matches(|c: char| c == '"' || c == '\'' || c == '【' || c == '】').trim();
            let candidate_artist = caps[2].trim().trim_matches(|c: char| c == '-' || c == '—' || c == '–' || c == '/' || c == '／' || c == '"' || c == '\'').trim();
            if !candidate_artist.is_empty() && !candidate_title.is_empty() {
                return (candidate_artist.to_string(), candidate_title.to_string());
            }
        }
    }

    // 5. Quote patterns: Artist 'Title' or Artist "Title"
    if let Ok(re) = Regex::new(r#"^([^'"\(\[]+?)\s*['"]([^'"]+)['"]"#) {
        if let Some(caps) = re.captures(&title) {
            let mut candidate_artist = caps[1].trim().to_string();
            let candidate_title = caps[2].trim();
            if let Some((main_art, _)) = candidate_artist.split_once('(') {
                if !main_art.trim().is_empty() {
                    candidate_artist = main_art.trim().to_string();
                }
            }
            if !candidate_artist.is_empty() && !candidate_title.is_empty() {
                return (candidate_artist, candidate_title.to_string());
            }
        }
    }

    // 6. Check for separators: " - ", " – ", " — ", " : ", "／", " / "
    let separators = [" - ", " – ", " — ", " : ", "／", " / "];
    for sep in &separators {
        if let Some((left, right)) = title.split_once(sep) {
            let left_trimmed = left.trim().trim_matches(|c: char| c == '"' || c == '\'' || c == '(' || c == ')' || c == '[' || c == ']' || c == '【' || c == '】' || c == '「' || c == '」' || c == '『' || c == '』').trim().to_string();
            let mut right_trimmed = right.trim().trim_matches(|c: char| c == '"' || c == '\'' || c == '(' || c == ')' || c == '[' || c == ']' || c == '【' || c == '】' || c == '「' || c == '」' || c == '『' || c == '』').trim().to_string();

            if let Ok(re) = Regex::new(r#"(?i)\s*["']?\s*(official\s*video|mv|hd|audio)\s*["']?$"#) {
                let cleaned = re.replace(&right_trimmed, "").trim().to_string();
                if !cleaned.is_empty() {
                    right_trimmed = cleaned;
                }
            }

            if !left_trimmed.is_empty() && !right_trimmed.is_empty() {
                // If uploader matches right side, then right is artist and left is title (e.g. "アイドル／YOASOBI")
                if let Some(ref u) = clean_uploader {
                    let u_lower = u.to_lowercase();
                    if u_lower.contains(&right_trimmed.to_lowercase()) && !u_lower.contains(&left_trimmed.to_lowercase()) {
                        return (right_trimmed, left_trimmed);
                    }
                }
                // Default: left is artist, right is title
                return (left_trimmed, right_trimmed);
            }
        }
    }

    // 7. Fallback: if no separator matched, clean title and use cleaned uploader
    let clean_title = title
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '【' || c == '】' || c == '「' || c == '」' || c == '『' || c == '』')
        .trim()
        .to_string();

    let artist = clean_uploader.filter(|u| !u.is_empty()).unwrap_or_else(|| "Unknown Artist".to_string());
    (artist, clean_title)
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

    /// Fetches a playlist with fallback to raw URL extraction (for watch mixes or non-standard playlist IDs)
    pub async fn fetch_playlist_or_url(
        &self,
        raw_input: &str,
        playlist_id: &str,
    ) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        // 1. Try standard InnerTube playlist fetch
        if let Ok(result) = self.fetch_innertube_playlist(playlist_id).await {
            if !result.1.is_empty() {
                return Ok(result);
            }
        }

        // 2. Try yt-dlp on standard playlist URL
        if let Ok(result) = self.fetch_ytdlp_playlist(playlist_id).await {
            if !result.1.is_empty() {
                return Ok(result);
            }
        }

        // 3. Fallback: yt-dlp directly on raw input URL (handles mixes, watch URLs, etc.)
        if raw_input.contains("youtube.com") || raw_input.contains("youtu.be") {
            return self.fetch_ytdlp_raw_url(raw_input).await;
        }

        Err(anyhow!(
            "failed to import playlist from {} (InnerTube & yt-dlp failed)",
            playlist_id
        ))
    }

    /// Primary Extraction: YouTube Music InnerTube browse endpoint
    pub async fn fetch_innertube_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let clean_pid = playlist_id
            .strip_prefix("yt:playlist:")
            .or_else(|| playlist_id.strip_prefix("yt:browse:"))
            .unwrap_or(playlist_id);
        let browse_id = if clean_pid.starts_with("VL")
            || clean_pid.starts_with("MPREb_")
            || clean_pid.starts_with("UC")
            || clean_pid.starts_with("FEmusic_")
        {
            clean_pid.to_string()
        } else {
            format!("VL{}", clean_pid)
        };

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(cookie_str) = crate::sources::sync::auth::YouTubeAuthHandler::resolve_cookie_header() {
            if let Ok(val) = HeaderValue::from_str(&cookie_str) {
                headers.insert(reqwest::header::COOKIE, val);
            }
        }

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

                let raw_duration = renderer["fixedColumns"]
                    .as_array()
                    .and_then(|cols| cols.first())
                    .and_then(|col| {
                        col["musicResponsiveListItemFixedColumnRenderer"]["text"]["runs"][0]["text"]
                            .as_str()
                    })
                    .or_else(|| {
                        flex_cols
                            .and_then(|cols| cols.get(1))
                            .and_then(|col| {
                                col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
                                    .as_array()
                            })
                            .and_then(|runs| runs.last())
                            .and_then(|r| r["text"].as_str())
                    })
                    .or_else(|| {
                        flex_cols.and_then(|cols| {
                            for col in cols {
                                if let Some(runs) = col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array() {
                                    for r in runs.iter().rev() {
                                        if let Some(txt) = r["text"].as_str() {
                                            if txt.contains(':') && txt.chars().all(|c| c.is_ascii_digit() || c == ':') {
                                                return Some(txt);
                                            }
                                        }
                                    }
                                }
                            }
                            None
                        })
                    })
                    .unwrap_or("0:00");

                let (clean_artist, clean_title) = clean_youtube_title(raw_title, artist_name);
                let duration_ms = parse_duration_str(raw_duration);

                let track_cover = renderer["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]
                    ["thumbnails"]
                    .as_array()
                    .and_then(|arr| arr.last())
                    .and_then(|t| t["url"].as_str())
                    .map(|s| crate::sources::lossless_relay::archiver::upgrade_cover_art_url_to_hd(s));

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

    /// Extract album information and full tracklist from YouTube Music InnerTube browse endpoint
    pub async fn fetch_innertube_album(
        &self,
        album_id: &str,
    ) -> Result<(String, String, Option<String>, Option<String>, Vec<TrackInfo>)> {
        let clean_bid = album_id
            .strip_prefix("yt:browse:")
            .or_else(|| album_id.strip_prefix("yt:playlist:"))
            .unwrap_or(album_id);
        let browse_id = if clean_bid.starts_with("OLAK5uy_") || clean_bid.starts_with("PL") {
            format!("VL{}", clean_bid)
        } else {
            clean_bid.to_string()
        };

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(cookie_str) = crate::sources::sync::auth::YouTubeAuthHandler::resolve_cookie_header() {
            if let Ok(val) = HeaderValue::from_str(&cookie_str) {
                headers.insert(reqwest::header::COOKIE, val);
            }
        }

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
            .context("failed to execute InnerTube album browse request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("InnerTube album browse returned status {}", resp.status()));
        }

        let body: Value = resp.json().await.context("failed to parse InnerTube JSON response")?;

        let mut album_title = String::new();
        let mut album_artist = String::new();
        let mut release_year = None;
        let mut cover_art_url = None;

        // Try twoColumnBrowseResultsRenderer (standard for MPREb_ albums)
        let two_col = &body["contents"]["twoColumnBrowseResultsRenderer"];
        if let Some(tabs) = two_col["tabs"].as_array() {
            if let Some(tab) = tabs.first() {
                let sec_contents = &tab["tabRenderer"]["content"]["sectionListRenderer"]["contents"];
                if let Some(first_sec) = sec_contents.as_array().and_then(|a| a.first()) {
                    let header = &first_sec["musicResponsiveHeaderRenderer"];
                    if !header.is_null() {
                        if let Some(t) = header["title"]["runs"][0]["text"].as_str() {
                            album_title = t.to_string();
                        }
                        if let Some(a) = header["straplineTextOne"]["runs"][0]["text"].as_str() {
                            album_artist = a.to_string();
                        } else if let Some(runs) = header["subtitle"]["runs"].as_array() {
                            if let Some(first) = runs.first().and_then(|r| r["text"].as_str()) {
                                if first != "Album" && first != "Single" && first != "EP" {
                                    album_artist = first.to_string();
                                }
                            }
                        }
                        if let Some(runs) = header["subtitle"]["runs"].as_array() {
                            for r in runs {
                                if let Some(txt) = r["text"].as_str() {
                                    let trimmed = txt.trim();
                                    if trimmed.len() == 4 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                                        release_year = Some(trimmed.to_string());
                                        break;
                                    }
                                }
                            }
                        }
                        cover_art_url = header["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]
                            .as_array()
                            .and_then(|arr| arr.last())
                            .and_then(|t| t["url"].as_str())
                            .map(|s| s.to_string());
                    }
                }
            }
        }

        // Fallback header from top-level header object
        if album_title.is_empty() {
            let header = &body["header"]["musicDetailHeaderRenderer"];
            if let Some(t) = header["title"]["runs"][0]["text"].as_str() {
                album_title = t.to_string();
            }
            if let Some(a) = header["subtitle"]["runs"][0]["text"].as_str() {
                album_artist = a.to_string();
            }
            cover_art_url = header["thumbnail"]["croppedSquareThumbnailRenderer"]["thumbnail"]["thumbnails"]
                .as_array()
                .and_then(|arr| arr.last())
                .and_then(|t| t["url"].as_str())
                .map(|s| s.to_string());
        }

        let mut tracks = Vec::new();

        // 1. Check secondaryContents (twoColumnBrowseResultsRenderer)
        let mut items = None;
        if let Some(sec_contents) = two_col["secondaryContents"]["sectionListRenderer"]["contents"].as_array() {
            for sec in sec_contents {
                if let Some(arr) = sec["musicShelfRenderer"]["contents"].as_array()
                    .or_else(|| sec["musicPlaylistShelfRenderer"]["contents"].as_array())
                {
                    items = Some(arr);
                    break;
                }
            }
        }
        // 2. Check tabs[0].tabRenderer.content (twoColumn or singleColumn tabs)
        if items.is_none() {
            let tabs = two_col["tabs"].as_array()
                .or_else(|| body["contents"]["singleColumnBrowseResultsRenderer"]["tabs"].as_array());
            if let Some(tabs_list) = tabs {
                if let Some(tab) = tabs_list.first() {
                    if let Some(sec_contents) = tab["tabRenderer"]["content"]["sectionListRenderer"]["contents"].as_array() {
                        for sec in sec_contents {
                            if let Some(arr) = sec["musicShelfRenderer"]["contents"].as_array()
                                .or_else(|| sec["musicPlaylistShelfRenderer"]["contents"].as_array())
                            {
                                items = Some(arr);
                                break;
                            }
                        }
                    }
                }
            }
        }

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

                let flex_cols = renderer["flexColumns"].as_array();
                let raw_title = flex_cols
                    .and_then(|cols| cols.first())
                    .and_then(|col| {
                        col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"]
                            .as_str()
                    })
                    .unwrap_or("Unknown Title");

                let raw_duration = renderer["fixedColumns"]
                    .as_array()
                    .and_then(|cols| cols.first())
                    .and_then(|col| {
                        col["musicResponsiveListItemFixedColumnRenderer"]["text"]["runs"][0]["text"]
                            .as_str()
                    })
                    .or_else(|| {
                        flex_cols
                            .and_then(|cols| cols.get(1))
                            .and_then(|col| {
                                col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
                                    .as_array()
                            })
                            .and_then(|runs| runs.last())
                            .and_then(|r| r["text"].as_str())
                    })
                    .unwrap_or("0:00");

                let duration_ms = parse_duration_str(raw_duration);
                let (clean_artist, clean_title) = clean_youtube_title(raw_title, Some(&album_artist));
                let artist_to_use = if clean_artist.is_empty() {
                    if album_artist.is_empty() { "Unknown Artist".to_string() } else { album_artist.clone() }
                } else {
                    clean_artist
                };

                let uri = video_id
                    .map(|vid| format!("yt:video:{}", vid))
                    .unwrap_or_else(|| format!("yt:track:{}", clean_title));

                tracks.push(TrackInfo {
                    uri: Some(uri.clone()),
                    name: clean_title,
                    artists: vec![artist_to_use],
                    album: album_title.clone(),
                    duration_ms: duration_ms.max(0) as u64,
                    id: Some(uri),
                    album_id: Some(clean_bid.to_string()),
                    artist_refs: Vec::new(),
                    is_playable: video_id.is_some(),
                    is_local: false,
                    track_number: (idx + 1) as u32,
                    explicit: false,
                    image_url: cover_art_url.clone(),
                    release_date: release_year.clone(),
                    generated_name: None,
                });
            }
        }

        Ok((album_title, album_artist, release_year, cover_art_url, tracks))
    }

    /// Extract artist profile, official top songs, and albums from YouTube Music InnerTube browse endpoint
    pub async fn fetch_innertube_artist(
        &self,
        artist_id: &str,
    ) -> Result<(String, Vec<TrackInfo>, Vec<AlbumDetailHeader>)> {
        let clean_bid = artist_id.strip_prefix("yt:browse:").unwrap_or(artist_id);

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(cookie_str) = crate::sources::sync::auth::YouTubeAuthHandler::resolve_cookie_header() {
            if let Ok(val) = HeaderValue::from_str(&cookie_str) {
                headers.insert(reqwest::header::COOKIE, val);
            }
        }

        let payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20240401.01.00",
                    "hl": "en",
                    "gl": "US"
                }
            },
            "browseId": clean_bid
        });

        let resp = self
            .client
            .post(YTM_BROWSE_URL)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .context("failed to execute InnerTube artist browse request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("InnerTube artist browse returned status {}", resp.status()));
        }

        let body: Value = resp.json().await.context("failed to parse InnerTube JSON response")?;

        // Extract artist name
        let header = &body["header"]["musicImmersiveHeaderRenderer"];
        let visual_header = &body["header"]["musicVisualHeaderRenderer"];
        let artist_name = header["title"]["runs"][0]["text"]
            .as_str()
            .or_else(|| visual_header["title"]["runs"][0]["text"].as_str())
            .unwrap_or("Artist")
            .to_string();

        let mut top_songs = Vec::new();
        let mut albums = Vec::new();

        let contents = body["contents"]["singleColumnBrowseResultsRenderer"]["tabs"][0]
            ["tabRenderer"]["content"]["sectionListRenderer"]["contents"]
            .as_array();

        if let Some(sections) = contents {
            for sec in sections {
                // 1. MusicShelfRenderer (Top Songs)
                if let Some(shelf) = sec.get("musicShelfRenderer") {
                    let title = shelf["title"]["runs"][0]["text"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase();
                    if title.contains("song") || top_songs.is_empty() {
                        if let Some(items) = shelf["contents"].as_array() {
                            for (idx, item) in items.iter().enumerate() {
                                let renderer = &item["musicResponsiveListItemRenderer"];
                                if renderer.is_null() {
                                    continue;
                                }

                                let video_id = renderer["playlistItemData"]["videoId"]
                                    .as_str()
                                    .or_else(|| {
                                        renderer["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]
                                            ["musicPlayButtonRenderer"]["playNavigationEndpoint"]
                                            ["watchEndpoint"]["videoId"]
                                            .as_str()
                                    });

                                let flex_cols = renderer["flexColumns"].as_array();
                                let raw_title = flex_cols
                                    .and_then(|cols| cols.first())
                                    .and_then(|col| {
                                        col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"]
                                            .as_str()
                                    })
                                    .unwrap_or("Unknown Title");

                                let raw_duration = renderer["fixedColumns"]
                                    .as_array()
                                    .and_then(|cols| cols.first())
                                    .and_then(|col| {
                                        col["musicResponsiveListItemFixedColumnRenderer"]["text"]["runs"][0]["text"]
                                            .as_str()
                                    })
                                    .or_else(|| {
                                        flex_cols
                                            .and_then(|cols| cols.get(1))
                                            .and_then(|col| {
                                                col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
                                                    .as_array()
                                            })
                                            .and_then(|runs| runs.last())
                                            .and_then(|r| r["text"].as_str())
                                    })
                                    .unwrap_or("0:00");

                                let (clean_art, clean_title) = clean_youtube_title(raw_title, Some(&artist_name));
                                let duration_ms = parse_duration_str(raw_duration);
                                let uri = video_id
                                    .map(|vid| format!("yt:video:{}", vid))
                                    .unwrap_or_else(|| format!("yt:track:{}", clean_title));

                                let thumbnail_url = renderer["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]
                                    .as_array()
                                    .and_then(|arr| arr.last())
                                    .and_then(|t| t["url"].as_str())
                                    .map(|s| crate::sources::lossless_relay::archiver::upgrade_cover_art_url_to_hd(s));

                                top_songs.push(TrackInfo {
                                    uri: Some(uri.clone()),
                                    name: clean_title,
                                    artists: vec![if clean_art.is_empty() { artist_name.clone() } else { clean_art }],
                                    album: String::new(),
                                    duration_ms: duration_ms.max(0) as u64,
                                    id: Some(uri),
                                    album_id: None,
                                    artist_refs: Vec::new(),
                                    is_playable: video_id.is_some(),
                                    is_local: false,
                                    track_number: (idx + 1) as u32,
                                    explicit: false,
                                    image_url: thumbnail_url,
                                    release_date: None,
                                    generated_name: None,
                                });
                            }
                        }
                    }
                }

                // 2. MusicCarouselShelfRenderer (Albums, Singles & EPs)
                if let Some(carousel) = sec.get("musicCarouselShelfRenderer") {
                    let header_title = carousel["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]["runs"][0]["text"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase();

                    if header_title.contains("album") || header_title.contains("single") || header_title.contains("ep") {
                        if let Some(items) = carousel["contents"].as_array() {
                            for item in items {
                                let renderer = &item["musicTwoRowItemRenderer"];
                                if renderer.is_null() {
                                    continue;
                                }

                                let album_title = renderer["title"]["runs"][0]["text"]
                                    .as_str()
                                    .unwrap_or("Untitled Album")
                                    .to_string();

                                let browse_id = renderer["navigationEndpoint"]["browseEndpoint"]["browseId"]
                                    .as_str()
                                    .unwrap_or("");

                                if browse_id.is_empty() {
                                    continue;
                                }

                                let year = renderer["subtitle"]["runs"].as_array().and_then(|runs| {
                                    runs.iter().find_map(|r| {
                                        let txt = r["text"].as_str().unwrap_or("").trim();
                                        if txt.len() == 4 && txt.chars().all(|c| c.is_ascii_digit()) {
                                            Some(txt.to_string())
                                        } else {
                                            None
                                        }
                                    })
                                });

                                // Deduplicate by title
                                if !albums.iter().any(|a: &AlbumDetailHeader| a.title.eq_ignore_ascii_case(&album_title)) {
                                    albums.push(AlbumDetailHeader {
                                        id: format!("yt:browse:{}", browse_id),
                                        title: album_title,
                                        year,
                                        track_count: 0,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok((artist_name, top_songs, albums))
    }

    /// Fallback Extraction: yt-dlp --flat-playlist --dump-json
    async fn fetch_ytdlp_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let url = format!("https://www.youtube.com/playlist?list={}", playlist_id);

        let mut cmd = Command::new("yt-dlp");
        cmd.arg("--flat-playlist")
            .arg("--dump-json")
            .arg("--no-warnings");

        for arg in crate::sources::sync::auth::YouTubeAuthHandler::ytdlp_cookie_args() {
            cmd.arg(arg);
        }

        let output = cmd
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
                let duration_secs = entry["duration"]
                    .as_f64()
                    .or_else(|| entry["duration"].as_i64().map(|i| i as f64))
                    .or_else(|| {
                        entry["duration_string"].as_str().map(|s| (parse_duration_str(s) / 1000) as f64)
                    })
                    .unwrap_or(0.0);
                let duration_ms = (duration_secs * 1000.0) as i64;

                let (clean_artist, clean_title) = clean_youtube_title(raw_title, uploader);

                let cover_art_url = entry["thumbnails"]
                    .as_array()
                    .and_then(|arr| arr.last())
                    .and_then(|t| t["url"].as_str())
                    .map(|s| crate::sources::lossless_relay::archiver::upgrade_cover_art_url_to_hd(s));

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
            cover_art_url: tracks.first().and_then(|t| t.cover_art_url.clone()),
            snapshot_id: None,
            track_count: tracks.len() as i64,
            last_synced_at: now,
        };

        Ok((playlist, tracks))
    }

    /// Direct yt-dlp execution on any YouTube URL (handles videos, mixes, watch links, etc.)
    pub async fn fetch_ytdlp_raw_url(
        &self,
        target_url: &str,
    ) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let mut cmd = Command::new("yt-dlp");
        cmd.arg("--flat-playlist")
            .arg("--dump-json")
            .arg("--no-warnings");

        for arg in crate::sources::sync::auth::YouTubeAuthHandler::ytdlp_cookie_args() {
            cmd.arg(arg);
        }

        let output = cmd
            .arg(target_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .with_context(|| format!("failed to spawn yt-dlp for url {}", target_url))?;

        if !output.status.success() {
            return Err(anyhow!(
                "yt-dlp exited with non-zero status for url {}",
                target_url
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut tracks = Vec::new();
        let now = Utc::now().timestamp();
        let mut playlist_title = "YouTube Playlist".to_string();
        let mut playlist_uploader = None;
        let mut first_vid = String::new();

        for (idx, line) in stdout.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Ok(entry) = serde_json::from_str::<Value>(line) {
                if idx == 0 {
                    if let Some(pt) = entry["playlist_title"].as_str() {
                        playlist_title = pt.to_string();
                    } else if let Some(t) = entry["title"].as_str() {
                        playlist_title = format!("[YouTube] {}", t);
                    }
                    if let Some(pu) = entry["playlist_uploader"].as_str() {
                        playlist_uploader = Some(pu.to_string());
                    } else if let Some(u) = entry["uploader"].as_str().or_else(|| entry["channel"].as_str()) {
                        playlist_uploader = Some(u.to_string());
                    }
                }

                let vid = entry["id"].as_str().unwrap_or("").to_string();
                if vid.is_empty() {
                    continue;
                }
                if first_vid.is_empty() {
                    first_vid = vid.clone();
                }

                let raw_title = entry["title"].as_str().unwrap_or("Unknown Title");
                let uploader = entry["uploader"].as_str().or_else(|| entry["channel"].as_str());
                let duration_secs = entry["duration"]
                    .as_f64()
                    .or_else(|| entry["duration"].as_i64().map(|i| i as f64))
                    .or_else(|| {
                        entry["duration_string"].as_str().map(|s| (parse_duration_str(s) / 1000) as f64)
                    })
                    .unwrap_or(0.0);
                let duration_ms = (duration_secs * 1000.0) as i64;

                let (clean_artist, clean_title) = clean_youtube_title(raw_title, uploader);

                let cover_art_url = entry["thumbnails"]
                    .as_array()
                    .and_then(|arr| arr.last())
                    .and_then(|t| t["url"].as_str())
                    .map(|s| crate::sources::lossless_relay::archiver::upgrade_cover_art_url_to_hd(s));

                tracks.push(PlaylistTrack {
                    playlist_id: String::new(), // will set below
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

        if tracks.is_empty() {
            return Err(anyhow!("no tracks extracted by yt-dlp from {}", target_url));
        }

        let canonical_id = if tracks.len() == 1 {
            format!("yt:video:{}", first_vid)
        } else {
            format!("yt:playlist:{:x}", now)
        };

        for t in &mut tracks {
            t.playlist_id = canonical_id.clone();
        }

        let playlist = ImportedPlaylist {
            id: canonical_id,
            platform: "youtube".to_string(),
            title: playlist_title,
            author: playlist_uploader,
            source_url: target_url.to_string(),
            cover_art_url: tracks.first().and_then(|t| t.cover_art_url.clone()),
            snapshot_id: None,
            track_count: tracks.len() as i64,
            last_synced_at: now,
        };

        Ok((playlist, tracks))
    }

    /// Fetches a single video by ID or URL and wraps it in an ImportedPlaylist and track
    pub async fn fetch_video(&self, video_id_or_url: &str) -> Result<(ImportedPlaylist, Vec<PlaylistTrack>)> {
        let url = if video_id_or_url.starts_with("http://") || video_id_or_url.starts_with("https://") {
            video_id_or_url.to_string()
        } else {
            format!("https://www.youtube.com/watch?v={}", video_id_or_url)
        };
        self.fetch_ytdlp_raw_url(&url).await
    }

    /// Fetches and saves a YouTube playlist or video into the local SQLite database
    pub async fn import_playlist(
        &self,
        db: &Database,
        url_or_id: &str,
    ) -> Result<ImportedPlaylist> {
        let resource = parse_youtube_url(url_or_id)
            .ok_or_else(|| anyhow!("invalid YouTube URL or ID: {}", url_or_id))?;

        let (playlist, tracks) = match resource {
            YouTubeResource::Playlist(id) => self.fetch_playlist_or_url(url_or_id, &id).await?,
            YouTubeResource::Video(vid) => self.fetch_video(&vid).await?,
        };

        db.upsert_imported_playlist(&playlist)
            .context("failed to insert imported playlist into database")?;
        db.replace_playlist_tracks(&playlist.id, &tracks)
            .context("failed to insert playlist tracks into database")?;

        Ok(playlist)
    }

    /// Fetches YouTube Music algorithmic radio / auto-play recommendations for a given seed video ID.
    /// Uses YouTube Music's InnerTube `v1/next` endpoint with `playlistId: "RDAMVM{videoId}"`.
    pub async fn fetch_youtube_radio_tracks(&self, video_id: &str) -> Result<Vec<TrackInfo>> {
        let clean_vid = parse_youtube_url(video_id)
            .and_then(|r| match r {
                YouTubeResource::Video(id) => Some(id),
                _ => None,
            })
            .unwrap_or_else(|| video_id.to_string());

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(cookie_str) = crate::sources::sync::auth::YouTubeAuthHandler::resolve_cookie_header() {
            if let Ok(val) = HeaderValue::from_str(&cookie_str) {
                headers.insert(reqwest::header::COOKIE, val);
            }
        }

        let payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20240401.01.00",
                    "hl": "en",
                    "gl": "US"
                }
            },
            "videoId": clean_vid,
            "playlistId": format!("RDAMVM{}", clean_vid),
            "isAudioOnly": true
        });

        let resp = self
            .client
            .post(YTM_NEXT_URL)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .context("failed to execute InnerTube next radio request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("InnerTube radio request returned status {}", resp.status()));
        }

        let body: Value = resp.json().await.context("failed to parse InnerTube radio JSON response")?;

        let mut tracks = Vec::new();
        let contents = find_playlist_panel_contents(&body);

        if let Some(items) = contents {
            for (idx, item) in items.iter().enumerate() {
                let renderer = item.get("playlistPanelVideoRenderer")
                    .or_else(|| item.get("playlistPanelVideoWrapperRenderer")
                        .and_then(|w| w.get("primaryRenderer"))
                        .and_then(|p| p.get("playlistPanelVideoRenderer")));

                let Some(r) = renderer else {
                    continue;
                };

                let vid = match r["videoId"].as_str() {
                    Some(v) if !v.is_empty() => v.to_string(),
                    _ => continue,
                };

                // Skip the seed video itself
                if vid == clean_vid {
                    continue;
                }

                let raw_title = r["title"]["runs"][0]["text"].as_str().unwrap_or("Unknown Title");
                let artist_name = r["longBylineText"]["runs"][0]["text"].as_str()
                    .or_else(|| r["shortBylineText"]["runs"][0]["text"].as_str());
                let (clean_artist, clean_title) = clean_youtube_title(raw_title, artist_name);

                let raw_duration = r["lengthText"]["runs"][0]["text"].as_str().unwrap_or("0:00");
                let duration_ms = parse_duration_str(raw_duration);

                let track_cover = r["thumbnail"]["thumbnails"]
                    .as_array()
                    .and_then(|arr| arr.last())
                    .and_then(|t| t["url"].as_str())
                    .map(|s| crate::sources::lossless_relay::archiver::upgrade_cover_art_url_to_hd(s));

                let album_name = r["longBylineText"]["runs"].as_array().and_then(|runs| {
                    if runs.len() >= 3 {
                        runs.get(2).and_then(|r| r["text"].as_str()).map(|s| s.to_string())
                    } else {
                        None
                    }
                }).unwrap_or_default();

                tracks.push(TrackInfo {
                    id: Some(vid.clone()),
                    uri: Some(format!("youtube:video:{}", vid)),
                    name: clean_title,
                    artists: vec![clean_artist],
                    album: album_name,
                    duration_ms: duration_ms as u64,
                    album_id: None,
                    artist_refs: vec![],
                    is_playable: true,
                    is_local: false,
                    track_number: idx as u32 + 1,
                    explicit: false,
                    image_url: track_cover,
                    release_date: None,
                    generated_name: None,
                });
            }
        }

        Ok(tracks)
    }
}

/// Helper function to find playlist panel contents in YouTube Music JSON responses
fn find_playlist_panel_contents<'a>(val: &'a Value) -> Option<&'a Vec<Value>> {
    if let Some(arr) = val["contents"]["singleColumnMusicWatchNextResultsRenderer"]["tabbedRenderer"]["watchNextTabbedResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]["musicQueueRenderer"]["content"]["playlistPanelRenderer"]["contents"].as_array() {
        return Some(arr);
    }
    if let Some(arr) = val["musicQueueRenderer"]["content"]["playlistPanelRenderer"]["contents"].as_array() {
        return Some(arr);
    }
    find_playlist_panel_recursive(val)
}

fn find_playlist_panel_recursive<'a>(val: &'a Value) -> Option<&'a Vec<Value>> {
    if let Some(obj) = val.as_object() {
        if let Some(panel) = obj.get("playlistPanelRenderer") {
            if let Some(contents) = panel.get("contents").and_then(|c| c.as_array()) {
                return Some(contents);
            }
        }
        for (_, v) in obj {
            if let Some(found) = find_playlist_panel_recursive(v) {
                return Some(found);
            }
        }
    } else if let Some(arr) = val.as_array() {
        for v in arr {
            if let Some(found) = find_playlist_panel_recursive(v) {
                return Some(found);
            }
        }
    }
    None
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

        assert_eq!(
            parse_youtube_url("youtube:playlist:PL12345"),
            Some(YouTubeResource::Playlist("PL12345".to_string()))
        );

        assert_eq!(
            parse_youtube_url("youtube:video:dQw4w9WgXcQ"),
            Some(YouTubeResource::Video("dQw4w9WgXcQ".to_string()))
        );

        assert_eq!(
            parse_youtube_url("yt:video:dQw4w9WgXcQ"),
            Some(YouTubeResource::Video("dQw4w9WgXcQ".to_string()))
        );

        assert_eq!(
            parse_youtube_url("https://youtu.be/dQw4w9WgXcQ?list=PL12345"),
            Some(YouTubeResource::Playlist("PL12345".to_string()))
        );

        assert_eq!(
            parse_youtube_url("https://music.youtube.com/browse/VLPL12345"),
            Some(YouTubeResource::Playlist("PL12345".to_string()))
        );

        assert_eq!(
            parse_youtube_url("youtube.com/playlist?list=PL12345"),
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

        // Japanese Artist「Title」 pattern with trailing unbracketed tag
        let (artist, title) = clean_youtube_title(
            "YOASOBI「アイドル」 Official Music Video",
            Some("Ayase / YOASOBI"),
        );
        assert_eq!(artist, "YOASOBI");
        assert_eq!(title, "アイドル");

        // CJK MV bracket with full-width slash and uploader matching right
        let (artist, title) = clean_youtube_title(
            "【MV】アイドル／YOASOBI",
            Some("Ayase / YOASOBI"),
        );
        assert_eq!(artist, "YOASOBI");
        assert_eq!(title, "アイドル");

        // Japanese Artist『Title』 pattern with clip tag
        let (artist, title) = clean_youtube_title(
            "LiSA 『炎』 -MUSiC CLiP-",
            Some("LiSA Official YouTube Channel"),
        );
        assert_eq!(artist, "LiSA");
        assert_eq!(title, "炎");

        // Korean Artist 'Title' pattern with bilingual name
        let (artist, title) = clean_youtube_title(
            "NewJeans (뉴진스) 'Ditto' Official MV (Performance ver.)",
            Some("HYBE LABELS"),
        );
        assert_eq!(artist, "NewJeans");
        assert_eq!(title, "Ditto");
    }

    #[test]
    fn test_find_playlist_panel_contents_and_radio_parser() {
        let fake_json = serde_json::json!({
            "contents": {
                "singleColumnMusicWatchNextResultsRenderer": {
                    "tabbedRenderer": {
                        "watchNextTabbedResultsRenderer": {
                            "tabs": [
                                {
                                    "tabRenderer": {
                                        "content": {
                                            "musicQueueRenderer": {
                                                "content": {
                                                    "playlistPanelRenderer": {
                                                        "contents": [
                                                            {
                                                                "playlistPanelVideoRenderer": {
                                                                    "videoId": "seed_vid_123",
                                                                    "title": { "runs": [{ "text": "Seed Song" }] },
                                                                    "longBylineText": { "runs": [{ "text": "Artist A" }, { "text": " • " }, { "text": "Album A" }] },
                                                                    "lengthText": { "runs": [{ "text": "3:20" }] }
                                                                }
                                                            },
                                                            {
                                                                "playlistPanelVideoRenderer": {
                                                                    "videoId": "radio_vid_456",
                                                                    "title": { "runs": [{ "text": "Discovered Radio Track" }] },
                                                                    "longBylineText": { "runs": [{ "text": "Artist B" }, { "text": " • " }, { "text": "Album B" }] },
                                                                    "lengthText": { "runs": [{ "text": "4:15" }] },
                                                                    "thumbnail": {
                                                                        "thumbnails": [{ "url": "https://lh3.googleusercontent.com/test=w120-h120" }]
                                                                    }
                                                                }
                                                            }
                                                        ]
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });

        let contents = find_playlist_panel_contents(&fake_json).expect("contents should be found");
        assert_eq!(contents.len(), 2);
    }
}
