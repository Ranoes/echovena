use super::types::{SearchBadge, SearchResponse, SearchResultItem, SearchScope};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;

const YTM_SEARCH_URL: &str = "https://music.youtube.com/youtubei/v1/search";
const YT_SEARCH_URL: &str = "https://www.youtube.com/youtubei/v1/search";
const SUGGEST_URL: &str = "https://suggestqueries-clients6.youtube.com/complete/search";
const YTM_SUGGEST_URL: &str = "https://music.youtube.com/youtubei/v1/music/get_search_suggestions";

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Client for performing zero-API-key YouTube and YouTube Music InnerTube searches
#[derive(Clone)]
pub struct InnerTubeClient {
    client: reqwest::Client,
}

impl Default for InnerTubeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl InnerTubeClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Primary search entry point: queries YouTube Music InnerTube, falling back to YouTube Web InnerTube
    pub async fn search(&self, query: &str, scope: SearchScope) -> Result<Vec<SearchResultItem>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        // Try YouTube Music InnerTube first
        match self.search_ytm_innertube(trimmed, scope).await {
            Ok(results) if !results.is_empty() => return Ok(results),
            Ok(_) => {
                log::debug!("YTM search returned 0 items, attempting YouTube Web fallback");
            }
            Err(e) => {
                log::warn!("YTM search failed ({}), attempting YouTube Web fallback", e);
            }
        }

        // Fallback to standard YouTube InnerTube
        self.search_yt_web_innertube(trimmed, scope).await
    }

    /// Fetches live autocomplete search suggestions as the user types
    pub async fn get_suggestions(&self, query: &str) -> Result<Vec<String>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        // Primary: fast public suggestqueries endpoint (Firefox client format)
        if let Ok(suggestions) = self.get_suggestqueries(trimmed).await {
            if !suggestions.is_empty() {
                return Ok(suggestions);
            }
        }

        // Secondary: YTM InnerTube suggestions endpoint
        self.get_ytm_suggestions(trimmed).await
    }

    /// Full search query combining items and suggestions
    pub async fn search_full(&self, query: &str, scope: SearchScope) -> Result<SearchResponse> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(SearchResponse::default());
        }

        let (items_res, suggestions_res) = tokio::join!(
            self.search(trimmed, scope),
            self.get_suggestions(trimmed)
        );

        let items = items_res.unwrap_or_default();
        let suggestions = suggestions_res.unwrap_or_default();

        Ok(SearchResponse {
            query: trimmed.to_string(),
            scope,
            items,
            suggestions,
        })
    }

    // -------------------------------------------------------------------------
    // YouTube Music InnerTube (WEB_REMIX)
    // -------------------------------------------------------------------------

    async fn search_ytm_innertube(&self, query: &str, scope: SearchScope) -> Result<Vec<SearchResultItem>> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ORIGIN, HeaderValue::from_static("https://music.youtube.com"));
        headers.insert(REFERER, HeaderValue::from_static("https://music.youtube.com/"));

        let mut payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20240401.01.00",
                    "hl": "en",
                    "gl": "US"
                }
            },
            "query": query
        });

        if let Some(params) = scope.innertube_params() {
            payload["params"] = serde_json::Value::String(params.to_string());
        }

        let resp = self
            .client
            .post(YTM_SEARCH_URL)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .context("failed to send YTM InnerTube search request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("YTM InnerTube returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .context("failed to parse YTM InnerTube JSON")?;

        Ok(parse_ytm_search_response(&json, scope))
    }

    // -------------------------------------------------------------------------
    // YouTube Web InnerTube (Fallback)
    // -------------------------------------------------------------------------

    async fn search_yt_web_innertube(&self, query: &str, scope: SearchScope) -> Result<Vec<SearchResultItem>> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ORIGIN, HeaderValue::from_static("https://www.youtube.com"));
        headers.insert(REFERER, HeaderValue::from_static("https://www.youtube.com/"));

        let payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20240401.01.00",
                    "hl": "en",
                    "gl": "US"
                }
            },
            "query": query
        });

        let resp = self
            .client
            .post(YT_SEARCH_URL)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .context("failed to send YouTube Web InnerTube search request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("YouTube Web InnerTube returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .context("failed to parse YouTube Web InnerTube JSON")?;

        Ok(parse_yt_web_search_response(&json, scope))
    }

    // -------------------------------------------------------------------------
    // Autocomplete & Suggestions
    // -------------------------------------------------------------------------

    async fn get_suggestqueries(&self, query: &str) -> Result<Vec<String>> {
        let resp = self
            .client
            .get(SUGGEST_URL)
            .query(&[("client", "firefox"), ("ds", "yt"), ("q", query)])
            .send()
            .await
            .context("failed to send suggestqueries request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("suggestqueries returned HTTP {}", resp.status()));
        }

        let json: Value = resp.json().await.context("failed to parse suggestions JSON")?;
        parse_suggestqueries_response(&json)
    }

    async fn get_ytm_suggestions(&self, query: &str) -> Result<Vec<String>> {
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
            "input": query
        });

        let resp = self
            .client
            .post(YTM_SUGGEST_URL)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .context("failed to send YTM suggestions request")?;

        if !resp.status().is_success() {
            return Err(anyhow!("YTM suggestions returned HTTP {}", resp.status()));
        }

        let json: Value = resp.json().await.context("failed to parse YTM suggestions JSON")?;
        Ok(parse_ytm_suggestions_response(&json))
    }
}

// -----------------------------------------------------------------------------
// Response Parsers (Public for Unit Testing)
// -----------------------------------------------------------------------------

/// Parses YouTube Music InnerTube JSON response
pub fn parse_ytm_search_response(json: &Value, scope: SearchScope) -> Vec<SearchResultItem> {
    let mut results = Vec::new();

    // Check tab contents
    let tabs = json["contents"]["tabbedSearchResultsRenderer"]["tabs"]
        .as_array()
        .or_else(|| json["contents"]["singleColumnBrowseResultsRenderer"]["tabs"].as_array());

    let Some(tabs_list) = tabs else {
        return results;
    };

    for tab in tabs_list {
        let sections = tab["tabRenderer"]["content"]["sectionListRenderer"]["contents"].as_array();
        let Some(section_list) = sections else {
            continue;
        };

        for section in section_list {
            // 1. MusicCardShelfRenderer (Top result card)
            if let Some(card) = section.get("musicCardShelfRenderer") {
                if let Some(item) = parse_ytm_card_shelf(card, scope) {
                    results.push(item);
                }
            }

            // 2. MusicShelfRenderer (Songs, Videos, Albums, Playlists)
            if let Some(shelf) = section.get("musicShelfRenderer") {
                if let Some(items) = shelf["contents"].as_array() {
                    for item in items {
                        if let Some(renderer) = item.get("musicResponsiveListItemRenderer") {
                            if let Some(parsed) = parse_ytm_responsive_list_item(renderer, scope) {
                                results.push(parsed);
                            }
                        }
                    }
                }
            }

            // 3. ItemSectionRenderer (Wraps musicShelfRenderer or responsive items)
            if let Some(item_section) = section.get("itemSectionRenderer") {
                if let Some(contents) = item_section["contents"].as_array() {
                    for content in contents {
                        if let Some(shelf) = content.get("musicShelfRenderer") {
                            if let Some(items) = shelf["contents"].as_array() {
                                for item in items {
                                    if let Some(renderer) = item.get("musicResponsiveListItemRenderer") {
                                        if let Some(parsed) = parse_ytm_responsive_list_item(renderer, scope) {
                                            results.push(parsed);
                                        }
                                    }
                                }
                            }
                        } else if let Some(renderer) = content.get("musicResponsiveListItemRenderer") {
                            if let Some(parsed) = parse_ytm_responsive_list_item(renderer, scope) {
                                results.push(parsed);
                            }
                        } else if let Some(card) = content.get("musicCardShelfRenderer") {
                            if let Some(item) = parse_ytm_card_shelf(card, scope) {
                                results.push(item);
                            }
                        }
                    }
                }
            }
        }
    }

    results
}

fn parse_ytm_card_shelf(card: &Value, default_scope: SearchScope) -> Option<SearchResultItem> {
    let title = card["title"]["runs"][0]["text"].as_str()?.to_string();
    let video_id = card["title"]["runs"][0]["navigationEndpoint"]["watchEndpoint"]["videoId"]
        .as_str()
        .or_else(|| card["onTap"]["watchEndpoint"]["videoId"].as_str())
        .map(|s| s.to_string());

    let browse_id = card["title"]["runs"][0]["navigationEndpoint"]["browseEndpoint"]["browseId"]
        .as_str()
        .or_else(|| card["onTap"]["browseEndpoint"]["browseId"].as_str())
        .or_else(|| card["header"]["musicCardShelfHeaderBasicRenderer"]["title"]["runs"][0]["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str())
        .or_else(|| {
            card["buttons"].as_array().and_then(|btns| {
                btns.iter().find_map(|b| {
                    b["buttonRenderer"]["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()
                })
            })
        })
        .map(|s| s.to_string());

    let (artist, album, duration_str) = parse_ytm_subtitle_runs(&card["subtitle"]["runs"]);
    let duration_ms = parse_duration_str(&duration_str);

    let mut badges = Vec::new();
    extract_badges(&title, &mut badges);
    extract_card_badges(card, &mut badges);

    // Determine scope / classification
    let scope = if default_scope == SearchScope::Music || default_scope == SearchScope::Video {
        default_scope
    } else if video_id.is_some() {
        SearchScope::Music
    } else if browse_id.as_ref().is_some_and(|b| b.starts_with("MPREb_") || b.starts_with("OLAK5uy_") || b.starts_with("VL")) {
        SearchScope::Album
    } else if browse_id.as_ref().is_some_and(|b| b.starts_with("UC") || b.starts_with("FEmusic_")) {
        SearchScope::Artist
    } else if default_scope == SearchScope::Album {
        SearchScope::Album
    } else if default_scope == SearchScope::Artist {
        SearchScope::Artist
    } else {
        default_scope
    };

    let id = if scope == SearchScope::Album || scope == SearchScope::Artist {
        if let Some(ref bid) = browse_id {
            format!("yt:browse:{}", bid)
        } else if let Some(ref vid) = video_id {
            format!("yt:video:{}", vid)
        } else {
            format!("yt:card:{}", title)
        }
    } else if let Some(ref vid) = video_id {
        format!("yt:video:{}", vid)
    } else if let Some(ref bid) = browse_id {
        format!("yt:browse:{}", bid)
    } else {
        format!("yt:card:{}", title)
    };

    let thumbnail_url = card["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]
        .as_array()
        .and_then(|arr| arr.last())
        .and_then(|t| t["url"].as_str())
        .map(|s| s.to_string());

    let is_playable = video_id.is_some() || scope == SearchScope::Music;

    Some(SearchResultItem {
        id,
        video_id,
        playlist_id: browse_id,
        title,
        artist,
        album,
        duration_ms,
        duration_str,
        badges,
        scope,
        thumbnail_url,
        is_playable,
    })
}

fn parse_ytm_responsive_list_item(renderer: &Value, default_scope: SearchScope) -> Option<SearchResultItem> {
    // 1. Extract video ID or browse ID
    let video_id = renderer["playlistItemData"]["videoId"]
        .as_str()
        .or_else(|| {
            renderer["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]
                ["musicPlayButtonRenderer"]["playNavigationEndpoint"]["watchEndpoint"]["videoId"]
                .as_str()
        })
        .or_else(|| {
            renderer["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
                [0]["navigationEndpoint"]["watchEndpoint"]["videoId"]
                .as_str()
        })
        .map(|s| s.to_string());

    let browse_id = renderer["navigationEndpoint"]["browseEndpoint"]["browseId"]
        .as_str()
        .or_else(|| {
            renderer["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
                [0]["navigationEndpoint"]["browseEndpoint"]["browseId"]
                .as_str()
        })
        .or_else(|| {
            renderer["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]
                ["musicPlayButtonRenderer"]["playNavigationEndpoint"]["browseEndpoint"]["browseId"]
                .as_str()
        })
        .map(|s| s.to_string());

    let flex_cols = renderer["flexColumns"].as_array()?;

    // Column 0: Title
    let raw_title = flex_cols
        .first()?
        ["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"]
        .as_str()?
        .to_string();

    // Column 1: Artist, Album, Duration, Item Type runs
    let (raw_artist, album, duration_str) = if let Some(col1) = flex_cols.get(1) {
        parse_ytm_subtitle_runs(
            &col1["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"],
        )
    } else {
        ("Unknown Artist".to_string(), None, "0:00".to_string())
    };

    let (clean_artist, clean_title) = clean_title_and_artist(&raw_title, Some(&raw_artist));
    let duration_ms = parse_duration_str(&duration_str);

    // Badges extraction
    let mut badges = Vec::new();
    extract_badges(&raw_title, &mut badges);
    extract_list_item_badges(renderer, &mut badges);

    // Determine scope / classification
    let scope = if default_scope == SearchScope::Music || default_scope == SearchScope::Video {
        default_scope
    } else if video_id.is_some() {
        SearchScope::Music
    } else if browse_id.as_ref().is_some_and(|b| b.starts_with("MPREb_") || b.starts_with("OLAK5uy_") || b.starts_with("VL")) {
        SearchScope::Album
    } else if browse_id.as_ref().is_some_and(|b| b.starts_with("UC") || b.starts_with("FEmusic_")) {
        SearchScope::Artist
    } else if default_scope == SearchScope::Album {
        SearchScope::Album
    } else if default_scope == SearchScope::Artist {
        SearchScope::Artist
    } else {
        default_scope
    };

    let is_playable = video_id.is_some() || scope == SearchScope::Music;
    let id = if scope == SearchScope::Album || scope == SearchScope::Artist {
        if let Some(ref bid) = browse_id {
            format!("yt:browse:{}", bid)
        } else if let Some(ref vid) = video_id {
            format!("yt:video:{}", vid)
        } else {
            format!("yt:item:{}", clean_title)
        }
    } else if let Some(ref vid) = video_id {
        format!("yt:video:{}", vid)
    } else if let Some(ref bid) = browse_id {
        format!("yt:browse:{}", bid)
    } else {
        format!("yt:item:{}", clean_title)
    };

    let thumbnail_url = renderer["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]
        .as_array()
        .and_then(|arr| arr.last())
        .and_then(|t| t["url"].as_str())
        .map(|s| s.to_string());

    Some(SearchResultItem {
        id,
        video_id,
        playlist_id: browse_id,
        title: clean_title,
        artist: clean_artist,
        album,
        duration_ms,
        duration_str,
        badges,
        scope,
        thumbnail_url,
        is_playable,
    })
}

fn parse_ytm_subtitle_runs(runs: &Value) -> (String, Option<String>, String) {
    let Some(run_list) = runs.as_array() else {
        return ("Unknown Artist".to_string(), None, "0:00".to_string());
    };

    let mut texts: Vec<&str> = Vec::new();
    for r in run_list {
        if let Some(text) = r["text"].as_str() {
            let trimmed = text.trim();
            if trimmed != "•" && !trimmed.is_empty() {
                texts.push(trimmed);
            }
        }
    }

    if texts.is_empty() {
        return ("Unknown Artist".to_string(), None, "0:00".to_string());
    }

    // Typical pattern:
    // [Artist, Album, Duration] or [ItemType (e.g. Song), Artist, Album, Duration]
    // Filter out item type prefixes if present
    if texts.first().is_some_and(|&t| t.eq_ignore_ascii_case("song") || t.eq_ignore_ascii_case("video")) {
        texts.remove(0);
    }

    let mut duration_str = "0:00".to_string();
    if let Some(last) = texts.last() {
        if is_duration_text(last) {
            duration_str = last.to_string();
            texts.pop();
        }
    }

    let artist = texts.first().copied().unwrap_or("Unknown Artist").to_string();
    let album = if texts.len() > 1 {
        Some(texts[1..].join(" "))
    } else {
        None
    };

    (artist, album, duration_str)
}

/// Parses YouTube Web (fallback) JSON response
pub fn parse_yt_web_search_response(json: &Value, default_scope: SearchScope) -> Vec<SearchResultItem> {
    let mut results = Vec::new();

    let contents = &json["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]
        ["sectionListRenderer"]["contents"];

    let Some(section_list) = contents.as_array() else {
        return results;
    };

    for section in section_list {
        let Some(items) = section["itemSectionRenderer"]["contents"].as_array() else {
            continue;
        };

        for item in items {
            if let Some(video) = item.get("videoRenderer") {
                let Some(vid) = video["videoId"].as_str() else {
                    continue;
                };

                let raw_title = video["title"]["runs"][0]["text"]
                    .as_str()
                    .unwrap_or("Unknown Title");

                let raw_author = video["ownerText"]["runs"][0]["text"]
                    .as_str()
                    .unwrap_or("Unknown Artist");

                let duration_str = video["lengthText"]["simpleText"]
                    .as_str()
                    .unwrap_or("0:00")
                    .to_string();

                let (clean_artist, clean_title) = clean_title_and_artist(raw_title, Some(raw_author));
                let duration_ms = parse_duration_str(&duration_str);

                let mut badges = Vec::new();
                extract_badges(raw_title, &mut badges);

                if let Some(badge_arr) = video["badges"].as_array() {
                    for b in badge_arr {
                        if let Some(label) = b["metadataBadgeRenderer"]["label"].as_str() {
                            if label.eq_ignore_ascii_case("official") {
                                if !badges.contains(&SearchBadge::Official) {
                                    badges.push(SearchBadge::Official);
                                }
                            }
                        }
                    }
                }

                let thumbnail_url = video["thumbnail"]["thumbnails"]
                    .as_array()
                    .and_then(|arr| arr.last())
                    .and_then(|t| t["url"].as_str())
                    .map(|s| s.to_string());

                results.push(SearchResultItem {
                    id: format!("yt:video:{}", vid),
                    video_id: Some(vid.to_string()),
                    playlist_id: None,
                    title: clean_title,
                    artist: clean_artist,
                    album: None,
                    duration_ms,
                    duration_str,
                    badges,
                    scope: default_scope,
                    thumbnail_url,
                    is_playable: true,
                });
            }
        }
    }

    results
}

/// Parses public suggestqueries JSON array: ["query", ["sug1", "sug2", ...]]
pub fn parse_suggestqueries_response(json: &Value) -> Result<Vec<String>> {
    let arr = json
        .as_array()
        .ok_or_else(|| anyhow!("expected JSON array"))?;

    let suggestions = arr
        .get(1)
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(suggestions)
}

/// Parses YTM get_search_suggestions response
pub fn parse_ytm_suggestions_response(json: &Value) -> Vec<String> {
    let mut suggestions = Vec::new();

    if let Some(contents) = json["contents"][0]["searchSuggestionsSectionRenderer"]["contents"].as_array() {
        for item in contents {
            if let Some(runs) = item["searchSuggestionRenderer"]["suggestion"]["runs"].as_array() {
                let text: String = runs
                    .iter()
                    .filter_map(|r| r["text"].as_str())
                    .collect();
                if !text.is_empty() {
                    suggestions.push(text);
                }
            }
        }
    }

    suggestions
}

// -----------------------------------------------------------------------------
// Badge & Metadata Helpers
// -----------------------------------------------------------------------------

fn extract_list_item_badges(renderer: &Value, badges: &mut Vec<SearchBadge>) {
    if let Some(badge_arr) = renderer["badges"].as_array() {
        for b in badge_arr {
            let label = b["musicInlineBadgeRenderer"]["accessibilityData"]["accessibilityData"]["label"]
                .as_str()
                .unwrap_or("");
            let icon = b["musicInlineBadgeRenderer"]["icon"]["iconType"]
                .as_str()
                .unwrap_or("");

            if label.eq_ignore_ascii_case("explicit") || icon.contains("EXPLICIT") {
                if !badges.contains(&SearchBadge::Explicit) {
                    badges.push(SearchBadge::Explicit);
                }
            }
        }
    }
}

fn extract_card_badges(card: &Value, badges: &mut Vec<SearchBadge>) {
    if let Some(badge_arr) = card["badges"].as_array() {
        for b in badge_arr {
            let label = b["musicInlineBadgeRenderer"]["accessibilityData"]["accessibilityData"]["label"]
                .as_str()
                .unwrap_or("");
            if label.eq_ignore_ascii_case("explicit") {
                if !badges.contains(&SearchBadge::Explicit) {
                    badges.push(SearchBadge::Explicit);
                }
            }
        }
    }
}

fn extract_badges(title: &str, badges: &mut Vec<SearchBadge>) {
    let lower = title.to_lowercase();
    if (lower.contains("official") || lower.contains("official video") || lower.contains("official audio"))
        && !badges.contains(&SearchBadge::Official)
    {
        badges.push(SearchBadge::Official);
    }
    if (lower.contains("remaster") || lower.contains("remastered"))
        && !badges.contains(&SearchBadge::Remaster)
    {
        badges.push(SearchBadge::Remaster);
    }
    if (lower.contains("live at") || lower.contains("live from") || lower.contains("(live)") || lower.contains("[live]"))
        && !badges.contains(&SearchBadge::Live)
    {
        badges.push(SearchBadge::Live);
    }
    if (lower.contains("[explicit]") || lower.contains("(explicit)"))
        && !badges.contains(&SearchBadge::Explicit)
    {
        badges.push(SearchBadge::Explicit);
    }
}

/// Cleans video titles into (clean_artist, clean_title)
pub fn clean_title_and_artist(raw_title: &str, uploader: Option<&str>) -> (String, String) {
    let mut title = raw_title.to_string();

    let noise_patterns = [
        r"(?i)\s*\[[^\]]*(official|video|audio|lyric|mv|remaster|visualizer|explicit|clean|4k|hd)[^\]]*\]",
        r"(?i)\s*\([^)]*(official|video|audio|lyric|mv|remaster|visualizer|explicit|clean|4k|hd)[^)]*\)",
    ];

    for pat in &noise_patterns {
        if let Ok(re) = Regex::new(pat) {
            title = re.replace_all(&title, "").to_string();
        }
    }

    title = title.trim().to_string();

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

    let separators = [" - ", " – ", " — ", " : "];
    for sep in &separators {
        if let Some((left, right)) = title.split_once(sep) {
            let candidate_artist = left.trim().trim_matches('"').trim_matches('\'').trim();
            let candidate_title = right.trim().trim_matches('"').trim_matches('\'').trim();

            if !candidate_artist.is_empty() && !candidate_title.is_empty() {
                return (candidate_artist.to_string(), candidate_title.to_string());
            }
        }
    }

    let artist = clean_uploader.unwrap_or_else(|| "Unknown Artist".to_string());
    (artist, title)
}

/// Checks if string has duration format like "3:45" or "1:02:30"
pub fn is_duration_text(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }
    parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
}

/// Parses "3:45" or "1:02:30" into milliseconds
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
