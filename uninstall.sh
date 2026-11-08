#!/usr/bin/env sh
# EchoVena uninstaller for Linux and macOS (POSIX sh).
#
#   curl -fsSL https://raw.githubusercontent.com/Ranoes/echovena/main/uninstall.sh | sh
#
# Flags & Environment overrides:
#   --purge-data  / ECHOVENA_PURGE_DATA=1   remove ~/.config/echovena, ~/.local/share/echovena, etc.
#   --purge-music / ECHOVENA_PURGE_MUSIC=1  remove ~/Music/EchoVena
#   --install-dir / ECHOVENA_INSTALL_DIR    custom install dir; default: $HOME/.local/bin
#   -y / --yes    / ECHOVENA_FORCE=1        skip confirmation
set -eu

BINARY="echovena"
INSTALL_DIR="${ECHOVENA_INSTALL_DIR:-$HOME/.local/bin}"
PURGE_DATA="${ECHOVENA_PURGE_DATA:-0}"
PURGE_MUSIC="${ECHOVENA_PURGE_MUSIC:-0}"
FORCE="${ECHOVENA_FORCE:-0}"

# --- visual formatting -------------------------------------------------------
if [ -t 2 ] && [ -z "${NO_COLOR:-}" ]; then
  BOLD="$(printf '\033[1m')"
  DIM="$(printf '\033[2m')"
  RED="$(printf '\033[31m')"
  GREEN="$(printf '\033[32m')"
  YELLOW="$(printf '\033[33m')"
  CYAN="$(printf '\033[36m')"
  RESET="$(printf '\033[0m')"
else
  BOLD=""
  DIM=""
  RED=""
  GREEN=""
  YELLOW=""
  CYAN=""
  RESET=""
fi

info()  { printf '%s\n' "${DIM}[*]${RESET} $*" >&2; }
ok()    { printf '%s\n' "${GREEN}[OK]${RESET} $*" >&2; }
warn()  { printf '%s\n' "${YELLOW}[!]${RESET} $*" >&2; }
error() { printf '%s\n' "${RED}[X]${RESET} $*" >&2; exit 1; }

# Parse CLI arguments
while [ $# -gt 0 ]; do
  case "$1" in
    --purge-data)
      PURGE_DATA=1
      shift
      ;;
    --purge-music)
      PURGE_MUSIC=1
      shift
      ;;
    --install-dir)
      INSTALL_DIR="$2"
      shift 2
      ;;
    -y|--yes|--force)
      FORCE=1
      shift
      ;;
    -h|--help)
      echo "EchoVena uninstaller"
      echo "Usage: ./uninstall.sh [OPTIONS]"
      echo ""
      echo "Options:"
      echo "  --purge-data    Remove config, SQLite database, state & caches"
      echo "  --purge-music   Remove offline music archive (~/Music/EchoVena)"
      echo "  --install-dir   Custom install directory (default: \$HOME/.local/bin)"
      echo "  -y, --yes       Skip confirmation prompt"
      echo "  -h, --help      Show this help message"
      exit 0
      ;;
    *)
      shift
      ;;
  esac
done

printf '\n%s\n' "${CYAN}  ===============================================================${RESET}" >&2
printf '%s\n' "${CYAN}   EchoVena Uninstaller - Linux & macOS${RESET}" >&2
printf '%s\n' "${DIM}   https://github.com/Ranoes/echovena${RESET}" >&2
printf '%s\n\n' "${CYAN}  ===============================================================${RESET}" >&2

# 1. Terminate any active running instances to release locks
if command -v pkill >/dev/null 2>&1; then
  pkill -x "${BINARY}" 2>/dev/null || true
fi

# 2. Remove binary from install directory
target_exe="${INSTALL_DIR}/${BINARY}"
if [ -f "$target_exe" ]; then
  info "Removing ${target_exe}..."
  rm -f "$target_exe"
  ok "Removed executable."
else
  info "Executable not found at ${target_exe}."
fi

# 3. Remove bundled PortAudio library on macOS if present
target_dylib="${INSTALL_DIR}/libportaudio.2.dylib"
if [ -f "$target_dylib" ]; then
  info "Removing bundled ${target_dylib}..."
  rm -f "$target_dylib"
  ok "Removed bundled audio library."
fi

# 4. Check for Cargo-installed binary (~/.cargo/bin/echovena)
cargo_bin="$HOME/.cargo/bin/${BINARY}"
if [ -f "$cargo_bin" ]; then
  info "Found Cargo-installed binary at ${cargo_bin}."
  if command -v cargo >/dev/null 2>&1; then
    info "Running 'cargo uninstall ${BINARY}'..."
    cargo uninstall "${BINARY}" 2>/dev/null || rm -f "$cargo_bin"
  else
    rm -f "$cargo_bin"
  fi
  ok "Removed Cargo binary."
fi

# 5. Purge application data, database and caches (if requested)
if [ "${PURGE_DATA}" = "1" ]; then
  info "Purging EchoVena configurations, state, database & caches..."
  rm -rf "$HOME/.config/${BINARY}"
  rm -rf "$HOME/.local/share/${BINARY}"
  rm -rf "$HOME/.local/state/${BINARY}"
  rm -rf "$HOME/.cache/${BINARY}"
  ok "Purged user configuration and caches."
else
  info "Preserved user data and configs (~/.config/echovena, ~/.local/share/echovena)."
  printf '%s\n' "  (To completely remove user data, re-run with: --purge-data or ECHOVENA_PURGE_DATA=1)" >&2
fi

# 6. Purge offline music archive (if requested)
music_archive="$HOME/Music/EchoVena"
if [ "${PURGE_MUSIC}" = "1" ]; then
  if [ -d "$music_archive" ]; then
    info "Purging offline music archive (${music_archive})..."
    rm -rf "$music_archive"
    ok "Deleted: ${music_archive}"
  fi
else
  if [ -d "$music_archive" ]; then
    info "Preserved offline music archive (${music_archive})."
  fi
fi

printf '\n%s\n' "${GREEN}EchoVena uninstallation complete.${RESET}" >&2

