# Changelog

All notable changes to **EchoVena** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-10-24

### Added
- **Lossless Relay Waterfall:** Multi-tier bit-perfect streaming prioritizing Qobuz (up to 24-bit / 192 kHz FLAC) -> Tidal Hi-Res (`hifi-api` with single-in-flight `Semaphore(1)` concurrency and HTTP 202 queue polling) -> Tidal Lossy (16-bit / 44.1 kHz FLAC) -> YouTube Opus (`yt-dlp`).
- **Multi-Platform Playlist Sync:** Synchronize playlists from Spotify (via public HTML scraping or ChaCha20-Poly1305 encrypted `sp_dc` cookie sync) and YouTube / YouTube Music with regex title sanitization.
- **Track Resolution Pipeline:** Cross-platform matching engine utilizing ISRC identification, Songlink / Odesli API bridges, and Jaro-Winkler fuzzy text similarity with duration gate tolerances.
- **60 FPS Live Karaoke Stage View:** Smooth sub-word character interpolation rendering word-synchronized lyrics from Apple Music TTML (including duet roles and background vocals), Musixmatch RichSync, Spotify `color-lyrics`, LRCLIB `.elrc`, and NetEase `.yrc`.
- **Local Lossless Archiver:** Single track (`S`) and batch playlist (`Shift+S`) archiving into FLAC with embedded Vorbis comments and cover art via `lofty`, accompanied by `.elrc` enhanced lyric sidecars and `.m3u8` playlist files.
- **Dynamic Provider Registry:** Fully configurable relay endpoints loaded dynamically from `~/.config/echovena/providers.json` with automatic health scoring and cooldown tracking (`R` modal).
- **One-Liner Installers:** Windows PowerShell (`install.ps1`) and Linux/macOS shell (`install.sh`) zero-setup installers.
- **Package Manifests:** Windows Package Manager Winget (`Ranoes.EchoVena`), Chocolatey (`echovena`), and Scoop (`echovena.json`).
