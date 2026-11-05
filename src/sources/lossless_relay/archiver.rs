use crate::core::paths::music_archive_dir;
use crate::core::plugin_api::TrackInfo;
use crate::lyrics::KaraokeTimeline;
use crate::sources::lossless_relay::db::{Database, PlaylistTrack, ResolutionEntry};
use anyhow::{anyhow, Context, Result};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag, TagType};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Sanitizes strings for safe cross-platform file and directory names
pub fn sanitize_filename(name: &str) -> String {
    let invalid = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let s: String = name
        .chars()
        .map(|c| if invalid.contains(&c) { '_' } else { c })
        .collect();
    let trimmed = s.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed
    }
}

/// Upgrades standard Google User Content / YouTube Music / YouTube thumbnail URLs to 1200x1200+ HD
pub fn upgrade_cover_art_url_to_hd(url: &str) -> String {
    // 1. Google / YouTube Music UserContent URLs (e.g., =w120-h120-l90-rj or =s120)
    if url.contains("googleusercontent.com") || url.contains("ggpht.com") {
        if let Ok(re_wh) = regex::Regex::new(r"=w\d+-h\d+.*") {
            if re_wh.is_match(url) {
                return re_wh.replace(url, "=s1200").to_string();
            }
        }
        if let Ok(re_s) = regex::Regex::new(r"=s\d+.*") {
            if re_s.is_match(url) {
                return re_s.replace(url, "=s1200").to_string();
            }
        }
        if !url.contains('=') {
            return format!("{}=s1200", url);
        }
    }

    // 2. YouTube static video thumbnails (i.ytimg.com)
    if url.contains("i.ytimg.com") {
        if url.contains("hqdefault.jpg")
            || url.contains("mqdefault.jpg")
            || url.contains("sddefault.jpg")
            || url.contains("default.jpg")
        {
            if let Ok(re_yt) = regex::Regex::new(r"/(hqdefault|mqdefault|sddefault|default)\.jpg") {
                return re_yt.replace(url, "/maxresdefault.jpg").to_string();
            }
        }
    }

    url.to_string()
}

/// Tags an audio file with Vorbis comments and embedded cover art using lofty
pub fn tag_flac_file(
    file_path: &Path,
    title: &str,
    artist: &str,
    album: Option<&str>,
    track_number: Option<u32>,
    isrc: Option<&str>,
    cover_jpeg: Option<&[u8]>,
) -> Result<()> {
    let mut tagged_file = Probe::open(file_path)
        .context("failed to probe audio file for tagging")?
        .read()
        .context("failed to read audio file tags")?;

    let mut tag = tagged_file
        .primary_tag_mut()
        .map(|t| t.clone())
        .unwrap_or_else(|| Tag::new(TagType::VorbisComments));

    tag.insert_text(ItemKey::TrackTitle, title.to_string());
    tag.insert_text(ItemKey::TrackArtist, artist.to_string());
    if let Some(alb) = album {
        tag.insert_text(ItemKey::AlbumTitle, alb.to_string());
    }
    if let Some(num) = track_number {
        tag.insert_text(ItemKey::TrackNumber, num.to_string());
    }
    if let Some(code) = isrc {
        tag.insert_text(ItemKey::Isrc, code.to_string());
    }

    if let Some(pic_data) = cover_jpeg {
        let pic = Picture::unchecked(pic_data.to_vec())
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Jpeg)
            .build();
        tag.push_picture(pic);
    }

    tag.save_to_path(file_path, WriteOptions::default())
        .context("failed to save tags to audio file")?;
    Ok(())
}

/// Convert a [`TrackInfo`] into a [`PlaylistTrack`] for archiving or syncing
pub fn track_info_to_playlist_track(t: &TrackInfo, position: usize, playlist_id: &str) -> PlaylistTrack {
    let artist = if t.artists.is_empty() {
        "Unknown Artist".to_string()
    } else {
        t.artists.join(", ")
    };
    let source_id = t.uri.clone().unwrap_or_else(|| format!("custom:{}", t.name));
    PlaylistTrack {
        playlist_id: playlist_id.to_string(),
        position: position as i64,
        source_track_id: source_id,
        raw_title: t.name.clone(),
        clean_title: t.name.clone(),
        artist,
        album: if t.album.is_empty() {
            None
        } else {
            Some(t.album.clone())
        },
        duration_ms: t.duration_ms as i64,
        isrc: None,
        cover_art_url: t.image_url.clone(),
    }
}

/// Convert a [`PlaylistTrack`] into a [`TrackInfo`] for player and queue consumption
pub fn playlist_track_to_track_info(p: &PlaylistTrack) -> TrackInfo {
    TrackInfo {
        uri: Some(p.source_track_id.clone()),
        name: p.clean_title.clone(),
        artists: vec![p.artist.clone()],
        album: p.album.clone().unwrap_or_default(),
        duration_ms: p.duration_ms.max(0) as u64,
        id: Some(p.source_track_id.clone()),
        album_id: None,
        artist_refs: Vec::new(),
        is_playable: true,
        is_local: false,
        track_number: (p.position + 1) as u32,
        explicit: false,
        image_url: p.cover_art_url.clone(),
        release_date: None,
        generated_name: None,
    }
}

/// Archives a single audio track with a custom extension ("flac" or "opus")
pub async fn archive_single_track_with_ext_to(
    db: &Database,
    track: &PlaylistTrack,
    audio_data: &[u8],
    audio_ext: &str,
    timeline: Option<&KaraokeTimeline>,
    cover_jpeg: Option<&[u8]>,
    root: &Path,
) -> Result<PathBuf> {
    let artist_dir = sanitize_filename(&track.artist);
    let album_dir = sanitize_filename(track.album.as_deref().unwrap_or("Singles"));

    let target_dir = root.join(artist_dir).join(album_dir);
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("failed creating directory {}", target_dir.display()))?;

    let file_stem = format!(
        "{:02} - {}",
        track.position + 1,
        sanitize_filename(&track.clean_title)
    );

    let ext = if audio_ext.is_empty() { "flac" } else { audio_ext };
    let audio_path = target_dir.join(format!("{}.{}", file_stem, ext));
    fs::write(&audio_path, audio_data)
        .with_context(|| format!("failed writing audio file {}", audio_path.display()))?;

    // Tag audio file (Vorbis Comments + Cover art)
    let track_num = (track.position + 1) as u32;
    tag_flac_file(
        &audio_path,
        &track.clean_title,
        &track.artist,
        track.album.as_deref(),
        Some(track_num),
        track.isrc.as_deref(),
        cover_jpeg,
    )
    .ok();

    // Sidecar Enhanced LRC (.elrc)
    if let Some(tl) = timeline {
        let elrc_text = tl.to_enhanced_lrc();
        let elrc_path = target_dir.join(format!("{}.elrc", file_stem));
        fs::write(&elrc_path, elrc_text).ok();
    }

    // Update SQLite resolution_cache with local_archive_path
    let entry = match db.get_resolution(&track.source_track_id) {
        Ok(Some(mut res)) => {
            res.local_archive_path = Some(audio_path.to_string_lossy().to_string());
            res
        }
        _ => ResolutionEntry {
            source_track_id: track.source_track_id.clone(),
            isrc: track.isrc.clone(),
            qobuz_id: None,
            tidal_id: None,
            resolution_method: "local_archive".into(),
            match_confidence: 1.0,
            local_archive_path: Some(audio_path.to_string_lossy().to_string()),
            resolved_at: chrono::Utc::now().timestamp(),
        },
    };
    db.upsert_resolution(&entry).ok();

    // Register into downloaded_tracks for library display
    let dl_track = crate::sources::lossless_relay::db::DownloadedTrack {
        source_track_id: track.source_track_id.clone(),
        title: track.clean_title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        duration_ms: track.duration_ms.max(0) as u64,
        file_path: audio_path.to_string_lossy().to_string(),
        cover_art_url: track.cover_art_url.clone(),
        downloaded_at: chrono::Utc::now().timestamp(),
    };
    db.add_downloaded_track(&dl_track).ok();

    Ok(audio_path)
}

/// Scans the archive directory for existing audio tracks and registers them in the database.
pub fn scan_and_sync_local_downloads(db: &Database, root: &Path) {
    if !root.exists() {
        return;
    }
    let existing_downloads = db.list_downloaded_tracks().unwrap_or_default();
    let mut existing_paths = std::collections::HashSet::new();
    let mut existing_keys = std::collections::HashSet::new();
    for d in &existing_downloads {
        existing_paths.insert(d.file_path.clone());
        if let Ok(canon) = std::path::Path::new(&d.file_path).canonicalize() {
            existing_paths.insert(canon.to_string_lossy().to_string());
        }
        if d.duration_ms > 0 {
            existing_keys.insert((d.artist.to_lowercase(), d.title.to_lowercase()));
        }
    }

    let mut dirs_to_visit = vec![root.to_path_buf()];
    while let Some(current_dir) = dirs_to_visit.pop() {
        if let Ok(entries) = fs::read_dir(&current_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs_to_visit.push(path);
                } else if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if matches!(ext.to_lowercase().as_str(), "flac" | "opus" | "m4a" | "mp3" | "webm") {
                            let path_str = path.to_string_lossy().to_string();
                            let canon_path = path.canonicalize().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| path_str.clone());
                            if existing_paths.contains(&path_str) || existing_paths.contains(&canon_path) {
                                continue;
                            }

                            let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                            let clean_title = if let Some((_, rest)) = file_stem.split_once(" - ") {
                                rest.to_string()
                            } else {
                                file_stem.to_string()
                            };
                            let parent = path.parent();
                            let album = parent.and_then(|p| p.file_name()).and_then(|s| s.to_str()).map(|s| s.to_string());
                            let artist = parent
                                .and_then(|p| p.parent())
                                .and_then(|p| p.file_name())
                                .and_then(|s| s.to_str())
                                .unwrap_or("Unknown Artist")
                                .to_string();

                            if existing_keys.contains(&(artist.to_lowercase(), clean_title.to_lowercase())) {
                                continue;
                            }

                            let track_id = format!("local:track:{}", sanitize_filename(&format!("{}-{}", artist, clean_title)));
                            let dl_track = crate::sources::lossless_relay::db::DownloadedTrack {
                                source_track_id: track_id.clone(),
                                title: clean_title,
                                artist,
                                album,
                                duration_ms: 0,
                                file_path: path_str,
                                cover_art_url: None,
                                downloaded_at: entry.metadata().ok().and_then(|m| m.modified().ok())
                                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                    .map(|d| d.as_secs() as i64)
                                    .unwrap_or_else(|| chrono::Utc::now().timestamp()),
                            };
                            let _ = db.add_downloaded_track(&dl_track);
                        }
                    }
                }
            }
        }
    }
}

/// Archives a single audio track to a designated root directory (defaults to FLAC)
pub async fn archive_single_track_to(
    db: &Database,
    track: &PlaylistTrack,
    audio_data: &[u8],
    timeline: Option<&KaraokeTimeline>,
    cover_jpeg: Option<&[u8]>,
    root: &Path,
) -> Result<PathBuf> {
    archive_single_track_with_ext_to(db, track, audio_data, "flac", timeline, cover_jpeg, root).await
}

/// Resolves audio via waterfall (Tidal/Qobuz FLAC if token present, else YouTube Opus),
/// fetches cover art and lyrics, and writes the archived files to `root`.
pub async fn resolve_and_archive_track(
    db: &Database,
    track: &PlaylistTrack,
    root: &Path,
) -> Result<PathBuf> {
    resolve_and_archive_track_with_source(db, track, root, crate::core::source::DownloadSource::default()).await
}

/// Resolves audio honoring `download_source`, fetches cover art and lyrics, and writes the archived files to `root`.
pub async fn resolve_and_archive_track_with_source(
    db: &Database,
    track: &PlaylistTrack,
    root: &Path,
    download_source: crate::core::source::DownloadSource,
) -> Result<PathBuf> {
    // 1. Try Token-gated LosslessWaterfall (Tidal/Qobuz FLAC) unless user specifically chose YouTubeOpus
    let stream_res = if download_source == crate::core::source::DownloadSource::YouTubeOpus {
        Err(anyhow!("YouTube Opus selected by user configuration"))
    } else {
        let registry = crate::config::providers::ProviderRegistry::load_or_create_default()
            .unwrap_or_else(|_| crate::config::providers::ProviderRegistry::default_registry());
        let waterfall = crate::sources::lossless_relay::waterfall::LosslessWaterfall::new(registry);
        let resolved = crate::sources::lossless_relay::resolver::ResolvedTrack {
            source_track_id: track.source_track_id.clone(),
            isrc: track.isrc.clone(),
            qobuz_id: None,
            tidal_id: None,
            resolution_method: "auto".to_string(),
            match_confidence: 1.0,
        };
        waterfall.resolve_stream(track, &resolved).await
    };

    let (audio_bytes, ext) = match stream_res {
        Ok(info) if info.format.starts_with("FLAC") && !info.stream_url.is_empty() => {
            log::info!("Downloading FLAC stream from {}", info.provider_id);
            let bytes = reqwest::get(&info.stream_url).await?.bytes().await?.to_vec();
            (bytes, "flac".to_string())
        }
        _ => {
            // Fallback to local audio cache or yt-dlp Opus
            let video_id = if let Some(vid) = track.source_track_id.strip_prefix("yt:video:") {
                Some(vid)
            } else if let Some(vid) = track.source_track_id.strip_prefix("youtube:") {
                Some(vid)
            } else {
                None
            };

            #[cfg(feature = "youtube")]
            let cached = video_id.and_then(crate::infra::youtube::cached_audio_path);
            #[cfg(not(feature = "youtube"))]
            let cached: Option<PathBuf> = None;

            if let Some(cached_path) = cached {
                let bytes = fs::read(&cached_path)?;
                let ext = cached_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("opus")
                    .to_string();
                (bytes, ext)
            } else {
                // Download with yt-dlp
                let query = if let Some(vid) = video_id {
                    format!("https://www.youtube.com/watch?v={vid}")
                } else {
                    format!("ytsearch1:{} {}", track.artist, track.clean_title)
                };

                let tmp_dir = std::env::temp_dir().join(format!("echovena_dl_{}_{}", std::process::id(), track.position));
                let _ = fs::create_dir_all(&tmp_dir);
                let tmp_file = tmp_dir.join("download.opus");
                let tmp_str = tmp_file.to_str().unwrap_or("download.opus");

                let mut cmd = tokio::process::Command::new("yt-dlp");
                #[cfg(all(windows, feature = "youtube"))]
                cmd.creation_flags(crate::infra::youtube::CREATE_NO_WINDOW);
                #[cfg(all(windows, not(feature = "youtube")))]
                cmd.creation_flags(0x0800_0000);
                cmd.args(&[
                    "-f",
                    "251/140/bestaudio",
                    "-x",
                    "--audio-format",
                    "opus",
                    "--no-playlist",
                    "--no-warnings",
                    "-o",
                    tmp_str,
                    &query,
                ]);

                let _ = cmd.output().await?;
                let downloaded = if tmp_file.exists() {
                    let bytes = fs::read(&tmp_file)?;
                    Some((bytes, "opus".to_string()))
                } else {
                    let mut found_bytes = None;
                    if let Ok(entries) = fs::read_dir(&tmp_dir) {
                        for entry in entries.flatten() {
                            if let Ok(b) = fs::read(entry.path()) {
                                if b.len() > 1024 {
                                    found_bytes = Some(b);
                                    break;
                                }
                            }
                        }
                    }
                    found_bytes.map(|bytes| (bytes, "opus".to_string()))
                };
                let _ = fs::remove_dir_all(&tmp_dir);
                downloaded.ok_or_else(|| {
                    anyhow!("yt-dlp failed to download audio for {}", track.clean_title)
                })?
            }
        }
    };

    // Cover art fetch (automatically upgraded to 1200x1200+ HD)
    let mut cover_bytes = None;
    if let Some(ref url) = track.cover_art_url {
        let hd_url = upgrade_cover_art_url_to_hd(url);
        if let Ok(resp) = reqwest::get(&hd_url).await {
            if let Ok(b) = resp.bytes().await {
                cover_bytes = Some(b.to_vec());
            }
        } else if let Ok(resp) = reqwest::get(url).await {
            if let Ok(b) = resp.bytes().await {
                cover_bytes = Some(b.to_vec());
            }
        }
    }

    // Lyrics timeline
    let timeline = if let Ok(Some(lyrics)) = db.get_lyrics(&track.source_track_id) {
        serde_json::from_str::<KaraokeTimeline>(&lyrics.timeline_json).ok()
    } else {
        None
    };

    archive_single_track_with_ext_to(
        db,
        track,
        &audio_bytes,
        &ext,
        timeline.as_ref(),
        cover_bytes.as_deref(),
        root,
    )
    .await
}

/// Archives a single audio track to ~/Music/EchoVena
pub async fn archive_single_track(
    db: &Database,
    track: &PlaylistTrack,
    audio_data: &[u8],
    timeline: Option<&KaraokeTimeline>,
    cover_jpeg: Option<&[u8]>,
) -> Result<PathBuf> {
    let root = music_archive_dir().context("unable to determine Music/EchoVena directory")?;
    archive_single_track_to(db, track, audio_data, timeline, cover_jpeg, &root).await
}

/// Generates a standard .m3u8 playlist file referencing local FLAC tracks
pub fn generate_m3u8_playlist_to(
    playlist_title: &str,
    tracks: &[PlaylistTrack],
    track_paths: &[PathBuf],
    root: &Path,
) -> Result<PathBuf> {
    fs::create_dir_all(root)?;

    let playlist_filename = format!("{}.m3u8", sanitize_filename(playlist_title));
    let playlist_path = root.join(playlist_filename);

    let mut file = fs::File::create(&playlist_path)?;
    writeln!(file, "#EXTM3U")?;

    for (track, path) in tracks.iter().zip(track_paths.iter()) {
        let duration_sec = track.duration_ms / 1000;
        writeln!(
            file,
            "#EXTINF:{},{} - {}",
            duration_sec, track.artist, track.clean_title
        )?;
        writeln!(file, "{}", path.to_string_lossy())?;
    }

    Ok(playlist_path)
}

/// Generates a standard .m3u8 playlist file in ~/Music/EchoVena
pub fn generate_m3u8_playlist(
    playlist_title: &str,
    tracks: &[PlaylistTrack],
    track_paths: &[PathBuf],
) -> Result<PathBuf> {
    let root = music_archive_dir().context("unable to determine Music/EchoVena directory")?;
    generate_m3u8_playlist_to(playlist_title, tracks, track_paths, &root)
}

/// Batch archives all tracks for an imported playlist to a designated root directory
pub async fn archive_playlist_to(
    db: &Database,
    playlist_id: &str,
    root: &Path,
) -> Result<PathBuf> {
    let playlist = db
        .get_imported_playlist(playlist_id)?
        .ok_or_else(|| anyhow!("imported playlist '{}' not found", playlist_id))?;

    let tracks = db.get_playlist_tracks(playlist_id)?;
    if tracks.is_empty() {
        return Err(anyhow!("playlist '{}' has no tracks to archive", playlist.title));
    }

    let mut archived_paths = Vec::new();

    for track in &tracks {
        let mut existing_path = None;
        if let Ok(Some(res)) = db.get_resolution(&track.source_track_id) {
            if let Some(path_str) = res.local_archive_path {
                let p = PathBuf::from(&path_str);
                if p.exists() {
                    existing_path = Some(p);
                }
            }
        }

        let final_path = match existing_path {
            Some(p) => p,
            None => {
                resolve_and_archive_track(db, track, root).await?
            }
        };

        archived_paths.push(final_path);
    }

    let m3u8_path = generate_m3u8_playlist_to(&playlist.title, &tracks, &archived_paths, root)?;
    Ok(m3u8_path)
}

/// Batch archives all tracks for an imported playlist in ~/Music/EchoVena
pub async fn archive_playlist(
    db: &Database,
    playlist_id: &str,
) -> Result<PathBuf> {
    let root = music_archive_dir().context("unable to determine Music/EchoVena directory")?;
    archive_playlist_to(db, playlist_id, &root).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::lossless_relay::db::ImportedPlaylist;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("AC/DC"), "AC_DC");
        assert_eq!(sanitize_filename("What? (Audio)"), "What_ (Audio)");
        assert_eq!(sanitize_filename("Track: Deluxe"), "Track_ Deluxe");
        assert_eq!(sanitize_filename("Normal Title"), "Normal Title");
    }

    #[test]
    fn test_generate_m3u8() {
        let temp_dir = tempfile::tempdir().unwrap();

        let tracks = vec![
            PlaylistTrack {
                playlist_id: "pl:1".to_string(),
                position: 0,
                source_track_id: "sp:1".to_string(),
                raw_title: "Track 1".to_string(),
                clean_title: "Track 1".to_string(),
                artist: "Artist 1".to_string(),
                album: None,
                duration_ms: 180_000,
                isrc: None,
                cover_art_url: None,
            },
            PlaylistTrack {
                playlist_id: "pl:1".to_string(),
                position: 1,
                source_track_id: "sp:2".to_string(),
                raw_title: "Track 2".to_string(),
                clean_title: "Track 2".to_string(),
                artist: "Artist 2".to_string(),
                album: None,
                duration_ms: 240_000,
                isrc: None,
                cover_art_url: None,
            },
        ];

        let paths = vec![
            temp_dir.path().join("01 - Track 1.flac"),
            temp_dir.path().join("02 - Track 2.flac"),
        ];

        let m3u8_path = generate_m3u8_playlist_to("My Mix", &tracks, &paths, temp_dir.path()).unwrap();
        assert!(m3u8_path.exists());

        let content = fs::read_to_string(&m3u8_path).unwrap();
        assert!(content.starts_with("#EXTM3U"));
        assert!(content.contains("#EXTINF:180,Artist 1 - Track 1"));
        assert!(content.contains("#EXTINF:240,Artist 2 - Track 2"));
    }

    #[tokio::test]
    async fn test_archive_single_and_playlist_flow() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::open_path(&db_path).unwrap();

        let pl = ImportedPlaylist {
            id: "pl_test".to_string(),
            platform: "spotify".to_string(),
            title: "Rock Hits".to_string(),
            author: Some("EchoVena".to_string()),
            source_url: "https://open.spotify.com/playlist/test".to_string(),
            cover_art_url: None,
            snapshot_id: None,
            track_count: 1,
            last_synced_at: 1000,
        };
        db.upsert_imported_playlist(&pl).unwrap();

        let track = PlaylistTrack {
            playlist_id: "pl_test".to_string(),
            position: 0,
            source_track_id: "track_001".to_string(),
            raw_title: "Back in Black".to_string(),
            clean_title: "Back in Black".to_string(),
            artist: "AC/DC".to_string(),
            album: Some("Back in Black".to_string()),
            duration_ms: 255_000,
            isrc: Some("AUAP07100030".to_string()),
            cover_art_url: None,
        };
        db.replace_playlist_tracks("pl_test", &[track.clone()]).unwrap();

        let archive_root = temp_dir.path().join("EchoVenaMusic");
        let flac_path = archive_single_track_to(
            &db,
            &track,
            b"test_audio_data",
            None,
            None,
            &archive_root,
        )
        .await
        .unwrap();

        assert!(flac_path.exists());
        assert!(flac_path.ends_with("01 - Back in Black.flac"));

        let opus_path = archive_single_track_with_ext_to(
            &db,
            &track,
            b"test_opus_data",
            "opus",
            None,
            None,
            &archive_root,
        )
        .await
        .unwrap();
        assert!(opus_path.exists());
        assert!(opus_path.ends_with("01 - Back in Black.opus"));

        // Batch playlist archive
        let m3u8_file = archive_playlist_to(&db, "pl_test", &archive_root).await.unwrap();
        assert!(m3u8_file.exists());
        assert!(m3u8_file.ends_with("Rock Hits.m3u8"));

        let m3u8_content = fs::read_to_string(&m3u8_file).unwrap();
        assert!(m3u8_content.contains("AC/DC - Back in Black"));
    }

    #[test]
    fn test_track_info_to_playlist_track() {
        let t = TrackInfo {
            generated_name: None,
            uri: Some("youtube:abc123xyz".to_string()),
            name: "Harder, Better, Faster, Stronger".to_string(),
            artists: vec!["Daft Punk".to_string()],
            album: "Discovery".to_string(),
            duration_ms: 224_000,
            id: None,
            album_id: None,
            artist_refs: vec![],
            is_playable: true,
            is_local: false,
            track_number: 4,
            explicit: false,
            image_url: Some("https://example.com/art.jpg".to_string()),
            release_date: None,
        };
        let pl_track = track_info_to_playlist_track(&t, 3, "my_pl");
        assert_eq!(pl_track.clean_title, "Harder, Better, Faster, Stronger");
        assert_eq!(pl_track.artist, "Daft Punk");
        assert_eq!(pl_track.album.as_deref(), Some("Discovery"));
        assert_eq!(pl_track.position, 3);
        assert_eq!(pl_track.source_track_id, "youtube:abc123xyz");
    }

    #[test]
    fn test_upgrade_cover_art_url_to_hd() {
        let google_url = "https://yt3.googleusercontent.com/abc=w120-h120-l90-rj";
        assert_eq!(
            upgrade_cover_art_url_to_hd(google_url),
            "https://yt3.googleusercontent.com/abc=s1200"
        );

        let yt_thumb = "https://i.ytimg.com/vi/abc123xyz/hqdefault.jpg";
        assert_eq!(
            upgrade_cover_art_url_to_hd(yt_thumb),
            "https://i.ytimg.com/vi/abc123xyz/maxresdefault.jpg"
        );
    }
}
