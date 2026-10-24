use crate::core::paths::session_state_file;
use crate::sources::lossless_relay::LosslessPlaybackState;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Session state persisted across app restarts or on SIGHUP
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EchoVenaSession {
    pub active_source: String,
    pub lossless_playback: Option<LosslessPlaybackState>,
    pub active_playlist_id: Option<String>,
    pub volume_percent: u8,
    pub timestamp: i64,
}

impl Default for EchoVenaSession {
    fn default() -> Self {
        Self {
            active_source: "LosslessRelay".to_string(),
            lossless_playback: None,
            active_playlist_id: None,
            volume_percent: 100,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

impl EchoVenaSession {
    pub fn new(
        active_source: &str,
        lossless_playback: Option<LosslessPlaybackState>,
        active_playlist_id: Option<String>,
        volume_percent: u8,
    ) -> Self {
        Self {
            active_source: active_source.to_string(),
            lossless_playback,
            active_playlist_id,
            volume_percent,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Saves session state to the specified path
    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating directory {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)
            .context("failed to serialize EchoVena session state")?;
        fs::write(path, json)
            .with_context(|| format!("failed writing session file {}", path.display()))?;
        Ok(())
    }

    /// Loads session state from the specified path
    pub fn load_from_path(path: &Path) -> Option<Self> {
        if path.exists() {
            let data = fs::read_to_string(path).ok()?;
            serde_json::from_str(&data).ok()
        } else {
            None
        }
    }

    /// Saves session state to default ~/.local/state/echovena/session.json
    pub fn save(&self) -> Result<()> {
        let path = session_state_file().context("unable to determine session_state_file path")?;
        self.save_to_path(&path)
    }

    /// Loads session state from default ~/.local/state/echovena/session.json
    pub fn load() -> Option<Self> {
        let path = session_state_file()?;
        Self::load_from_path(&path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let session_file = temp_dir.path().join("session.json");

        let session = EchoVenaSession::new("LosslessRelay", None, Some("pl_123".to_string()), 85);
        session.save_to_path(&session_file).unwrap();

        assert!(session_file.exists());
        let loaded = EchoVenaSession::load_from_path(&session_file).expect("failed loading session");
        assert_eq!(loaded.active_source, "LosslessRelay");
        assert_eq!(loaded.active_playlist_id, Some("pl_123".to_string()));
        assert_eq!(loaded.volume_percent, 85);
    }
}
