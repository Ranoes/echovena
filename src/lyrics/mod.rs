pub mod cjk_data;
pub mod lrclib_elrc;
pub mod netease_yrc;
pub mod paxsenix_ttml;
pub mod romanize;
pub mod spotify_lyrics;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum KaraokeDisplayMode {
    #[default]
    Original,
    Romanized,
    Both,
}

impl KaraokeDisplayMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Original => Self::Romanized,
            Self::Romanized => Self::Both,
            Self::Both => Self::Original,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Original => "Orig",
            Self::Romanized => "Rom",
            Self::Both => "Both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum KaraokeAlignment {
    Left,
    #[default]
    Center,
    Right,
}

impl KaraokeAlignment {
    pub fn cycle(self) -> Self {
        match self {
            Self::Left => Self::Center,
            Self::Center => Self::Right,
            Self::Right => Self::Left,
        }
    }

    pub fn to_ratatui(self) -> ratatui::layout::Alignment {
        match self {
            Self::Left => ratatui::layout::Alignment::Left,
            Self::Center => ratatui::layout::Alignment::Center,
            Self::Right => ratatui::layout::Alignment::Right,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Middle / Center",
            Self::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LyricProviderChoice {
    #[default]
    Auto,      // Waterfall priority
    Lrclib,    // LRCLIB (.elrc)
    Paxsenix,  // Apple Music TTML / Musixmatch RichSync
    Spotify,   // Spotify Color Lyrics
    Netease,   // NetEase (.yrc)
}

impl LyricProviderChoice {
    pub fn cycle(self) -> Self {
        match self {
            Self::Auto => Self::Lrclib,
            Self::Lrclib => Self::Paxsenix,
            Self::Paxsenix => Self::Spotify,
            Self::Spotify => Self::Netease,
            Self::Netease => Self::Auto,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto (Waterfall)",
            Self::Lrclib => "LRCLIB (.elrc)",
            Self::Paxsenix => "Paxsenix (Apple Music/Musixmatch)",
            Self::Spotify => "Spotify Color Lyrics",
            Self::Netease => "NetEase (.yrc)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncType {
    Syllable, // Word-by-word / Syllable-synced (Karaoke)
    Line,     // Line-synced
    Unsynced,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VocalRole {
    Lead1,      // v1 - Left-aligned, Cyan
    Lead2,      // v2 - Right-aligned, Magenta
    Background, // x-bg - Dimmed italics below line
}

impl Default for VocalRole {
    fn default() -> Self {
        VocalRole::Lead1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KaraokeWord {
    pub text: String,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    #[serde(default)]
    pub romanization: Option<String>,
}

impl KaraokeWord {
    pub fn new(text: &str, start_time_ms: i64, end_time_ms: i64) -> Self {
        Self {
            text: text.to_string(),
            start_time_ms,
            end_time_ms,
            romanization: None,
        }
    }

    /// Progress through this word from 0.0 to 1.0
    pub fn progress(&self, current_time_ms: i64) -> f64 {
        if current_time_ms <= self.start_time_ms {
            0.0
        } else if current_time_ms >= self.end_time_ms {
            1.0
        } else {
            let duration = (self.end_time_ms - self.start_time_ms).max(1) as f64;
            ((current_time_ms - self.start_time_ms) as f64 / duration).clamp(0.0, 1.0)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KaraokeLine {
    pub text: String,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    #[serde(default)]
    pub words: Vec<KaraokeWord>,
    #[serde(default)]
    pub vocal_role: VocalRole,
    #[serde(default)]
    pub romanization: Option<String>,
    #[serde(default)]
    pub translation: Option<String>,
}

impl KaraokeLine {
    pub fn active_word_index(&self, current_time_ms: i64) -> Option<usize> {
        if self.words.is_empty() {
            return None;
        }
        for (idx, w) in self.words.iter().enumerate() {
            if current_time_ms >= w.start_time_ms && current_time_ms <= w.end_time_ms {
                return Some(idx);
            }
        }
        // If between words on the active line, return the upcoming word
        for (idx, w) in self.words.iter().enumerate() {
            if current_time_ms < w.start_time_ms {
                return Some(idx.saturating_sub(1));
            }
        }
        Some(self.words.len() - 1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KaraokeTimeline {
    pub provider: String,
    pub sync_type: SyncType,
    pub lines: Vec<KaraokeLine>,
    #[serde(default)]
    pub raw_elrc: Option<String>,
}

impl KaraokeTimeline {
    pub fn active_line_index(&self, current_time_ms: i64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        for (idx, line) in self.lines.iter().enumerate() {
            if current_time_ms >= line.start_time_ms && current_time_ms <= line.end_time_ms {
                return Some(idx);
            }
        }
        // If in between lines or before first line
        for (idx, line) in self.lines.iter().enumerate() {
            if current_time_ms < line.start_time_ms {
                return if idx == 0 { None } else { Some(idx - 1) };
            }
        }
        Some(self.lines.len() - 1)
    }

    /// Pre-calculates and caches algorithmic romanization for all lines and words
    /// so that live 60 FPS karaoke stage rendering has 0ms latency.
    pub fn ensure_romanization(&mut self) {
        let has_kana = self.lines.iter().any(|l| {
            l.text.chars().any(|c| ('\u{3040}'..='\u{309F}').contains(&c) || ('\u{30A0}'..='\u{30FF}').contains(&c))
        });
        let has_cjk = self.lines.iter().any(|l| {
            l.text.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c) || ('\u{3400}'..='\u{4DBF}').contains(&c))
        });
        let is_chinese = has_cjk && !has_kana;

        for line in &mut self.lines {
            let needs_rom = match &line.romanization {
                None => true,
                Some(r) => crate::lyrics::romanize::needs_romanization(r),
            };
            if needs_rom {
                line.romanization = crate::lyrics::romanize::romanize_text_with_mode(&line.text, is_chinese);
            }
            for word in &mut line.words {
                let word_needs_rom = match &word.romanization {
                    None => true,
                    Some(r) => crate::lyrics::romanize::needs_romanization(r),
                };
                if word_needs_rom {
                    word.romanization = crate::lyrics::romanize::romanize_text_with_mode(&word.text, is_chinese);
                }
            }
        }
    }

    /// Converts the timeline into an exportable Enhanced LRC (.elrc) format
    pub fn to_enhanced_lrc(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let l_min = (line.start_time_ms / 60000) % 60;
            let l_sec = (line.start_time_ms / 1000) % 60;
            let l_hun = (line.start_time_ms % 1000) / 10;
            out.push_str(&format!("[{:02}:{:02}.{:02}]", l_min, l_sec, l_hun));

            if !line.words.is_empty() {
                for word in &line.words {
                    let w_min = (word.start_time_ms / 60000) % 60;
                    let w_sec = (word.start_time_ms / 1000) % 60;
                    let w_hun = (word.start_time_ms % 1000) / 10;
                    out.push_str(&format!("<{:02}:{:02}.{:02}>{}", w_min, w_sec, w_hun, word.text));
                }
            } else {
                out.push(' ');
                out.push_str(&line.text);
            }
            out.push('\n');
        }
        out
    }
}

pub struct LyricWaterfall {
    registry: crate::config::providers::ProviderRegistry,
    client: reqwest::Client,
}

impl LyricWaterfall {
    pub fn new(registry: crate::config::providers::ProviderRegistry) -> Self {
        Self {
            registry,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(4500))
                .build()
                .unwrap(),
        }
    }

    /// Resolves lyrics for a track using SQLite cache and priority waterfall:
    /// Paxsenix (TTML/RichSync) -> Spotify (Color Lyrics/Bypass) -> LRCLIB (.elrc) -> NetEase (.yrc)
    pub async fn resolve_lyrics(
        &self,
        db: &crate::sources::lossless_relay::db::Database,
        source_track_id: &str,
        isrc: Option<&str>,
        artist: &str,
        clean_title: &str,
        album: Option<&str>,
        duration_ms: i64,
    ) -> anyhow::Result<KaraokeTimeline> {
        // 1. Check local SQLite lyrics_cache
        if let Ok(Some(cached)) = db.get_lyrics(source_track_id) {
            if let Ok(mut timeline) = serde_json::from_str::<KaraokeTimeline>(&cached.timeline_json) {
                timeline.ensure_romanization();
                return Ok(timeline);
            }
        }

        let mut providers = self.registry.lyric_providers.clone();
        providers.sort_by_key(|p| p.priority);

        for provider in &providers {
            if !provider.enabled {
                continue;
            }

            match provider.provider_type.as_str() {
                "paxsenix-karaoke" => {
                    let mut query = vec![];
                    if let Some(i) = isrc {
                        if !i.trim().is_empty() {
                            query.push(("isrc", i));
                        }
                    }
                    let q_str = format!("{} {}", artist, clean_title);
                    if query.is_empty() {
                        query.push(("q", &q_str));
                    }

                    let url = format!("{}/lyrics", provider.base_url.trim_end_matches('/'));
                    if let Ok(resp) = self.client.get(&url).query(&query).send().await {
                        if resp.status().is_success() {
                            if let Ok(text) = resp.text().await {
                                let timeline = if text.trim_start().starts_with('<') {
                                    paxsenix_ttml::parse_apple_music_ttml(&text, &provider.id)
                                } else {
                                    paxsenix_ttml::parse_musixmatch_richsync(&text, &provider.id)
                                };
                                if let Ok(tl) = timeline {
                                    self.cache_timeline(db, source_track_id, isrc, &tl).ok();
                                    return Ok(tl);
                                }
                            }
                        }
                    }
                }
                "spotify-spclient" => {
                    if let Some(sp_id) = source_track_id.strip_prefix("spotify:track:") {
                        if let Ok(Some(sp_dc)) = crate::sources::lossless_relay::spotify_cookie_sync::load_encrypted_sp_dc() {
                            let scraper = crate::sources::lossless_relay::spotify_scraper::SpotifyScraper::new();
                            if let Ok(token) = scraper.get_token().await {
                                let client = spotify_lyrics::SpotifyLyricsClient::new();
                                if let Ok(tl) = client.fetch_internal_lyrics(sp_id, &token, &sp_dc).await {
                                    self.cache_timeline(db, source_track_id, isrc, &tl).ok();
                                    return Ok(tl);
                                }
                            }
                        }
                    }
                }
                "spotify-lyrics-bridge" => {
                    if let Some(sp_id) = source_track_id.strip_prefix("spotify:track:") {
                        let client = spotify_lyrics::SpotifyLyricsClient::new();
                        if let Ok(tl) = client.fetch_bypass_lyrics(&provider.base_url, sp_id).await {
                            self.cache_timeline(db, source_track_id, isrc, &tl).ok();
                            return Ok(tl);
                        }
                    }
                }
                "lrclib" => {
                    let client = lrclib_elrc::LrclibClient::new(&provider.base_url);
                    let dur_sec = if duration_ms > 0 { Some(duration_ms / 1000) } else { None };
                    if let Ok(tl) = client.fetch_lyrics(clean_title, artist, album, dur_sec).await {
                        self.cache_timeline(db, source_track_id, isrc, &tl).ok();
                        return Ok(tl);
                    }
                }
                "netease" => {
                    // Try NetEase if configured
                }
                _ => {}
            }
        }

        anyhow::bail!("no lyrics found for track {}", source_track_id)
    }

    fn cache_timeline(
        &self,
        db: &crate::sources::lossless_relay::db::Database,
        source_track_id: &str,
        isrc: Option<&str>,
        timeline: &KaraokeTimeline,
    ) -> anyhow::Result<()> {
        let json = serde_json::to_string(timeline)?;
        let sync_type_str = match timeline.sync_type {
            SyncType::Syllable => "SYLLABLE",
            SyncType::Line => "LINE",
            SyncType::Unsynced => "UNSYNCED",
        };
        let entry = crate::sources::lossless_relay::db::LyricsEntry {
            source_track_id: source_track_id.to_string(),
            isrc: isrc.map(|s| s.to_string()),
            provider: timeline.provider.clone(),
            sync_type: sync_type_str.to_string(),
            timeline_json: json,
            raw_elrc: timeline.raw_elrc.clone(),
            user_offset_ms: 0,
            fetched_at: chrono::Utc::now().timestamp(),
        };
        db.upsert_lyrics(&entry)
    }
}
