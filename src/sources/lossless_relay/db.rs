use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedPlaylist {
    pub id: String,
    pub platform: String,
    pub title: String,
    pub author: Option<String>,
    pub source_url: String,
    pub cover_art_url: Option<String>,
    pub snapshot_id: Option<String>,
    pub track_count: i64,
    pub last_synced_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistTrack {
    pub playlist_id: String,
    pub position: i64,
    pub source_track_id: String,
    pub raw_title: String,
    pub clean_title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: i64,
    pub isrc: Option<String>,
    pub cover_art_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolutionEntry {
    pub source_track_id: String,
    pub isrc: Option<String>,
    pub qobuz_id: Option<String>,
    pub tidal_id: Option<String>,
    pub resolution_method: String,
    pub match_confidence: f64,
    pub local_archive_path: Option<String>,
    pub resolved_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricsEntry {
    pub source_track_id: String,
    pub isrc: Option<String>,
    pub provider: String,
    pub sync_type: String,
    pub timeline_json: String,
    pub raw_elrc: Option<String>,
    pub user_offset_ms: i32,
    pub fetched_at: i64,
}

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open_default() -> Result<Self> {
        let path = crate::core::paths::library_db_path()
            .context("failed to resolve echovena library.db path")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed creating database dir {}", parent.display()))?;
        }
        Self::open_path(&path)
    }

    pub fn open_path(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed opening sqlite db at {}", path.display()))?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("failed opening in-memory sqlite db")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS imported_playlists (
                id TEXT PRIMARY KEY,
                platform TEXT NOT NULL,
                title TEXT NOT NULL,
                author TEXT,
                source_url TEXT NOT NULL,
                cover_art_url TEXT,
                snapshot_id TEXT,
                track_count INTEGER DEFAULT 0,
                last_synced_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS playlist_tracks (
                playlist_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                source_track_id TEXT NOT NULL,
                raw_title TEXT NOT NULL,
                clean_title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT,
                duration_ms INTEGER NOT NULL,
                isrc TEXT,
                cover_art_url TEXT,
                PRIMARY KEY (playlist_id, position),
                FOREIGN KEY (playlist_id) REFERENCES imported_playlists(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS resolution_cache (
                source_track_id TEXT PRIMARY KEY,
                isrc TEXT,
                qobuz_id TEXT,
                tidal_id TEXT,
                resolution_method TEXT NOT NULL,
                match_confidence REAL NOT NULL,
                local_archive_path TEXT,
                resolved_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS lyrics_cache (
                source_track_id TEXT PRIMARY KEY,
                isrc TEXT,
                provider TEXT NOT NULL,
                sync_type TEXT NOT NULL,
                timeline_json TEXT NOT NULL,
                raw_elrc TEXT,
                user_offset_ms INTEGER DEFAULT 0,
                fetched_at INTEGER NOT NULL
            );
            "#,
        )
        .context("failed executing database schema migration")?;
        Ok(())
    }

    pub fn upsert_imported_playlist(&self, pl: &ImportedPlaylist) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO imported_playlists (
                id, platform, title, author, source_url, cover_art_url, snapshot_id, track_count, last_synced_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                platform = excluded.platform,
                title = excluded.title,
                author = excluded.author,
                source_url = excluded.source_url,
                cover_art_url = excluded.cover_art_url,
                snapshot_id = excluded.snapshot_id,
                track_count = excluded.track_count,
                last_synced_at = excluded.last_synced_at;
            "#,
            params![
                pl.id,
                pl.platform,
                pl.title,
                pl.author,
                pl.source_url,
                pl.cover_art_url,
                pl.snapshot_id,
                pl.track_count,
                pl.last_synced_at,
            ],
        )
        .context("failed to upsert imported playlist")?;
        Ok(())
    }

    pub fn get_imported_playlist(&self, id: &str) -> Result<Option<ImportedPlaylist>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, platform, title, author, source_url, cover_art_url, snapshot_id, track_count, last_synced_at
            FROM imported_playlists WHERE id = ?1;
            "#,
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ImportedPlaylist {
                id: row.get(0)?,
                platform: row.get(1)?,
                title: row.get(2)?,
                author: row.get(3)?,
                source_url: row.get(4)?,
                cover_art_url: row.get(5)?,
                snapshot_id: row.get(6)?,
                track_count: row.get(7)?,
                last_synced_at: row.get(8)?,
            })
        })?;

        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    pub fn list_imported_playlists(&self) -> Result<Vec<ImportedPlaylist>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, platform, title, author, source_url, cover_art_url, snapshot_id, track_count, last_synced_at
            FROM imported_playlists ORDER BY last_synced_at DESC;
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ImportedPlaylist {
                id: row.get(0)?,
                platform: row.get(1)?,
                title: row.get(2)?,
                author: row.get(3)?,
                source_url: row.get(4)?,
                cover_art_url: row.get(5)?,
                snapshot_id: row.get(6)?,
                track_count: row.get(7)?,
                last_synced_at: row.get(8)?,
            })
        })?;

        let mut playlists = Vec::new();
        for r in rows {
            playlists.push(r?);
        }
        Ok(playlists)
    }

    pub fn delete_imported_playlist(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM imported_playlists WHERE id = ?1;", params![id])
            .context("failed to delete imported playlist")?;
        Ok(())
    }

    pub fn replace_playlist_tracks(&self, playlist_id: &str, tracks: &[PlaylistTrack]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1;",
            params![playlist_id],
        )?;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO playlist_tracks (
                    playlist_id, position, source_track_id, raw_title, clean_title, artist, album, duration_ms, isrc, cover_art_url
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);
                "#,
            )?;

            for track in tracks {
                stmt.execute(params![
                    track.playlist_id,
                    track.position,
                    track.source_track_id,
                    track.raw_title,
                    track.clean_title,
                    track.artist,
                    track.album,
                    track.duration_ms,
                    track.isrc,
                    track.cover_art_url,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn get_playlist_tracks(&self, playlist_id: &str) -> Result<Vec<PlaylistTrack>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT playlist_id, position, source_track_id, raw_title, clean_title, artist, album, duration_ms, isrc, cover_art_url
            FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position ASC;
            "#,
        )?;
        let rows = stmt.query_map(params![playlist_id], |row| {
            Ok(PlaylistTrack {
                playlist_id: row.get(0)?,
                position: row.get(1)?,
                source_track_id: row.get(2)?,
                raw_title: row.get(3)?,
                clean_title: row.get(4)?,
                artist: row.get(5)?,
                album: row.get(6)?,
                duration_ms: row.get(7)?,
                isrc: row.get(8)?,
                cover_art_url: row.get(9)?,
            })
        })?;

        let mut tracks = Vec::new();
        for r in rows {
            tracks.push(r?);
        }
        Ok(tracks)
    }

    pub fn upsert_resolution(&self, entry: &ResolutionEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO resolution_cache (
                source_track_id, isrc, qobuz_id, tidal_id, resolution_method, match_confidence, local_archive_path, resolved_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(source_track_id) DO UPDATE SET
                isrc = excluded.isrc,
                qobuz_id = excluded.qobuz_id,
                tidal_id = excluded.tidal_id,
                resolution_method = excluded.resolution_method,
                match_confidence = excluded.match_confidence,
                local_archive_path = excluded.local_archive_path,
                resolved_at = excluded.resolved_at;
            "#,
            params![
                entry.source_track_id,
                entry.isrc,
                entry.qobuz_id,
                entry.tidal_id,
                entry.resolution_method,
                entry.match_confidence,
                entry.local_archive_path,
                entry.resolved_at,
            ],
        )
        .context("failed to upsert resolution entry")?;
        Ok(())
    }

    pub fn get_resolution(&self, source_track_id: &str) -> Result<Option<ResolutionEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT source_track_id, isrc, qobuz_id, tidal_id, resolution_method, match_confidence, local_archive_path, resolved_at
            FROM resolution_cache WHERE source_track_id = ?1;
            "#,
        )?;
        let mut rows = stmt.query_map(params![source_track_id], |row| {
            Ok(ResolutionEntry {
                source_track_id: row.get(0)?,
                isrc: row.get(1)?,
                qobuz_id: row.get(2)?,
                tidal_id: row.get(3)?,
                resolution_method: row.get(4)?,
                match_confidence: row.get(5)?,
                local_archive_path: row.get(6)?,
                resolved_at: row.get(7)?,
            })
        })?;

        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_lyrics(&self, entry: &LyricsEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO lyrics_cache (
                source_track_id, isrc, provider, sync_type, timeline_json, raw_elrc, user_offset_ms, fetched_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(source_track_id) DO UPDATE SET
                isrc = excluded.isrc,
                provider = excluded.provider,
                sync_type = excluded.sync_type,
                timeline_json = excluded.timeline_json,
                raw_elrc = excluded.raw_elrc,
                user_offset_ms = excluded.user_offset_ms,
                fetched_at = excluded.fetched_at;
            "#,
            params![
                entry.source_track_id,
                entry.isrc,
                entry.provider,
                entry.sync_type,
                entry.timeline_json,
                entry.raw_elrc,
                entry.user_offset_ms,
                entry.fetched_at,
            ],
        )
        .context("failed to upsert lyrics entry")?;
        Ok(())
    }

    pub fn get_lyrics(&self, source_track_id: &str) -> Result<Option<LyricsEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT source_track_id, isrc, provider, sync_type, timeline_json, raw_elrc, user_offset_ms, fetched_at
            FROM lyrics_cache WHERE source_track_id = ?1;
            "#,
        )?;
        let mut rows = stmt.query_map(params![source_track_id], |row| {
            Ok(LyricsEntry {
                source_track_id: row.get(0)?,
                isrc: row.get(1)?,
                provider: row.get(2)?,
                sync_type: row.get(3)?,
                timeline_json: row.get(4)?,
                raw_elrc: row.get(5)?,
                user_offset_ms: row.get(6)?,
                fetched_at: row.get(7)?,
            })
        })?;

        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
    }

    pub fn update_lyric_offset(&self, source_track_id: &str, user_offset_ms: i32) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE lyrics_cache SET user_offset_ms = ?1 WHERE source_track_id = ?2;",
            params![user_offset_ms, source_track_id],
        )
        .context("failed to update lyric offset")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_init_and_playlist_crud() {
        let db = Database::open_in_memory().unwrap();

        let pl = ImportedPlaylist {
            id: "spotify:playlist:123".to_string(),
            platform: "spotify".to_string(),
            title: "Test Playlist".to_string(),
            author: Some("Tester".to_string()),
            source_url: "https://open.spotify.com/playlist/123".to_string(),
            cover_art_url: Some("https://example.com/cover.jpg".to_string()),
            snapshot_id: Some("snap1".to_string()),
            track_count: 2,
            last_synced_at: 1000,
        };

        db.upsert_imported_playlist(&pl).unwrap();

        let fetched = db.get_imported_playlist("spotify:playlist:123").unwrap();
        assert_eq!(fetched, Some(pl.clone()));

        let tracks = vec![
            PlaylistTrack {
                playlist_id: "spotify:playlist:123".to_string(),
                position: 0,
                source_track_id: "spotify:track:t1".to_string(),
                raw_title: "Song 1 (Official)".to_string(),
                clean_title: "Song 1".to_string(),
                artist: "Artist 1".to_string(),
                album: Some("Album 1".to_string()),
                duration_ms: 180000,
                isrc: Some("US1234567890".to_string()),
                cover_art_url: None,
            },
            PlaylistTrack {
                playlist_id: "spotify:playlist:123".to_string(),
                position: 1,
                source_track_id: "spotify:track:t2".to_string(),
                raw_title: "Song 2".to_string(),
                clean_title: "Song 2".to_string(),
                artist: "Artist 2".to_string(),
                album: None,
                duration_ms: 210000,
                isrc: None,
                cover_art_url: None,
            },
        ];

        db.replace_playlist_tracks("spotify:playlist:123", &tracks).unwrap();

        let fetched_tracks = db.get_playlist_tracks("spotify:playlist:123").unwrap();
        assert_eq!(fetched_tracks.len(), 2);
        assert_eq!(fetched_tracks[0].clean_title, "Song 1");

        // Foreign key cascade check: deleting playlist deletes tracks
        db.delete_imported_playlist("spotify:playlist:123").unwrap();
        let remaining_tracks = db.get_playlist_tracks("spotify:playlist:123").unwrap();
        assert_eq!(remaining_tracks.len(), 0);
    }

    #[test]
    fn test_resolution_cache_crud() {
        let db = Database::open_in_memory().unwrap();

        let res = ResolutionEntry {
            source_track_id: "spotify:track:abc".to_string(),
            isrc: Some("GB1234567890".to_string()),
            qobuz_id: Some("qobuz_999".to_string()),
            tidal_id: Some("tidal_888".to_string()),
            resolution_method: "isrc_exact".to_string(),
            match_confidence: 1.0,
            local_archive_path: None,
            resolved_at: 2000,
        };

        db.upsert_resolution(&res).unwrap();
        let fetched = db.get_resolution("spotify:track:abc").unwrap();
        assert_eq!(fetched, Some(res));

        let not_found = db.get_resolution("nonexistent").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn test_lyrics_cache_crud() {
        let db = Database::open_in_memory().unwrap();

        let entry = LyricsEntry {
            source_track_id: "spotify:track:lyrics1".to_string(),
            isrc: Some("US9999999999".to_string()),
            provider: "paxsenix_ttml".to_string(),
            sync_type: "SYLLABLE".to_string(),
            timeline_json: r#"{"lines":[]}"#.to_string(),
            raw_elrc: Some("[00:01.00] Test".to_string()),
            user_offset_ms: 0,
            fetched_at: 3000,
        };

        db.upsert_lyrics(&entry).unwrap();
        let fetched = db.get_lyrics("spotify:track:lyrics1").unwrap();
        assert_eq!(fetched, Some(entry));

        db.update_lyric_offset("spotify:track:lyrics1", 200).unwrap();
        let updated = db.get_lyrics("spotify:track:lyrics1").unwrap().unwrap();
        assert_eq!(updated.user_offset_ms, 200);
    }
}
