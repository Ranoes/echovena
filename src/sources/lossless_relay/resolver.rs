use crate::config::providers::ProviderRegistry;
use crate::sources::lossless_relay::db::{Database, PlaylistTrack, ResolutionEntry};
use anyhow::{anyhow, Result};
use chrono::Utc;
use reqwest::header::{HeaderValue, USER_AGENT};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;

const SONGLINK_API_URL: &str = "https://api.song.link/v1-alpha.1/links";
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const CACHE_TTL_SECONDS: i64 = 30 * 86400; // 30-day TTL

/// Computes the Jaro-Winkler string similarity between two strings (0.0 to 1.0)
pub fn jaro_winkler_similarity(s1: &str, s2: &str) -> f64 {
    let s1_clean = s1.trim().to_lowercase();
    let s2_clean = s2.trim().to_lowercase();

    if s1_clean == s2_clean {
        return 1.0;
    }
    if s1_clean.is_empty() || s2_clean.is_empty() {
        return 0.0;
    }

    let s1_chars: Vec<char> = s1_clean.chars().collect();
    let s2_chars: Vec<char> = s2_clean.chars().collect();

    let len1 = s1_chars.len();
    let len2 = s2_chars.len();

    let match_window = (len1.max(len2) / 2).saturating_sub(1);

    let mut s1_matches = vec![false; len1];
    let mut s2_matches = vec![false; len2];

    let mut matches = 0usize;

    for i in 0..len1 {
        let start = i.saturating_sub(match_window);
        let end = (i + match_window + 1).min(len2);

        for j in start..end {
            if s2_matches[j] || s1_chars[i] != s2_chars[j] {
                continue;
            }
            s1_matches[i] = true;
            s2_matches[j] = true;
            matches += 1;
            break;
        }
    }

    if matches == 0 {
        return 0.0;
    }

    // Transpositions
    let mut transpositions = 0usize;
    let mut k = 0usize;
    for i in 0..len1 {
        if !s1_matches[i] {
            continue;
        }
        while !s2_matches[k] {
            k += 1;
        }
        if s1_chars[i] != s2_chars[k] {
            transpositions += 1;
        }
        k += 1;
    }

    let m = matches as f64;
    let t = (transpositions as f64) / 2.0;
    let jaro = (m / (len1 as f64) + m / (len2 as f64) + (m - t) / m) / 3.0;

    // Common prefix up to 4 characters
    let mut prefix_len = 0usize;
    for i in 0..len1.min(len2).min(4) {
        if s1_chars[i] == s2_chars[i] {
            prefix_len += 1;
        } else {
            break;
        }
    }

    let prefix_scale = 0.1;
    let jaro_winkler = jaro + (prefix_len as f64) * prefix_scale * (1.0 - jaro);
    jaro_winkler.min(1.0)
}

/// Checks if duration matches within platform-specific tolerance
pub fn is_duration_match(source_duration_ms: i64, candidate_duration_ms: i64, is_youtube: bool) -> bool {
    let tolerance_ms = if is_youtube { 4000 } else { 2000 };
    (source_duration_ms - candidate_duration_ms).abs() <= tolerance_ms
}

#[derive(Debug, Clone)]
pub struct CandidateTrack {
    pub provider: String, // "qobuz" | "tidal"
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: i64,
    pub isrc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTrack {
    pub source_track_id: String,
    pub isrc: Option<String>,
    pub qobuz_id: Option<String>,
    pub tidal_id: Option<String>,
    pub resolution_method: String,
    pub match_confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low,
    High,
}

#[derive(Clone)]
pub struct Resolver {
    client: reqwest::Client,
    db: Database,
    providers: Arc<Mutex<Option<ProviderRegistry>>>,
}

impl Resolver {
    pub fn new(db: Database) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_millis(4500))
                .build()
                .unwrap(),
            db,
            providers: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_providers(db: Database, providers: ProviderRegistry) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_millis(providers.waterfall_timeout_ms.max(2000)))
                .build()
                .unwrap(),
            db,
            providers: Arc::new(Mutex::new(Some(providers))),
        }
    }

    /// Sets or updates the active ProviderRegistry
    pub async fn set_providers(&self, registry: ProviderRegistry) {
        let mut p = self.providers.lock().await;
        *p = Some(registry);
    }

    /// Resolves a single track using the 4-step resolution pipeline:
    /// Cache -> ISRC Direct -> Songlink -> Fuzzy Jaro-Winkler -> YT Direct Fallback
    pub async fn resolve_track(&self, track: &PlaylistTrack) -> Result<ResolvedTrack> {
        let now = Utc::now().timestamp();

        // Step 0: Check SQLite cache with 30-day TTL
        if let Ok(Some(cached)) = self.db.get_resolution(&track.source_track_id) {
            if (now - cached.resolved_at) < CACHE_TTL_SECONDS {
                return Ok(ResolvedTrack {
                    source_track_id: cached.source_track_id,
                    isrc: cached.isrc,
                    qobuz_id: cached.qobuz_id,
                    tidal_id: cached.tidal_id,
                    resolution_method: cached.resolution_method,
                    match_confidence: cached.match_confidence,
                });
            }
        }

        let is_youtube = track.source_track_id.starts_with("yt:video:");

        // Step 1: Direct Provider Catalog ISRC Lookup
        if let Some(ref isrc) = track.isrc {
            if !isrc.trim().is_empty() {
                if let Ok(Some(res)) = self.query_catalog_by_isrc(isrc, &track.source_track_id).await {
                    self.save_resolution(&res).ok();
                    return Ok(res);
                }
            }
        }

        // Step 2: Songlink / Odesli Cross-Platform Bridge
        if let Ok(Some(res)) = self.query_songlink(&track.source_track_id).await {
            self.save_resolution(&res).ok();
            return Ok(res);
        }

        // Step 3: Duration-Gated Fuzzy Match
        if let Ok(Some(res)) = self
            .query_fuzzy_catalog(
                &track.source_track_id,
                &track.artist,
                &track.clean_title,
                track.duration_ms,
                is_youtube,
            )
            .await
        {
            self.save_resolution(&res).ok();
            return Ok(res);
        }

        // Step 4: YouTube Direct Fallback (for uncataloged remixes, mixes, or sets)
        if is_youtube {
            let res = ResolvedTrack {
                source_track_id: track.source_track_id.clone(),
                isrc: track.isrc.clone(),
                qobuz_id: None,
                tidal_id: None,
                resolution_method: "yt_direct_fallback".to_string(),
                match_confidence: 1.0,
            };
            self.save_resolution(&res).ok();
            return Ok(res);
        }

        Err(anyhow!(
            "unable to resolve lossless relay track for: {}",
            track.source_track_id
        ))
    }

    /// Queries the Songlink/Odesli API for Spotify or YouTube track URLs
    pub async fn query_songlink(&self, source_track_id: &str) -> Result<Option<ResolvedTrack>> {
        let lookup_url = if let Some(sp_id) = source_track_id.strip_prefix("spotify:track:") {
            format!("https://open.spotify.com/track/{}", sp_id)
        } else if let Some(yt_id) = source_track_id.strip_prefix("yt:video:") {
            format!("https://music.youtube.com/watch?v={}", yt_id)
        } else {
            return Ok(None);
        };

        let resp = self
            .client
            .get(SONGLINK_API_URL)
            .query(&[("url", &lookup_url)])
            .header(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT))
            .send()
            .await;

        let Ok(response) = resp else {
            return Ok(None);
        };

        if !response.status().is_success() {
            return Ok(None);
        }

        let body: Value = match response.json().await {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };

        let mut tidal_id = None;
        let mut qobuz_id = None;
        let mut isrc = None;

        if let Some(links) = body.get("linksByPlatform") {
            if let Some(tidal) = links.get("tidal") {
                if let Some(uid) = tidal["entityUniqueId"].as_str() {
                    tidal_id = Some(uid.trim_start_matches("TIDAL_SONG::").to_string());
                }
            }
            if let Some(qobuz) = links.get("qobuz") {
                if let Some(uid) = qobuz["entityUniqueId"].as_str() {
                    qobuz_id = Some(uid.trim_start_matches("QOBUZ_SONG::").to_string());
                }
            }
        }

        if let Some(entities) = body.get("entitiesByUniqueId").and_then(|e| e.as_object()) {
            for (_, val) in entities {
                if let Some(cand_isrc) = val["isrc"].as_str() {
                    if !cand_isrc.is_empty() {
                        isrc = Some(cand_isrc.to_string());
                        break;
                    }
                }
            }
        }

        if tidal_id.is_some() || qobuz_id.is_some() {
            Ok(Some(ResolvedTrack {
                source_track_id: source_track_id.to_string(),
                isrc,
                qobuz_id,
                tidal_id,
                resolution_method: "songlink".to_string(),
                match_confidence: 0.95,
            }))
        } else {
            Ok(None)
        }
    }

    /// Queries configured provider catalog endpoints directly by ISRC
    pub async fn query_catalog_by_isrc(
        &self,
        isrc: &str,
        source_track_id: &str,
    ) -> Result<Option<ResolvedTrack>> {
        let registry = {
            let guard = self.providers.lock().await;
            guard.clone()
        };

        let Some(registry) = registry else {
            return Ok(None);
        };

        let mut qobuz_id = None;
        let mut tidal_id = None;

        for provider in &registry.audio_providers {
            if !provider.enabled {
                continue;
            }

            match provider.provider_type.as_str() {
                "qobuz-rest-api" => {
                    if let Some(ref base) = provider.base_url {
                        let url = format!("{}/catalog/track/search", base.trim_end_matches('/'));
                        if let Ok(resp) = self.client.get(&url).query(&[("isrc", isrc)]).send().await {
                            if resp.status().is_success() {
                                if let Ok(val) = resp.json::<Value>().await {
                                    if let Some(id) = val["track"]["id"].as_i64().or_else(|| val["id"].as_i64()) {
                                        qobuz_id = Some(id.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                "hifi-api" => {
                    if let Some(ref catalog) = provider.catalog_url {
                        let url = format!("{}/search/", catalog.trim_end_matches('/'));
                        if let Ok(resp) = self
                            .client
                            .get(&url)
                            .query(&[("query", isrc), ("filter", "tracks")])
                            .send()
                            .await
                        {
                            if resp.status().is_success() {
                                if let Ok(val) = resp.json::<Value>().await {
                                    if let Some(items) = val["tracks"]["items"].as_array() {
                                        if let Some(first) = items.first() {
                                            if let Some(id) = first["id"].as_i64().or_else(|| first["id"].as_str().and_then(|s| s.parse().ok())) {
                                                tidal_id = Some(id.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            if qobuz_id.is_some() && tidal_id.is_some() {
                break;
            }
        }

        if qobuz_id.is_some() || tidal_id.is_some() {
            Ok(Some(ResolvedTrack {
                source_track_id: source_track_id.to_string(),
                isrc: Some(isrc.to_string()),
                qobuz_id,
                tidal_id,
                resolution_method: "isrc_exact".to_string(),
                match_confidence: 1.0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Queries configured provider catalog endpoints with fuzzy matching (Jaro-Winkler >= 0.85 + Duration Gate)
    pub async fn query_fuzzy_catalog(
        &self,
        source_track_id: &str,
        artist: &str,
        title: &str,
        source_duration_ms: i64,
        is_youtube: bool,
    ) -> Result<Option<ResolvedTrack>> {
        let registry = {
            let guard = self.providers.lock().await;
            guard.clone()
        };

        let Some(registry) = registry else {
            return Ok(None);
        };

        let query = format!("{} {}", artist, title);
        let mut best_qobuz: Option<(String, f64)> = None;
        let mut best_tidal: Option<(String, f64)> = None;

        for provider in &registry.audio_providers {
            if !provider.enabled {
                continue;
            }

            match provider.provider_type.as_str() {
                "qobuz-rest-api" => {
                    if let Some(ref base) = provider.base_url {
                        let url = format!("{}/catalog/track/search", base.trim_end_matches('/'));
                        if let Ok(resp) = self
                            .client
                            .get(&url)
                            .query(&[("query", query.as_str()), ("limit", "5")])
                            .send()
                            .await
                        {
                            if resp.status().is_success() {
                                if let Ok(val) = resp.json::<Value>().await {
                                    if let Some(tracks) = val["tracks"]["items"].as_array() {
                                        for item in tracks {
                                            let cand_title = item["title"].as_str().unwrap_or("");
                                            let cand_artist = item["performer"]["name"].as_str().unwrap_or("");
                                            let cand_dur = item["duration"].as_i64().unwrap_or(0) * 1000;
                                            let cand_id = item["id"].as_i64().map(|i| i.to_string());

                                            if let Some(id) = cand_id {
                                                if is_duration_match(source_duration_ms, cand_dur, is_youtube) {
                                                    let t_sim = jaro_winkler_similarity(title, cand_title);
                                                    let a_sim = jaro_winkler_similarity(artist, cand_artist);
                                                    let avg_sim = (t_sim + a_sim) / 2.0;
                                                    if avg_sim >= 0.85 {
                                                        if best_qobuz.as_ref().map_or(true, |(_, s)| avg_sim > *s) {
                                                            best_qobuz = Some((id, avg_sim));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                "hifi-api" => {
                    if let Some(ref catalog) = provider.catalog_url {
                        let url = format!("{}/search/", catalog.trim_end_matches('/'));
                        if let Ok(resp) = self
                            .client
                            .get(&url)
                            .query(&[("query", query.as_str()), ("filter", "tracks"), ("limit", "5")])
                            .send()
                            .await
                        {
                            if resp.status().is_success() {
                                if let Ok(val) = resp.json::<Value>().await {
                                    if let Some(tracks) = val["tracks"]["items"].as_array() {
                                        for item in tracks {
                                            let cand_title = item["title"].as_str().unwrap_or("");
                                            let cand_artist = item["artist"]["name"]
                                                .as_str()
                                                .or_else(|| item["artists"][0]["name"].as_str())
                                                .unwrap_or("");
                                            let cand_dur = item["duration"].as_i64().unwrap_or(0) * 1000;
                                            let cand_id = item["id"].as_i64().map(|i| i.to_string());

                                            if let Some(id) = cand_id {
                                                if is_duration_match(source_duration_ms, cand_dur, is_youtube) {
                                                    let t_sim = jaro_winkler_similarity(title, cand_title);
                                                    let a_sim = jaro_winkler_similarity(artist, cand_artist);
                                                    let avg_sim = (t_sim + a_sim) / 2.0;
                                                    if avg_sim >= 0.85 {
                                                        if best_tidal.as_ref().map_or(true, |(_, s)| avg_sim > *s) {
                                                            best_tidal = Some((id, avg_sim));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let q_id = best_qobuz.as_ref().map(|(id, _)| id.clone());
        let t_id = best_tidal.as_ref().map(|(id, _)| id.clone());
        let conf = best_qobuz
            .as_ref()
            .map(|(_, s)| *s)
            .or_else(|| best_tidal.as_ref().map(|(_, s)| *s))
            .unwrap_or(0.0);

        if q_id.is_some() || t_id.is_some() {
            Ok(Some(ResolvedTrack {
                source_track_id: source_track_id.to_string(),
                isrc: None,
                qobuz_id: q_id,
                tidal_id: t_id,
                resolution_method: "fuzzy_duration".to_string(),
                match_confidence: conf,
            }))
        } else {
            Ok(None)
        }
    }

    /// Saves a resolved track into the SQLite resolution_cache
    pub fn save_resolution(&self, res: &ResolvedTrack) -> Result<()> {
        let entry = ResolutionEntry {
            source_track_id: res.source_track_id.clone(),
            isrc: res.isrc.clone(),
            qobuz_id: res.qobuz_id.clone(),
            tidal_id: res.tidal_id.clone(),
            resolution_method: res.resolution_method.clone(),
            match_confidence: res.match_confidence,
            local_archive_path: None,
            resolved_at: Utc::now().timestamp(),
        };
        self.db.upsert_resolution(&entry)
    }
}

/// Background batch queue worker for pre-resolving playlist tracks at 2 requests/sec with preemption
pub struct BatchResolutionWorker {
    resolver: Resolver,
    queue_tx: mpsc::Sender<(PlaylistTrack, Priority)>,
}

impl BatchResolutionWorker {
    pub fn new(resolver: Resolver) -> (Self, mpsc::Receiver<(PlaylistTrack, Priority)>) {
        let (tx, rx) = mpsc::channel(1000);
        (
            Self {
                resolver,
                queue_tx: tx,
            },
            rx,
        )
    }

    pub fn resolver(&self) -> &Resolver {
        &self.resolver
    }

    /// Enqueues a batch of playlist tracks for background resolution
    pub async fn enqueue_batch(&self, tracks: Vec<PlaylistTrack>) -> Result<()> {
        for track in tracks {
            self.queue_tx.send((track, Priority::Low)).await.ok();
        }
        Ok(())
    }

    /// Enqueues a single track with High priority (user playback preemption)
    pub async fn enqueue_immediate(&self, track: PlaylistTrack) -> Result<()> {
        self.queue_tx.send((track, Priority::High)).await.ok();
        Ok(())
    }

    /// Runs the background resolution processing loop
    pub async fn run_loop(resolver: Resolver, mut rx: mpsc::Receiver<(PlaylistTrack, Priority)>) {
        while let Some((track, priority)) = rx.recv().await {
            // High priority runs immediately; Low priority rate-limits to 2 req/sec (500ms delay)
            if priority == Priority::Low {
                sleep(Duration::from_millis(500)).await;
            }

            match resolver.resolve_track(&track).await {
                Ok(resolved) => {
                    log::debug!(
                        "Resolved track {} via {} (conf: {:.2})",
                        resolved.source_track_id,
                        resolved.resolution_method,
                        resolved.match_confidence
                    );
                }
                Err(e) => {
                    log::trace!("Track resolution skipped or failed for {}: {}", track.source_track_id, e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jaro_winkler_similarity() {
        // Exact match
        assert_eq!(jaro_winkler_similarity("Daft Punk", "Daft Punk"), 1.0);
        assert_eq!(jaro_winkler_similarity("get lucky", "GET LUCKY"), 1.0);

        // Close match (should be >= 0.85)
        let sim = jaro_winkler_similarity("Get Lucky", "Get Lucky (Radio Edit)");
        assert!(sim >= 0.85, "sim was {}", sim);

        // Completely different strings
        let low_sim = jaro_winkler_similarity("Daft Punk", "Beethoven");
        assert!(low_sim < 0.6, "low_sim was {}", low_sim);

        // Empty string cases
        assert_eq!(jaro_winkler_similarity("", ""), 1.0);
        assert_eq!(jaro_winkler_similarity("abc", ""), 0.0);
    }

    #[test]
    fn test_duration_matching() {
        // Spotify tolerance: <= 2000 ms
        assert!(is_duration_match(240_000, 241_500, false));
        assert!(is_duration_match(240_000, 238_500, false));
        assert!(!is_duration_match(240_000, 243_000, false));

        // YouTube tolerance: <= 4000 ms
        assert!(is_duration_match(240_000, 243_500, true));
        assert!(is_duration_match(240_000, 236_500, true));
        assert!(!is_duration_match(240_000, 245_000, true));
    }

    #[test]
    fn test_resolution_cache_lifecycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("library.db");
        let db = Database::open_path(&db_path).unwrap();

        let resolver = Resolver::new(db.clone());

        let track = PlaylistTrack {
            playlist_id: "test:pl".to_string(),
            position: 0,
            source_track_id: "yt:video:demo12345".to_string(),
            raw_title: "Demo Title".to_string(),
            clean_title: "Demo Title".to_string(),
            artist: "Demo Artist".to_string(),
            album: None,
            duration_ms: 180_000,
            isrc: None,
            cover_art_url: None,
        };

        // Initially no cache entry
        assert!(db.get_resolution(&track.source_track_id).unwrap().is_none());

        // Resolve youtube track (will fallback to yt_direct_fallback since no providers or Songlink mock)
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(resolver.resolve_track(&track)).unwrap();

        assert_eq!(res.source_track_id, "yt:video:demo12345");
        assert_eq!(res.resolution_method, "yt_direct_fallback");
        assert_eq!(res.match_confidence, 1.0);

        // Cached entry now exists in SQLite
        let cached = db.get_resolution(&track.source_track_id).unwrap().unwrap();
        assert_eq!(cached.resolution_method, "yt_direct_fallback");
    }
}
