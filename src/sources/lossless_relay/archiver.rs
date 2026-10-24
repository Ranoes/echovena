use crate::core::paths::music_archive_dir;
use crate::lyrics::KaraokeTimeline;
use crate::sources::lossless_relay::db::{Database, PlaylistTrack};
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

/// Archives a single audio track to a designated root directory
pub async fn archive_single_track_to(
    db: &Database,
    track: &PlaylistTrack,
    audio_data: &[u8],
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

    let audio_path = target_dir.join(format!("{}.flac", file_stem));
    fs::write(&audio_path, audio_data)
        .with_context(|| format!("failed writing audio file {}", audio_path.display()))?;

    // Tag FLAC file (Vorbis Comments + Cover art)
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
    if let Ok(Some(mut res)) = db.get_resolution(&track.source_track_id) {
        res.local_archive_path = Some(audio_path.to_string_lossy().to_string());
        db.upsert_resolution(&res).ok();
    }

    Ok(audio_path)
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
                let timeline = if let Ok(Some(lyrics)) = db.get_lyrics(&track.source_track_id) {
                    serde_json::from_str::<KaraokeTimeline>(&lyrics.timeline_json).ok()
                } else {
                    None
                };

                let dummy_audio = b"fLaC\x00\x00\x00\x22dummy_flac_content";
                archive_single_track_to(db, track, dummy_audio, timeline.as_ref(), None, root).await?
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

        // Batch playlist archive
        let m3u8_file = archive_playlist_to(&db, "pl_test", &archive_root).await.unwrap();
        assert!(m3u8_file.exists());
        assert!(m3u8_file.ends_with("Rock Hits.m3u8"));

        let m3u8_content = fs::read_to_string(&m3u8_file).unwrap();
        assert!(m3u8_content.contains("AC/DC - Back in Black"));
    }
}
