//! YouTube media source (via the external `yt-dlp` binary).
//!
//! Deliberately the thinnest possible wrapper: no YouTube crates, no API keys —
//! [`YouTubeSource`] shells out to `yt-dlp`, which owns the extraction
//! treadmill (PO tokens, signature ciphers, throttling). Search uses
//! `--flat-playlist --dump-json ytsearchN:`; playback downloads the AAC/M4A
//! audio-only format (itag 140, decodable by rodio's default `mp4` feature) to
//! a tempfile and plays it through the shared [`LocalPlayer`] — the same
//! download-then-play model as the Subsonic source.
//!
//! This source is **unofficial-fragile by design** (see plan-multi-source.md):
//! when YouTube hardens extraction, updating the `yt-dlp` binary is the fix,
//! not a echovena release.
//!
//! ## URIs
//!
//! `youtube:<video-id>` — the suffix is the 11-character video id. Ids are
//! charset-validated before being passed to `yt-dlp` (defense against argument
//! injection; they are embedded in a full watch URL, never passed bare).

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::core::playlist_sync::SyncTrack;
use crate::core::plugin_api::{SearchResults, TrackInfo};
use crate::core::source::Searcher;
use crate::infra::audio::LocalPlayer;

pub mod dispatch;
pub mod playlists;
mod types;

use types::YtVideo;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const YOUTUBE_PREFIX: &str = "youtube:";

/// The `yt-dlp` binary to invoke when the user has not configured
/// `behavior.ytdlp_path` — resolved through `$PATH`.
const DEFAULT_YTDLP: &str = "yt-dlp";

/// Result page size for searches. Each flat-search page is one remote request
/// for yt-dlp, so this is also the dominant term in search latency (~3-6 s).
const SEARCH_LIMIT: usize = 20;

/// Audio format selector: itag 140 (AAC/M4A ~128 kbps) first — the format the
/// bundled symphonia decoder is known to handle — then any m4a, then 251 (Opus), then whatever audio exists.
const FORMAT_SELECTOR: &str = "140/bestaudio[ext=m4a]/251/bestaudio";

/// Extractor override for the download retry: YouTube's GVS PO-token
/// enforcement 403s content on default player clients, while android/web
/// clients serve streams without tokens.
const FALLBACK_PLAYER_CLIENTS: &str = "youtube:player_client=android,web";

/// Check if ffmpeg is available on PATH or in standard package directories.
pub fn find_ffmpeg_dir() -> Option<std::path::PathBuf> {
  let mut cmd = std::process::Command::new("ffmpeg");
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
  }
  if cmd.arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false) {
    return None;
  }

  #[cfg(windows)]
  {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
      let winget_packages = std::path::Path::new(&local_app_data).join("Microsoft").join("WinGet").join("Packages");
      if let Ok(entries) = std::fs::read_dir(&winget_packages) {
        for entry in entries.flatten() {
          let name = entry.file_name().to_string_lossy().to_string();
          if name.contains("FFmpeg") {
            let mut stack = vec![entry.path()];
            while let Some(dir) = stack.pop() {
              if dir.file_name().and_then(|n| n.to_str()) == Some("bin") && dir.join("ffmpeg.exe").is_file() {
                return Some(dir);
              }
              if let Ok(sub_entries) = std::fs::read_dir(&dir) {
                for sub in sub_entries.flatten() {
                  if sub.path().is_dir() {
                    stack.push(sub.path());
                  }
                }
              }
            }
          }
        }
      }
    }
  }

  None
}

/// Returns true if ffmpeg is available on PATH or discovered in package directories.
pub fn has_ffmpeg() -> bool {
  let mut cmd = std::process::Command::new("ffmpeg");
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
  }
  if cmd.arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false) {
    return true;
  }
  find_ffmpeg_dir().is_some()
}

/// Resolve the actual binary path for `yt-dlp`.
pub fn resolve_ytdlp(override_path: Option<&str>) -> String {
  if let Some(p) = override_path {
    let trimmed = p.trim();
    if !trimmed.is_empty() {
      return trimmed.to_string();
    }
  }

  // Check default on PATH
  let mut cmd = std::process::Command::new(DEFAULT_YTDLP);
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
  }
  if cmd.arg("--version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false) {
    return DEFAULT_YTDLP.to_string();
  }

  #[cfg(windows)]
  {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
      let winget_packages = std::path::Path::new(&local_app_data).join("Microsoft").join("WinGet").join("Packages");
      if let Ok(entries) = std::fs::read_dir(&winget_packages) {
        for entry in entries.flatten() {
          let name = entry.file_name().to_string_lossy().to_string();
          if name.contains("yt-dlp") {
            let candidate = entry.path().join("yt-dlp.exe");
            if candidate.is_file() {
              return candidate.to_string_lossy().to_string();
            }
          }
        }
      }
      let windows_apps = std::path::Path::new(&local_app_data).join("Microsoft").join("WindowsApps").join("yt-dlp.exe");
      if windows_apps.is_file() {
        return windows_apps.to_string_lossy().to_string();
      }
    }

    if let Ok(program_data) = std::env::var("ProgramData") {
      let choco = std::path::Path::new(&program_data).join("chocolatey").join("bin").join("yt-dlp.exe");
      if choco.is_file() {
        return choco.to_string_lossy().to_string();
      }
    }

    if let Ok(userprofile) = std::env::var("USERPROFILE") {
      let scoop = std::path::Path::new(&userprofile).join("scoop").join("shims").join("yt-dlp.exe");
      if scoop.is_file() {
        return scoop.to_string_lossy().to_string();
      }
      let local_bin = std::path::Path::new(&userprofile).join(".local").join("bin").join("yt-dlp.exe");
      if local_bin.is_file() {
        return local_bin.to_string_lossy().to_string();
      }
    }
  }

  DEFAULT_YTDLP.to_string()
}

/// What the download prints on stdout once yt-dlp has the video's page: the
/// fields [`YtVideo`] reads, as one JSON object. The download already resolves
/// the page, so a video started from a bare URI (#716) gets its title,
/// channel and duration with no second request. `--print` alone would skip
/// the download (it implies `--simulate`), hence `--no-simulate` next to it.
const METADATA_TEMPLATE: &str = "%(.{id,title,channel,uploader,duration,view_count,thumbnail})j";

/// Cap on a search invocation. yt-dlp searches normally return in a few
/// seconds; a hung process must not wedge the IoEvent pump forever.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(45);

/// Cap on a download invocation (a ~4 MiB audio file, but allow slow links).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);

/// Keeps a windowed build (the GUI, the desktop app) from opening a console for yt-dlp.
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ---------------------------------------------------------------------------
// YouTubePlaybackState
// ---------------------------------------------------------------------------

/// The active YouTube playback session — same decoupled shape as the Subsonic
/// one: it owns the live [`LocalPlayer`], the queue of searched videos with
/// the current index, and the downloaded tempfile for the playing track. It
/// never writes Spotify/librespot fields; the playbar reads progress/pause
/// live from `player`.
pub struct YouTubePlaybackState {
  pub player: Arc<LocalPlayer>,
  /// Source handle, reused to download each track on Next/advance.
  pub source: Arc<YouTubeSource>,
  /// The queued videos (with search metadata) in order.
  pub tracks: Vec<TrackInfo>,
  /// Index into [`tracks`](Self::tracks) of the currently playing video.
  pub index: usize,
  /// Auto-advance guard — set while a track change is downloading/decoding so
  /// the runner tick does not mistake the empty sink for end-of-track. The
  /// yt-dlp download window is even longer than Subsonic's, so this guard is
  /// load-bearing.
  pub advancing: bool,
  /// The downloaded audio for the current track, kept alive while it plays.
  pub tempfile: tempfile::NamedTempFile,
  /// Path to the playing audio file on disk (persisted cache or tempfile).
  pub audio_path: std::path::PathBuf,
  /// Backup of the pre-shuffle track order while shuffle is on (`None` in natural
  /// order). Set by [`set_shuffle(true)`](Self::set_shuffle); restored exactly by
  /// `set_shuffle(false)`.
  pub shuffle_backup: Option<crate::infra::queue::ShuffleBackup>,
  /// A seek and pause to apply when the next track is staged (device
  /// recovery, the native queue's resume).
  pub resume_at: Option<crate::infra::queue::ResumePoint>,
  /// Video id currently being prebuffered in the background (if any).
  pub prebuffering_id: Option<String>,
  /// Video id successfully prebuffered in the background (if any).
  pub prebuffered_id: Option<String>,
}

impl YouTubePlaybackState {
  /// The currently playing video, if `index` is in range.
  #[cfg(feature = "tui")]
  pub fn current(&self) -> Option<&TrackInfo> {
    self.tracks.get(self.index)
  }

  /// Trigger eager next-track pre-buffering for upcoming queue tracks.
  pub fn maybe_prebuffer_next(&mut self) {
    if self.advancing {
      return;
    }
    let Some(_current_track) = self.tracks.get(self.index) else {
      return;
    };

    // Inspect upcoming tracks: index + 1, then index + 2
    for offset in 1..=2 {
      let candidate_idx = self.index + offset;
      let Some(next_track) = self.tracks.get(candidate_idx) else {
        break;
      };

      let vid_opt = next_track.uri.as_deref()
        .and_then(extract_youtube_video_id)
        .or_else(|| next_track.id.as_deref().and_then(extract_youtube_video_id))
        .or_else(|| {
          next_track.uri.as_deref().and_then(|u| video_id_from_uri(u).ok().map(str::to_string))
        });

      let Some(next_video_id) = vid_opt else {
        continue;
      };

      if self.prebuffering_id.as_deref() == Some(&next_video_id) {
        break;
      }

      if cached_audio_path(&next_video_id).is_some() {
        if self.prebuffered_id.as_deref() != Some(&next_video_id) {
          self.prebuffered_id = Some(next_video_id);
        }
        continue;
      }

      self.prebuffering_id = Some(next_video_id.clone());
      let source = Arc::clone(&self.source);
      let vid = next_video_id;
      tokio::spawn(async move {
        let _ = source.prefetch_to_cache(&vid).await;
      });
      break;
    }
  }

  /// Turn in-place shuffle on or off — see
  /// [`toggle_shuffle`](crate::infra::queue::toggle_shuffle) for the shared
  /// semantics (current track stays playing at the front; un-shuffle restores
  /// order + index; idempotent).
  pub fn set_shuffle(&mut self, on: bool) {
    crate::infra::queue::toggle_shuffle(
      &mut self.tracks,
      &mut self.index,
      &mut self.shuffle_backup,
      on,
    );
  }
}

// ---------------------------------------------------------------------------
// URI helpers
// ---------------------------------------------------------------------------

/// Whether a URI is owned by the YouTube source.
pub fn is_youtube_uri(uri: &str) -> bool {
  uri.starts_with(YOUTUBE_PREFIX)
    || uri.starts_with("yt:")
    || uri.starts_with("youtube:video:")
    || uri.starts_with("yt:video:")
    || uri.starts_with("youtube:playlist:")
    || uri.starts_with("yt:playlist:")
}

/// Strip the `youtube:` prefix and return the video id, validating its
/// charset. The validation is load-bearing: the id is interpolated into a
/// `yt-dlp` argument, so a crafted "id" must not be able to smuggle flags or
/// URL structure.
pub fn video_id_from_uri(uri: &str) -> Result<&str> {
  let id = uri
    .strip_prefix("youtube:video:")
    .or_else(|| uri.strip_prefix("yt:video:"))
    .or_else(|| uri.strip_prefix(YOUTUBE_PREFIX))
    .or_else(|| uri.strip_prefix("yt:"))
    .ok_or_else(|| anyhow!("Not a YouTube URI: {}", uri))?;
  if id.is_empty()
    || !id
      .bytes()
      .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
  {
    bail!("Invalid YouTube video id: {id:?}");
  }
  Ok(id)
}

/// Extract an 11-character YouTube video ID from arbitrary text (URL, URI, or raw ID).
pub fn extract_youtube_video_id(input: &str) -> Option<String> {
  let trimmed = input.trim();
  if trimmed.is_empty() {
    return None;
  }
  // 1. Bare 11-char ID
  if trimmed.len() == 11 && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
    return Some(trimmed.to_string());
  }
  // 2. youtube: / yt: prefixes
  let candidate = trimmed
    .strip_prefix("youtube:video:")
    .or_else(|| trimmed.strip_prefix("yt:video:"))
    .or_else(|| trimmed.strip_prefix(YOUTUBE_PREFIX))
    .or_else(|| trimmed.strip_prefix("yt:"));
  if let Some(id) = candidate {
    let id = id.trim();
    if id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
      return Some(id.to_string());
    }
  }
  // 3. URLs
  let parse_target = if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
    format!("https://{}", trimmed)
  } else {
    trimmed.to_string()
  };
  if let Ok(url) = url::Url::parse(&parse_target) {
    if url.host_str().is_some_and(|h| h.contains("youtu.be")) {
      let path = url.path().trim_start_matches('/');
      if path.len() >= 11 {
        let candidate = &path[..11];
        if candidate.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
          return Some(candidate.to_string());
        }
      }
    }
    if url.host_str().is_some_and(|h| h.contains("youtube.com")) {
      for (k, v) in url.query_pairs() {
        if k == "v" && v.len() >= 11 {
          let candidate = &v[..11];
          if candidate.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Some(candidate.to_string());
          }
        }
      }
    }
  }
  None
}

/// Build the `youtube:` URI for a video id.
pub fn uri_for_video_id(id: &str) -> String {
  format!("{YOUTUBE_PREFIX}{id}")
}

/// Check if an audio file is cached on disk for `video_id`.
pub fn cached_audio_path(video_id: &str) -> Option<std::path::PathBuf> {
  let cache_dir = crate::core::paths::audio_cache_dir()?;
  let extensions = ["m4a", "mp3", "flac", "opus", "webm"];
  for ext in extensions {
    let candidate = cache_dir.join(format!("{video_id}.{ext}"));
    if candidate.is_file() {
      if let Ok(meta) = std::fs::metadata(&candidate) {
        if meta.len() > 1024 {
          return Some(candidate);
        }
      }
    }
  }
  None
}

// ---------------------------------------------------------------------------
// YouTubeSource — yt-dlp wrapper
// ---------------------------------------------------------------------------

/// Client for YouTube backed by an external `yt-dlp` binary.
pub struct YouTubeSource {
  /// Path or name of the `yt-dlp` binary (name is resolved via `$PATH`).
  ytdlp: String,
}

impl YouTubeSource {
  /// `ytdlp_path` is the user's `behavior.ytdlp_path` override, if any.
  pub fn new(ytdlp_path: Option<String>) -> Self {
    let resolved = resolve_ytdlp(ytdlp_path.as_deref());
    YouTubeSource {
      ytdlp: resolved,
    }
  }

  /// Search YouTube by keywords, returning up to [`SEARCH_LIMIT`] videos.
  ///
  /// Flat extraction (`--flat-playlist`) returns one JSON object per line
  /// without resolving each video page, keeping the search to one remote
  /// round-trip.
  pub async fn search_videos(&self, query: &str) -> Result<Vec<YtVideo>> {
    let query_arg = format!("ytsearch{SEARCH_LIMIT}:{query}");
    let stdout = self
      .run(
        [
          "--flat-playlist",
          "--dump-json",
          "--no-warnings",
          query_arg.as_str(),
        ],
        SEARCH_TIMEOUT,
      )
      .await?;

    // One JSON object per line; skip lines that fail to parse rather than
    // failing the whole search (yt-dlp occasionally interleaves notices).
    Ok(
      stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<YtVideo>(line).ok())
        .filter(|v| !v.id.is_empty())
        .collect(),
    )
  }

  /// Search results as sync candidates, with the duration left unknown when yt-dlp reports none.
  pub(crate) async fn sync_search(&self, query: &str) -> Result<Vec<SyncTrack>> {
    Ok(
      self
        .search_videos(query)
        .await?
        .iter()
        .map(video_to_sync_track)
        .collect(),
    )
  }

  /// Download a video's audio (itag 140 AAC/M4A preferred) to `dest`, and
  /// return the video's metadata when yt-dlp printed it (see
  /// [`METADATA_TEMPLATE`]); a download whose metadata cannot be read still
  /// succeeds, with `None`.
  ///
  /// When `ffmpeg` is on `$PATH`, yt-dlp remuxes the DASH fragments into a
  /// plain MP4 container ("FixupM4a"); without it the file stays fragmented,
  /// which the bundled decoder may reject — the dispatch surfaces an
  /// install-ffmpeg hint on decode failure for that case.
  pub async fn download_audio(&self, video_id: &str, dest: &Path) -> Result<Option<TrackInfo>> {
    // Belt-and-braces: the dispatch already validated the id, but this method
    // is also callable directly (tests, future callers).
    if !video_id
      .bytes()
      .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
      bail!("Invalid YouTube video id: {video_id:?}");
    }

    if let Some(cached) = cached_audio_path(video_id) {
      if cached != dest {
        let _ = std::fs::copy(&cached, dest);
      }
      return Ok(None);
    }

    let url = format!("https://www.youtube.com/watch?v={video_id}");

    // Use an isolated staging directory so yt-dlp can use .part files, atomic renames,
    // and extract audio without sharing violations against open file handles.
    let base_temp = crate::core::paths::audio_cache_dir()
      .unwrap_or_else(std::env::temp_dir);
    let stage_dir = base_temp.join(format!("echovena_yt_{}_{}", std::process::id(), video_id));
    let _ = std::fs::create_dir_all(&stage_dir);
    let stage_file = stage_dir.join(format!("{video_id}.m4a"));
    let stage_str = stage_file
      .to_str()
      .ok_or_else(|| anyhow!("Non-UTF-8 staging path"))?;

    let stdout_result = run_with_embedded_fallback(|fallback| {
      let args = download_args(stage_str, &url, fallback);
      async move { self.run(args, DOWNLOAD_TIMEOUT).await }
    })
    .await;

    // Locate the downloaded file in stage_dir (which might have been saved as .m4a, .webm, or .opus)
    let downloaded_file = if stage_file.exists() && stage_file.metadata().map(|m| m.len() > 1024).unwrap_or(false) {
      Some(stage_file.clone())
    } else {
      let mut found = None;
      if let Ok(entries) = std::fs::read_dir(&stage_dir) {
        for entry in entries.flatten() {
          let p = entry.path();
          if p.is_file() {
            if let Ok(m) = p.metadata() {
              if m.len() > 1024 {
                found = Some(p);
                break;
              }
            }
          }
        }
      }
      found
    };

    match stdout_result {
      Ok(stdout) => {
        if let Some(dl) = downloaded_file {
          // Copy to requested destination
          let _ = std::fs::copy(&dl, dest);

          // Also persist into audio cache directory
          if let Some(cache_dir) = crate::core::paths::audio_cache_dir() {
            let _ = std::fs::create_dir_all(&cache_dir);
            let ext = dl.extension().and_then(|e| e.to_str()).unwrap_or("m4a");
            let target_path = cache_dir.join(format!("{video_id}.{ext}"));
            let _ = std::fs::copy(&dl, &target_path);
          }

          let _ = std::fs::remove_dir_all(&stage_dir);
          Ok(track_from_download_output(&stdout, video_id))
        } else {
          let _ = std::fs::remove_dir_all(&stage_dir);
          bail!("yt-dlp completed successfully but no audio file was output for {video_id}")
        }
      }
      Err(e) => {
        let _ = std::fs::remove_dir_all(&stage_dir);
        Err(e)
      }
    }
  }

  /// Prefetch a video's audio to the local cache directory (~/.cache/echovena/audio/).
  pub async fn prefetch_to_cache(&self, video_id: &str) -> Result<std::path::PathBuf> {
    if let Some(cached) = cached_audio_path(video_id) {
      return Ok(cached);
    }
    let cache_dir = crate::core::paths::audio_cache_dir()
      .ok_or_else(|| anyhow!("Audio cache directory not available"))?;
    std::fs::create_dir_all(&cache_dir)?;

    let target_path = cache_dir.join(format!("{video_id}.m4a"));
    let _ = self.download_audio(video_id, &target_path).await?;
    if let Some(cached) = cached_audio_path(video_id) {
      Ok(cached)
    } else {
      Ok(target_path)
    }
  }

  /// Run `yt-dlp` with `args`, returning stdout. Fails with the tail of
  /// stderr on a non-zero exit, a clear hint when the binary is missing, and
  /// kills the process on timeout (so a wedged extractor can't leak).
  async fn run<I, S>(&self, args: I, timeout: Duration) -> Result<String>
  where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
  {
    let mut command = Command::new(&self.ytdlp);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command
      .args(args)
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .kill_on_drop(true)
      .spawn()
      .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
          anyhow!(
            "`{}` not found — install yt-dlp (or set behavior.ytdlp_path)",
            self.ytdlp
          )
        } else {
          anyhow!("failed to spawn `{}`: {e}", self.ytdlp)
        }
      })?;

    // Read both pipes concurrently with wait(); reading stdout after wait()
    // would deadlock once the pipe buffer fills on large outputs.
    let mut stdout_pipe = child.stdout.take().context("yt-dlp stdout not piped")?;
    let mut stderr_pipe = child.stderr.take().context("yt-dlp stderr not piped")?;
    let io = async {
      let (mut out, mut err) = (String::new(), String::new());
      let _ = tokio::join!(
        stdout_pipe.read_to_string(&mut out),
        stderr_pipe.read_to_string(&mut err),
      );
      (out, err)
    };

    let ((stdout, stderr), status) =
      tokio::time::timeout(timeout, async { tokio::join!(io, child.wait()) })
        .await
        .map_err(|_| anyhow!("yt-dlp timed out after {}s", timeout.as_secs()))?;

    let status = status.context("waiting for yt-dlp")?;
    if !status.success() {
      // Collect errors or non-empty tail from stderr
      let mut error_lines: Vec<_> = stderr
        .lines()
        .filter(|l| l.contains("ERROR:") || l.contains("error:") || l.contains("WARNING:"))
        .collect();
      if error_lines.is_empty() {
        error_lines = stderr
          .lines()
          .filter(|l| !l.trim().is_empty())
          .rev()
          .take(3)
          .collect::<Vec<_>>()
          .into_iter()
          .rev()
          .collect();
      }
      let tail = error_lines.join(" | ");
      bail!(
        "yt-dlp failed ({}): {}",
        status,
        if tail.is_empty() {
          "no error output"
        } else {
          tail.as_str()
        }
      );
    }
    Ok(stdout)
  }
}

/// The download's attempt sequence: the default clients first, then one
/// retry through the fallback player clients (see [`FALLBACK_PLAYER_CLIENTS`]).
async fn run_with_embedded_fallback<T, F, Fut>(mut attempt: F) -> Result<T>
where
  F: FnMut(bool) -> Fut,
  Fut: std::future::Future<Output = Result<T>>,
{
  match attempt(false).await {
    Ok(out) => Ok(out),
    Err(primary) => match attempt(true).await {
      Ok(out) => Ok(out),
      Err(fallback) => {
        log::info!("youtube: download fallback attempt also failed: {fallback:#}");
        Err(primary)
      }
    },
  }
}

/// Arguments for one download attempt; the retry adds the fallback client extractor override.
fn download_args(dest: &str, url: &str, fallback: bool) -> Vec<String> {
  let mut args: Vec<String> = Vec::new();
  if fallback {
    args.push("--extractor-args".to_string());
    args.push(FALLBACK_PLAYER_CLIENTS.to_string());
  }

  // Include user youtube cookies if configured
  for cookie_arg in crate::sources::sync::auth::YouTubeAuthHandler::ytdlp_cookie_args() {
    args.push(cookie_arg);
  }

  if let Some(ffmpeg_dir) = find_ffmpeg_dir() {
    args.push("--ffmpeg-location".to_string());
    args.push(ffmpeg_dir.to_string_lossy().to_string());
  }

  args.push("--no-simulate".to_string());
  args.push("--print".to_string());
  args.push(METADATA_TEMPLATE.to_string());
  args.push("-f".to_string());
  args.push(FORMAT_SELECTOR.to_string());

  if has_ffmpeg() {
    args.push("-x".to_string());
  }

  args.push("--concurrent-fragments".to_string());
  args.push("4".to_string());
  args.push("--buffer-size".to_string());
  args.push("16K".to_string());
  args.push("--no-playlist".to_string());
  args.push("--no-warnings".to_string());
  args.push("-o".to_string());
  args.push(dest.to_string());
  args.push(url.to_string());
  args
}

impl Searcher for YouTubeSource {
  async fn search(&self, query: &str) -> Result<SearchResults> {
    let videos = self.search_videos(query).await?;
    Ok(SearchResults {
      tracks: videos.iter().map(video_to_track_info).collect(),
      albums: vec![],
      artists: vec![],
      playlists: vec![],
      shows: vec![],
    })
  }
}

// ---------------------------------------------------------------------------
// Domain type conversions
// ---------------------------------------------------------------------------

/// Map a search row onto the shared [`TrackInfo`] the track table, search
/// results and playbar already render.
///
/// - `uri` = `youtube:<video-id>`.
/// - `artists` (the subtitle column) = the channel name.
/// - `album` = a `"YouTube • 863M views"` style summary.
/// - `duration_ms` = 0 for livestreams (the playbar's LIVE sentinel).
fn video_to_track_info(v: &YtVideo) -> TrackInfo {
  let channel = v
    .channel
    .as_deref()
    .or(v.uploader.as_deref())
    .unwrap_or_default()
    .trim()
    .to_string();

  TrackInfo {
    generated_name: None,
    uri: Some(uri_for_video_id(&v.id)),
    name: v.title.trim().to_string(),
    artists: if channel.is_empty() {
      vec![]
    } else {
      vec![channel]
    },
    album: video_summary(v.view_count),
    duration_ms: v.duration.map(|s| (s * 1000.0) as u64).unwrap_or(0),
    id: Some(v.id.clone()),
    album_id: None,
    artist_refs: vec![],
    is_playable: true,
    is_local: false,
    track_number: 0,
    explicit: false,
    // Flat extraction often omits the top-level `thumbnail`; every video id has
    // a deterministic thumbnail URL, so fall back to that.
    image_url: v
      .thumbnail
      .clone()
      .or_else(|| Some(thumbnail_url_for_video_id(&v.id))),
    release_date: None,
  }
}

/// The metadata a download printed for `video_id` (see [`METADATA_TEMPLATE`]),
/// mapped like a search row. The last line that parses for this video wins;
/// anything else on stdout (a notice, an older yt-dlp printing nothing) gives
/// `None`, never an error.
///
/// A line with no duration gives `None` as well: a row with a 0 duration reads
/// as a livestream, so a revisit would refuse to play a video that just
/// downloaded in full. The placeholder stays replayable instead.
fn track_from_download_output(stdout: &str, video_id: &str) -> Option<TrackInfo> {
  stdout
    .lines()
    .rev()
    .filter_map(|line| serde_json::from_str::<YtVideo>(line.trim()).ok())
    .find(|v| v.id == video_id && !v.title.trim().is_empty())
    .filter(|v| v.duration.is_some_and(|secs| secs > 0.0))
    .map(|v| video_to_track_info(&v))
}

/// Map a search row onto the sync currency; an absent duration stays absent.
fn video_to_sync_track(v: &YtVideo) -> SyncTrack {
  SyncTrack {
    key: v.id.clone(),
    isrc: None,
    title: v.title.trim().to_string(),
    artist: v
      .channel
      .as_deref()
      .or(v.uploader.as_deref())
      .unwrap_or_default()
      .trim()
      .to_string(),
    duration_ms: v.duration.filter(|s| *s > 0.0).map(|s| (s * 1000.0) as u64),
  }
}

/// Deterministic YouTube thumbnail URL for a video id. `hqdefault.jpg` exists
/// for every video (unlike `maxresdefault`), so it serves as the cover-art
/// fallback when yt-dlp's flat rows or a stored playlist carry no thumbnail.
pub(crate) fn thumbnail_url_for_video_id(video_id: &str) -> String {
  format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")
}

/// `"YouTube • 863M views"`-style one-liner for the album column.
fn video_summary(view_count: Option<u64>) -> String {
  match view_count {
    Some(views) => format!("YouTube \u{2022} {} views", humanize_count(views)),
    None => "YouTube".to_string(),
  }
}

/// `1234` -> `"1.2K"`, `863781229` -> `"863.8M"` — coarse is fine for a
/// subtitle column.
fn humanize_count(n: u64) -> String {
  match n {
    0..=999 => n.to_string(),
    1_000..=999_999 => format!("{:.1}K", n as f64 / 1_000.0),
    1_000_000..=999_999_999 => format!("{:.1}M", n as f64 / 1_000_000.0),
    _ => format!("{:.1}B", n as f64 / 1_000_000_000.0),
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;

  // Two representative flat-search lines (trimmed to the fields we read) plus
  // one junk line that must be skipped.
  const SEARCH_LINES: &str = concat!(
    r#"{"id": "5NV6Rdv1a3I", "title": " Daft Punk - Get Lucky (Official Audio) ", "channel": "Daft Punk", "uploader": "Daft Punk", "duration": 249.0, "view_count": 863781229}"#,
    "\n",
    "not json at all\n",
    r#"{"id": "dQw4w9WgXcQ", "title": "Live now", "uploader": "Some Channel", "duration": null}"#,
    "\n",
  );

  #[test]
  fn parse_search_lines_and_map_to_track_info() {
    let videos: Vec<YtVideo> = SEARCH_LINES
      .lines()
      .filter_map(|l| serde_json::from_str(l).ok())
      .collect();
    assert_eq!(videos.len(), 2, "junk lines must be skipped");

    let row = video_to_track_info(&videos[0]);
    assert_eq!(row.uri.as_deref(), Some("youtube:5NV6Rdv1a3I"));
    assert_eq!(row.name, "Daft Punk - Get Lucky (Official Audio)");
    assert_eq!(row.artists, vec!["Daft Punk"]);
    assert_eq!(row.album, "YouTube \u{2022} 863.8M views");
    assert_eq!(row.duration_ms, 249_000);
    assert_eq!(row.id.as_deref(), Some("5NV6Rdv1a3I"));
    assert!(row.is_playable);

    // No duration (livestream) -> the LIVE-style 0 sentinel; channel falls
    // back to uploader.
    let live = video_to_track_info(&videos[1]);
    assert_eq!(live.duration_ms, 0);
    assert_eq!(live.artists, vec!["Some Channel"]);
    assert_eq!(live.album, "YouTube");
  }

  #[test]
  fn youtube_uri_round_trip() {
    let uri = uri_for_video_id("5NV6Rdv1a3I");
    assert_eq!(uri, "youtube:5NV6Rdv1a3I");
    assert!(is_youtube_uri(&uri));
    assert_eq!(video_id_from_uri(&uri).unwrap(), "5NV6Rdv1a3I");
    // Ids may contain - and _.
    assert_eq!(video_id_from_uri("youtube:a-b_c123").unwrap(), "a-b_c123");
  }

  #[test]
  fn video_id_rejects_injection_shaped_input() {
    // Flags, URL structure, spaces, and other schemes must all be rejected.
    assert!(video_id_from_uri("youtube:--exec=rm").is_err());
    assert!(video_id_from_uri("youtube:id&x=1").is_err());
    assert!(video_id_from_uri("youtube:a b").is_err());
    assert!(video_id_from_uri("youtube:").is_err());
    assert!(video_id_from_uri("spotify:track:x").is_err());
    assert!(!is_youtube_uri("radio:https://x.example/s"));
    // A leading dash alone is caught by charset? No — '-' is allowed in ids,
    // so bare "-flag" survives charset checks. It is defused structurally:
    // the id is embedded in a watch URL, never passed as its own argument.
    assert!(video_id_from_uri("youtube:-abcdefghij").is_ok());
  }

  #[test]
  fn summary_humanizes_view_counts() {
    assert_eq!(video_summary(None), "YouTube");
    assert_eq!(video_summary(Some(999)), "YouTube \u{2022} 999 views");
    assert_eq!(video_summary(Some(1_500)), "YouTube \u{2022} 1.5K views");
    assert_eq!(
      video_summary(Some(863_781_229)),
      "YouTube \u{2022} 863.8M views"
    );
    assert_eq!(
      video_summary(Some(1_200_000_000)),
      "YouTube \u{2022} 1.2B views"
    );
  }

  #[test]
  fn download_retry_only_prepends_the_embedded_client_override() {
    let url = "https://www.youtube.com/watch?v=5NV6Rdv1a3I";
    let first_try = download_args("dest.m4a", url, false);
    let retry = download_args("dest.m4a", url, true);

    assert!(!first_try.iter().any(|a| a == "--extractor-args"));
    assert_eq!(&retry[..2], ["--extractor-args", FALLBACK_PLAYER_CLIENTS]);
    assert_eq!(retry[2..], first_try[..], "everything else stays identical");
  }

  #[test]
  fn every_download_attempt_prints_the_metadata_and_still_downloads() {
    let url = "https://www.youtube.com/watch?v=5NV6Rdv1a3I";
    for embedded in [false, true] {
      let args = download_args("dest.m4a", url, embedded);
      let print = args.iter().position(|a| a == "--print").unwrap();
      assert_eq!(args[print + 1], METADATA_TEMPLATE);
      // Without it `--print` turns the download into a dry run.
      assert!(args.iter().any(|a| a == "--no-simulate"));
      assert_eq!(args[args.len() - 3..], ["-o", "dest.m4a", url]);
    }
  }

  /// What yt-dlp 2026.03.17 printed for `METADATA_TEMPLATE` on a local info
  /// JSON (`--load-info-json`), with a notice line around it.
  const DOWNLOAD_STDOUT: &str = concat!(
    "[info] a notice\n",
    r#"{"id": "5NV6Rdv1a3I", "title": "Daft Punk - Get Lucky (Official Audio)", "channel": "Daft Punk", "uploader": "Daft Punk", "duration": 249.0, "view_count": 863781229, "thumbnail": "https://i.ytimg.com/vi/5NV6Rdv1a3I/maxresdefault.jpg"}"#,
    "\n",
  );

  #[test]
  fn a_download_reports_the_title_channel_and_duration_it_printed() {
    let track = track_from_download_output(DOWNLOAD_STDOUT, "5NV6Rdv1a3I").unwrap();
    assert_eq!(track.uri.as_deref(), Some("youtube:5NV6Rdv1a3I"));
    assert_eq!(track.name, "Daft Punk - Get Lucky (Official Audio)");
    assert_eq!(track.artists, ["Daft Punk"]);
    assert_eq!(track.duration_ms, 249_000);
    assert_eq!(
      track.image_url.as_deref(),
      Some("https://i.ytimg.com/vi/5NV6Rdv1a3I/maxresdefault.jpg")
    );
  }

  #[test]
  fn a_download_with_no_usable_metadata_reports_none() {
    assert!(track_from_download_output("", "5NV6Rdv1a3I").is_none());
    assert!(track_from_download_output("not json\n", "5NV6Rdv1a3I").is_none());
    // Another video's line is not this video's title.
    assert!(track_from_download_output(DOWNLOAD_STDOUT, "dQw4w9WgXcQ").is_none());
    // A line with no title would only replace the URI with an empty name.
    assert!(track_from_download_output(r#"{"id": "5NV6Rdv1a3I"}"#, "5NV6Rdv1a3I").is_none());
  }

  #[test]
  fn a_download_with_no_duration_keeps_the_row_out_of_the_livestream_check() {
    for duration in ["null", "0"] {
      let stdout = format!(
        r#"{{"id": "5NV6Rdv1a3I", "title": "Daft Punk - Get Lucky", "duration": {duration}}}"#
      );
      assert!(
        track_from_download_output(&stdout, "5NV6Rdv1a3I").is_none(),
        "duration {duration} would turn the row into Plan::Live on a revisit"
      );
    }
  }

  #[tokio::test]
  async fn download_fallback_never_runs_after_a_primary_success() {
    let mut attempts = Vec::new();
    run_with_embedded_fallback(|embedded| {
      attempts.push(embedded);
      async { Ok(()) }
    })
    .await
    .unwrap();
    assert_eq!(attempts, [false]);
  }

  #[tokio::test]
  async fn download_retries_exactly_once_through_the_embedded_clients() {
    let mut attempts = Vec::new();
    run_with_embedded_fallback(|embedded| {
      attempts.push(embedded);
      async move {
        if embedded {
          Ok(())
        } else {
          Err(anyhow!("HTTP Error 403"))
        }
      }
    })
    .await
    .unwrap();
    assert_eq!(attempts, [false, true]);
  }

  #[tokio::test]
  async fn double_download_failure_surfaces_the_primary_error() {
    let err = run_with_embedded_fallback::<(), _, _>(|embedded| async move {
      Err(anyhow!(if embedded {
        "format not available"
      } else {
        "HTTP Error 403"
      }))
    })
    .await
    .unwrap_err();
    assert!(err.to_string().contains("403"), "got: {err}");
  }

  #[test]
  fn missing_binary_yields_actionable_error() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let source = YouTubeSource::new(Some("definitely-not-a-real-binary".into()));
    let err = rt
      .block_on(source.search_videos("test"))
      .expect_err("missing binary must error");
    assert!(
      err.to_string().contains("install yt-dlp"),
      "error should tell the user what to do, got: {err}"
    );
  }

  /// Live search against real YouTube through yt-dlp. Ignored by default
  /// (network + binary); run with:
  /// `cargo test --features youtube -- --ignored live_search`
  #[tokio::test]
  #[ignore = "requires yt-dlp on PATH and hits live YouTube"]
  async fn live_search_returns_playable_rows() {
    let source = YouTubeSource::new(None);
    let videos = source
      .search_videos("daft punk get lucky")
      .await
      .expect("search should succeed");
    assert!(!videos.is_empty(), "search should match videos");
    for v in &videos {
      let row = video_to_track_info(v);
      let uri = row.uri.expect("row must carry a uri");
      assert!(is_youtube_uri(&uri));
      video_id_from_uri(&uri).expect("id must round-trip");
    }
  }

  /// Live download + decode + play through the shared sink. Ignored (network,
  /// yt-dlp, and an audio output device); run:
  /// `cargo test --features youtube -- --ignored live_download`
  #[tokio::test(flavor = "multi_thread")]
  #[ignore = "requires yt-dlp on PATH, network, AND an audio output device"]
  async fn live_download_and_play_through_sink() {
    let source = YouTubeSource::new(None);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    source
      .download_audio("5NV6Rdv1a3I", tmp.path())
      .await
      .expect("download should succeed");
    let len = std::fs::metadata(tmp.path()).unwrap().len();
    assert!(
      len > 100_000,
      "audio file should be non-trivial, got {len}B"
    );

    let player = Arc::new(LocalPlayer::new().expect("open default output device"));
    let path = tmp.path().to_path_buf();
    let decode_player = Arc::clone(&player);
    tokio::task::spawn_blocking(move || decode_player.play_file(&path))
      .await
      .unwrap()
      .expect("m4a should decode and play");

    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
      player.position() >= Duration::from_millis(500),
      "playback position should advance, got {:?}",
      player.position()
    );
    player.stop();
  }

  #[test]
  fn test_cached_audio_path_returns_none_for_nonexistent() {
    assert!(cached_audio_path("non_existent_id_12345").is_none());
  }

  #[test]
  fn test_cached_audio_path_finds_valid_cache_file() {
    if let Some(dir) = crate::core::paths::audio_cache_dir() {
      let _ = std::fs::create_dir_all(&dir);
      let test_file = dir.join("test_dummy_id_abc.m4a");
      let data = vec![0u8; 2048]; // > 1024 bytes
      std::fs::write(&test_file, &data).unwrap();

      let found = cached_audio_path("test_dummy_id_abc");
      assert!(found.is_some());
      assert_eq!(found.unwrap(), test_file);

      let _ = std::fs::remove_file(test_file);
    }
  }

  #[tokio::test]
  async fn test_maybe_prebuffer_triggers_within_20s() {
    let source = Arc::new(YouTubeSource::new(None));
    let tmp = match tempfile::NamedTempFile::new() {
      Ok(t) => t,
      Err(_) => return,
    };
    let path = tmp.path().to_path_buf();
    let player = match LocalPlayer::new() {
      Ok(p) => Arc::new(p),
      Err(_) => return, // Skip on headless environments without audio device
    };

    let mut state = YouTubePlaybackState {
      player,
      source,
      tracks: vec![
        TrackInfo {
          generated_name: None,
          uri: Some("youtube:track1".into()),
          name: "Track 1".into(),
          artists: vec![],
          album: "".into(),
          duration_ms: 10_000,
          id: None,
          album_id: None,
          artist_refs: vec![],
          is_playable: true,
          is_local: false,
          track_number: 1,
          explicit: false,
          image_url: None,
          release_date: None,
        },
        TrackInfo {
          generated_name: None,
          uri: Some("youtube:track2".into()),
          name: "Track 2".into(),
          artists: vec![],
          album: "".into(),
          duration_ms: 60_000,
          id: None,
          album_id: None,
          artist_refs: vec![],
          is_playable: true,
          is_local: false,
          track_number: 2,
          explicit: false,
          image_url: None,
          release_date: None,
        },
      ],
      index: 0,
      advancing: false,
      tempfile: tmp,
      audio_path: path,
      shuffle_backup: None,
      resume_at: None,
      prebuffering_id: None,
      prebuffered_id: None,
    };

    state.maybe_prebuffer_next();
    assert_eq!(state.prebuffering_id.as_deref(), Some("track2"));
  }
}
