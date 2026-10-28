use std::fmt;

/// Scopes for YouTube / YouTube Music search
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchScope {
    #[default]
    All,
    Music,
    Album,
    Artist,
    Video,
}

impl SearchScope {
    /// Returns the display label for the scope tab
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Music => "MUSIC",
            Self::Album => "ALBUM",
            Self::Artist => "ARTIST",
            Self::Video => "VIDEO",
        }
    }

    /// Short label for tight layouts
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Music => "MUSIC",
            Self::Album => "ALBUM",
            Self::Artist => "ARTIST",
            Self::Video => "VIDEO",
        }
    }

    /// InnerTube search filter params for YouTube Music
    pub fn innertube_params(&self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Music => Some("EgWKAQIIAWoKEAkQBRAKEAMQBBAF"),
            Self::Album => Some("EgWKAQIBAWoKEAkQBRAKEAMQBBAF"),
            Self::Artist => Some("EgWKAQIgAWoKEAkQBRAKEAMQBBAF"),
            Self::Video => Some("EgWKAQIQAWoKEAkQBRAKEAMQBBAF"),
        }
    }

    /// Cycles forward to the next scope (e.g. on Tab)
    pub fn cycle_next(&self) -> Self {
        match self {
            Self::All => Self::Music,
            Self::Music => Self::Album,
            Self::Album => Self::Artist,
            Self::Artist => Self::Video,
            Self::Video => Self::All,
        }
    }

    /// Alias for cycle_next
    pub fn cycle(&self) -> Self {
        self.cycle_next()
    }

    /// Cycles backward to the previous scope (e.g. on Shift+Tab)
    pub fn cycle_prev(&self) -> Self {
        match self {
            Self::All => Self::Video,
            Self::Video => Self::Artist,
            Self::Artist => Self::Album,
            Self::Album => Self::Music,
            Self::Music => Self::All,
        }
    }

    /// Alias for cycle_prev
    pub fn cycle_back(&self) -> Self {
        self.cycle_prev()
    }
}

impl fmt::Display for SearchScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Metadata badges for search result items
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SearchBadge {
    Official,
    Explicit,
    Remaster,
    Live,
    Custom(String),
}

impl SearchBadge {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Official => "OFFICIAL",
            Self::Explicit => "EXPLICIT",
            Self::Remaster => "REMASTER",
            Self::Live => "LIVE",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl fmt::Display for SearchBadge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A parsed search result item from YouTube / YouTube Music
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResultItem {
    /// Canonical source ID (e.g. `yt:video:abc123xyz` or `yt:playlist:abc123xyz`)
    pub id: String,
    /// Video ID if this item is a playable video/track
    pub video_id: Option<String>,
    /// Playlist or browse ID if this item is an album/playlist
    pub playlist_id: Option<String>,
    /// Track or collection title
    pub title: String,
    /// Artist or channel name
    pub artist: String,
    /// Album or release name
    pub album: Option<String>,
    /// Duration in milliseconds (0 if unknown or playlist)
    pub duration_ms: i64,
    /// Human-readable duration string (e.g. "3:45")
    pub duration_str: String,
    /// Badges (Official, Explicit, Remaster, etc.)
    pub badges: Vec<SearchBadge>,
    /// Scope / item classification
    pub scope: SearchScope,
    /// Thumbnail URL
    pub thumbnail_url: Option<String>,
    /// True if directly playable as an audio track
    pub is_playable: bool,
}

impl SearchResultItem {
    /// Converts this search result to a TrackInfo compatible with EchoVena's queue & playback
    pub fn to_track_info(&self) -> crate::core::plugin_api::TrackInfo {
        let uri = if let Some(ref vid) = self.video_id {
            format!("youtube:{}", vid)
        } else {
            self.id.clone()
        };

        crate::core::plugin_api::TrackInfo {
            uri: Some(uri),
            name: self.title.clone(),
            artists: vec![self.artist.clone()],
            album: self.album.clone().unwrap_or_default(),
            duration_ms: self.duration_ms.max(0) as u64,
            id: self.video_id.clone(),
            album_id: self.playlist_id.clone(),
            artist_refs: Vec::new(),
            is_playable: self.is_playable,
            is_local: false,
            track_number: 0,
            explicit: self.badges.contains(&SearchBadge::Explicit),
            image_url: self.thumbnail_url.clone(),
            release_date: None,
            generated_name: None,
        }
    }
}

/// Aggregated search response with results and autocomplete suggestions
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchResponse {
    pub query: String,
    pub scope: SearchScope,
    pub items: Vec<SearchResultItem>,
    pub suggestions: Vec<String>,
}
