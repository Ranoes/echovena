# Contributing to EchoVena

Thanks for your interest in EchoVena! You do not need to write code to help: bug reports, docs, themes, packaging, and testing on your setup all count.

- [Ways to help](#ways-to-help)
- [Report a bug](#report-a-bug)
- [Development Setup](#development-setup)
- [Where things live](#where-things-live)
- [Running EchoVena](#running-echovena)
- [Before opening a PR](#before-opening-a-pr)

## Ways to help

- **Report a bug** or suggest an enhancement on [GitHub Issues](https://github.com/Ranoes/echovena/issues).
- **Improve documentation.** Docs live in [`docs/`](docs/) and [`README.md`](README.md).
- **Add a theme preset.** See `ThemePreset` in `src/core/theme.rs` and [`docs/themes.md`](docs/themes.md).
- **Test on your setup.** Test across Windows, Linux, and macOS audio environments.
- **Package EchoVena.** Maintain or improve distribution packages (Winget, Chocolatey, Scoop, AUR, Homebrew).

Be kind, and follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Report a bug

Run EchoVena with `--debug`, reproduce the behavior, and attach the log:

```bash
echovena --debug
```

EchoVena prints the log path when it starts and when it exits (`~/.local/state/echovena/echovena_panic.log` on panic). Skim the log before posting: tokens and keys are never written, but a log can contain your display name.

Security reports go to the [Security Policy](SECURITY.md), not to a public issue.

## Development Setup

1. Install Rust (1.90+ required) via [rustup](https://rustup.rs).
2. For the YouTube Opus fallback stream, install [yt-dlp](https://github.com/yt-dlp/yt-dlp).
3. Fork the repository, clone your fork, and branch from `main`:
   ```bash
   git clone https://github.com/<your-username>/echovena.git
   cd echovena
   git checkout -b feature/my-feature
   ```

## Where things live

| Path | What it is |
|---|---|
| `src/core/` | Application state, tick scheduler, shared `Action` vocabulary, config and session files |
| `src/sources/lossless_relay/` | Lossless relay waterfall (Qobuz, Tidal `hifi-api`, yt-dlp), playlist scrapers/importers, ISRC resolver, FLAC ring buffer, archiver |
| `src/lyrics/` | Live karaoke lyrics engine (Apple Music TTML, Musixmatch RichSync, Spotify color-lyrics, LRCLIB .elrc, NetEase .yrc) |
| `src/ui/` | 60 FPS Karaoke stage renderer, relay status modal, URL import modal |
| `src/tui/` | Terminal UI event loop, key handlers, and widgets |
| `src/infra/` | Audio backends (`cpal`/`rodio`), FFT visualizer (`cavacore`), Spotify/YouTube/Qobuz/Subsonic clients |
| `src/cli/` | Command-line interface definitions and subcommands |
| `packaging/` | Windows Winget, Chocolatey, and Scoop distribution manifests |

## Running EchoVena

```bash
# Debug run
cargo run

# Optimized release run
cargo run --release
```

## Before opening a PR

Ensure the test suite and compiler checks pass cleanly:

```bash
cargo check
cargo test
```
