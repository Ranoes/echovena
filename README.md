# EchoVena (`echovena`)

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.90%2B-orange.svg)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/Version-1.1.0-emerald.svg)](Cargo.toml)

> **Lossless Hi-Res FLAC Terminal Music Streamer, Multi-Platform Playlist Sync & 60 FPS Real-Time Word-by-Word Karaoke TUI**

EchoVena is an ultra-fast, standalone terminal music player and streaming suite written in Rust. It decouples track discovery and playlist management (Spotify & YouTube) from audio delivery, streaming bit-perfect lossless audio up to 24-bit / 192 kHz FLAC through a multi-provider waterfall queue while rendering live, word-synchronized karaoke lyrics at 60 FPS.

---

## Quick Install (One-Liner)

### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/Ranoes/echovena/main/install.ps1 | iex
```

### Windows (Package Managers)
```powershell
# Winget
winget install Ranoes.EchoVena

# Chocolatey
choco install echovena
```

### Linux & macOS (No Homebrew Required)
```sh
curl -fsSL https://raw.githubusercontent.com/Ranoes/echovena/main/install.sh | sh
```
> Prebuilt release archives are provided for **Windows `x86_64`**, **Linux `x86_64`**, and **macOS Apple Silicon (`arm64`)** with SHA-256 verification and bundled audio libraries on macOS. Other architectures can build from source via `cargo install` below.

### Rust (Cargo)
```bash
cargo install --git https://github.com/Ranoes/echovena.git --locked
```

---

## Key Features

- **Bit-Perfect Lossless Relay Waterfall:** Streams studio-quality audio through an automated priority waterfall: **Qobuz (24-bit / 192 kHz)** $\to$ **Tidal Hi-Res (`hifi-api`)** $\to$ **Tidal Lossy (16-bit / 44.1 kHz)** $\to$ **YouTube Opus (`yt-dlp`)**. All `hifi-api` requests enforce a global single-in-flight `Semaphore(1)` lock and handle HTTP 202 queue polling.
- **Multi-Platform Playlist Sync Engine:** Import and sync playlists from Spotify (via public HTML scraping or ChaCha20-Poly1305 encrypted `sp_dc` cookie sync) and YouTube / YouTube Music with smart regex title sanitation.
- **High-Precision Track Resolution:** Multi-stage resolution pipeline combining international standard recording codes (ISRC), Songlink / Odesli universal bridges, and Jaro-Winkler fuzzy text similarity with duration validation gates.
- **60 FPS Live Karaoke Stage View:** Smooth sub-word character interpolation rendering word-by-word synced lyrics from Apple Music TTML (including duet roles and background vocals), Musixmatch RichSync, Spotify `color-lyrics`, LRCLIB `.elrc`, and NetEase `.yrc`.
- **Local Lossless Archiver:** Save highlighted songs (`s`), currently playing tracks (`S`), or entire playlists/albums (`z`) as audio files with embedded metadata and cover art via `lofty`, accompanied by `.elrc` enhanced lyric sidecars and `.m3u8` playlist files in `~/Music/EchoVena`.
- **FFT Audio Spectrum Visualizer:** Real-time CAVA audio visualizer and responsive playbar telemetry displaying live sample rates, bit depths, and active streaming codecs.
- **Dynamic Provider Registry:** Fully configurable relay endpoints loaded dynamically from `~/.config/echovena/providers.json` with automatic health scoring and cooldown management.

---

## Keyboard Shortcuts

| Key | Description |
|---|---|
| `/` | **Live Search & Autocomplete:** Real-time YouTube / YTMusic search with `Tab` scope filtering (`Music`, `Album`, `Artist`, `Video`) |
| `h` / `j` / `k` / `l` | **Vim Navigation:** Move across panels, lists, columns, and playlists (`H` / `M` / `L` jump top/mid/bottom) |
| `p` / `Ctrl+V` | **Safe Clipboard Paste:** Paste URLs, tokens, or search queries inside input modals |
| `U` | **URL Import Modal:** Paste Spotify or YouTube playlist / track URLs to resolve and import |
| `T` | **Now Playing Dashboard:** Open the 60 FPS word-by-word Karaoke & Cover Art dashboard |
| `B` | **Fullscreen Lyrics:** Toggle dedicated synchronized lyrics view (`[` / `]` adjust `-100ms` / `+100ms` offset) |
| `Q` | **Play Queue:** Open two-section active & upcoming playback queue |
| `e` / `E` | **Queue Track:** `e` queues highlighted track next (Play Next); `E` adds to end of queue |
| `s` | **Save Highlighted Song:** Archive highlighted song offline with cover art and `.elrc` lyrics |
| `S` (`Shift+S`) | **Save Playing Track:** Archive currently playing track offline with cover art and `.elrc` lyrics |
| `z` | **Download Playlist / Album:** Batch archive full playlist or album offline with `.elrc` and `.m3u8` |
| `Space` | Play / Pause playback |
| `>` / `<` | Next / Previous track |
| `.` / `,` | Seek forward / backward |
| `+` / `-` | Increase / Decrease volume |
| `r` / `R` | Cycle repeat mode / Toggle shuffle |
| `d` | Open Audio Stream & Download Source Picker |
| `?` | Open context-aware Keybindings Help modal |
| `q` / `Ctrl+Q` | `q` Back / Close modal; `Ctrl+Q` Quit EchoVena |

---

## Configuration & Provider Registry

EchoVena stores user configurations, cached metadata, and audio providers in standard XDG locations:

- **Providers Configuration:** `~/.config/echovena/providers.json`
- **Application Config:** `~/.config/echovena/config.yml`
- **Persistent State & Session:** `~/.local/state/echovena/session.json`
- **SQLite Database:** `~/.local/share/echovena/library.db`
- **FLAC Ring Buffer:** `~/.cache/echovena/streaming_cache/`
- **Music Archive:** `~/Music/EchoVena/`

### Example `~/.config/echovena/providers.json`
```json
{
  "version": 1,
  "audio_relays": [
    { "name": "Qobuz Primary", "endpoint": "https://qobuz-api.example.com", "priority": 1, "enabled": true },
    { "name": "Tidal Hi-Res", "endpoint": "https://hifi-api.example.com", "priority": 2, "enabled": true },
    { "name": "YouTube yt-dlp", "endpoint": "local", "priority": 3, "enabled": true }
  ],
  "lyric_relays": [
    { "name": "Paxsenix TTML", "endpoint": "https://lyrics.paxsenix.org", "priority": 1, "enabled": true },
    { "name": "LRCLIB", "endpoint": "https://lrclib.net", "priority": 2, "enabled": true }
  ]
}
```

---

## Building from Source

EchoVena requires Rust 1.90+ with Cargo:

```bash
# Clone the repository
git clone https://github.com/Ranoes/echovena.git
cd echovena

# Build and run
cargo run --release
```

---

## Credits & Acknowledgments

EchoVena is built upon an incredible ecosystem of open-source projects, tools, and libraries. Sincere gratitude to all original authors and contributors:

- **Base TUI Architecture & Audio Visualizer:** [`LargeModGames/spotatui`](https://github.com/LargeModGames/spotatui) and the original [`Rigellute/spotify-tui`](https://github.com/Rigellute/spotify-tui) by Alexander Keliris for the foundational Rust terminal user interface, `rodio`/`cpal` playback foundation, and FFT spectrum visualizer.
- **Lossless Relay Waterfall & Cookie Sync Inspiration:** [`rawnaldclark/Stash`](https://github.com/rawnaldclark/Stash) for pioneering the personal Spotify sync, multi-provider lossless FLAC relay waterfall, and live karaoke lyric architecture.
- **Accountless Catalog Resolution & Provider Architecture:** [`BartolomeoRusso9/SpotiFLAC-Module-Version`](https://github.com/BartolomeoRusso9/SpotiFLAC-Module-Version) and [`afkarxyz/SpotiFLAC`](https://github.com/afkarxyz/SpotiFLAC) for inspiring the accountless Spotify metadata scraper, ISRC resolution, and modular provider patterns.
- **Lossless Audio Relay Bridges:** [`binimum/hifi-api`](https://github.com/binimum/hifi-api) and [`sachinsenal0x64/hifi`](https://github.com/sachinsenal0x64/hifi) for the single-in-flight lossless manifest queue architecture, alongside `qobuz-rest-api` and OpenSubsonic.
- **Karaoke & Synced Lyrics Ecosystem:** [Paxsenix Lyrics API](https://lyrics.paxsenix.org) (Apple Music TTML & Musixmatch RichSync word-by-word sync), [LRCLIB](https://lrclib.net) (`.elrc` & `.lrc` open lyrics database), [`akashrchandran/spotify-lyrics-api`](https://github.com/akashrchandran/spotify-lyrics-api), and NetEase Cloud Music `.yrc`.
- **Cross-Platform Track Matching & Extraction:** [Songlink / Odesli API](https://odesli.co/) for cross-platform Spotify/YouTube/Tidal/Qobuz matching, and [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) for YouTube playlist & Opus stream fallback.
- **Core Rust Ecosystem:** [`ratatui`](https://ratatui.rs/), [`symphonia`](https://github.com/pdeljanov/Symphonia), [`rodio`](https://github.com/RustAudio/rodio), [`cpal`](https://github.com/RustAudio/cpal), [`lofty`](https://github.com/Serial-ATA/lofty-rs), [`rusqlite`](https://github.com/rusqlite/rusqlite), and [`tokio`](https://tokio.rs/).

---

## License

EchoVena is released under the [MIT License](LICENSE).
