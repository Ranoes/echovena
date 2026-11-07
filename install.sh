#!/usr/bin/env sh
# EchoVena one-line installer for Linux and macOS (POSIX sh).
#
#   curl -fsSL https://raw.githubusercontent.com/Ranoes/echovena/main/install.sh | sh
#
# Environment overrides:
#   ECHOVENA_VERSION            install a specific tag (e.g. v1.1.0); default: latest
#   ECHOVENA_INSTALL_DIR        where to put the binary; default: $HOME/.local/bin
#   ECHOVENA_BUILD_FROM_SOURCE  set to 1 to compile via cargo instead of downloading a release
set -eu

REPO="Ranoes/echovena"
BINARY="echovena"
INSTALL_DIR="${ECHOVENA_INSTALL_DIR:-$HOME/.local/bin}"

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

printf '\n%s\n' "${CYAN}  ===============================================================${RESET}" >&2
printf '%s\n' "${CYAN}   EchoVena - Terminal Music Streamer, Sync & 60 FPS Karaoke TUI${RESET}" >&2
printf '%s\n' "${DIM}   https://github.com/Ranoes/echovena${RESET}" >&2
printf '%s\n\n' "${CYAN}  ===============================================================${RESET}" >&2

install_from_source() {
  if ! command -v cargo >/dev/null 2>&1; then
    error "cargo is not installed. Install Rust from https://rustup.rs and re-run."
  fi
  info "Building EchoVena from source: cargo install --git https://github.com/${REPO}.git --locked"
  cargo install --git "https://github.com/${REPO}.git" --locked
  ok "Compiled and installed EchoVena via cargo."
}

compute_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | awk '{print $NF}'
  else
    error "Need sha256sum, shasum, or openssl to verify download integrity."
  fi
}

if [ "${ECHOVENA_BUILD_FROM_SOURCE:-0}" = "1" ]; then
  install_from_source
else
  # --- download helper -------------------------------------------------------
  if command -v curl >/dev/null 2>&1; then
    dl() { curl -fsSL "$1" -o "$2"; }
  elif command -v wget >/dev/null 2>&1; then
    dl() { wget -qO "$2" "$1"; }
  else
    error "Need curl or wget to download EchoVena."
  fi

  # --- detect platform (only ship verified prebuilt targets) -----------------
  os="$(uname -s)"
  arch="$(uname -m)"
  platform_os=""
  asset=""

  case "${os}:${arch}" in
    Linux:x86_64|Linux:amd64)
      platform_os="linux"
      asset="${BINARY}-linux-x86_64.tar.gz"
      ;;
    Darwin:arm64|Darwin:aarch64)
      platform_os="macos"
      asset="${BINARY}-macos-aarch64.tar.gz"
      ;;
    *)
      warn "No prebuilt release binary is published for ${os}/${arch} (prebuilt targets: Linux x86_64, macOS Apple Silicon arm64)."
      printf '%s\n' "  To build and install from source instead, run:" >&2
      printf '%s\n' "    cargo install --git https://github.com/${REPO}.git --locked" >&2
      printf '%s\n' "  Or re-run this installer with ECHOVENA_BUILD_FROM_SOURCE=1" >&2
      exit 1
      ;;
  esac

  if [ -n "${ECHOVENA_VERSION:-}" ]; then
    tag="v${ECHOVENA_VERSION#v}"
    base_url="https://github.com/${REPO}/releases/download/${tag}"
    label="$tag"
  else
    base_url="https://github.com/${REPO}/releases/latest/download"
    label="latest"
  fi

  release_url="${base_url}/${asset}"
  sums_url="${base_url}/SHA256SUMS.txt"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT INT TERM

  info "Downloading ${BOLD}${asset}${RESET} (${label})..."
  if ! dl "$release_url" "${tmp}/${asset}"; then
    error "Failed to download ${release_url}. Check https://github.com/${REPO}/releases or set ECHOVENA_BUILD_FROM_SOURCE=1."
  fi

  info "Verifying SHA-256 checksum via SHA256SUMS.txt..."
  if ! dl "$sums_url" "${tmp}/SHA256SUMS.txt"; then
    error "Failed to download ${sums_url}. Aborting for safety."
  fi

  expected_sha="$(awk -v f="$asset" '$2 == f || $2 == "*"f { print tolower($1); exit }' "${tmp}/SHA256SUMS.txt")"
  if [ -z "$expected_sha" ]; then
    error "Asset '${asset}' was not found in SHA256SUMS.txt. Aborting."
  fi

  actual_sha="$(compute_sha256 "${tmp}/${asset}" | tr '[:upper:]' '[:lower:]')"
  if [ "$actual_sha" != "$expected_sha" ]; then
    error "SHA-256 mismatch for ${asset}! Expected ${expected_sha}, got ${actual_sha}."
  fi
  ok "SHA-256 verified (${actual_sha})"

  info "Extracting ${asset}..."
  tar -xzf "${tmp}/${asset}" -C "$tmp"

  [ -f "${tmp}/${BINARY}" ] || error "Archive did not contain '${BINARY}' binary."

  mkdir -p "$INSTALL_DIR"
  cp "${tmp}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
  chmod 0755 "${INSTALL_DIR}/${BINARY}"

  if [ "$platform_os" = "macos" ] && [ -f "${tmp}/libportaudio.2.dylib" ]; then
    cp "${tmp}/libportaudio.2.dylib" "${INSTALL_DIR}/libportaudio.2.dylib"
    chmod 0755 "${INSTALL_DIR}/libportaudio.2.dylib"
    if command -v xattr >/dev/null 2>&1; then
      xattr -dr com.apple.quarantine "${INSTALL_DIR}/${BINARY}" "${INSTALL_DIR}/libportaudio.2.dylib" 2>/dev/null || true
    fi
  elif [ "$platform_os" = "macos" ] && command -v xattr >/dev/null 2>&1; then
    xattr -dr com.apple.quarantine "${INSTALL_DIR}/${BINARY}" 2>/dev/null || true
  fi

  ver_out="$("${INSTALL_DIR}/${BINARY}" --version)" || error "Installed binary failed '--version' smoke check."
  ok "Installed ${ver_out} to ${BOLD}${INSTALL_DIR}/${BINARY}${RESET}"

  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*)
      ok "${INSTALL_DIR} is in your PATH."
      ;;
    *)
      warn "${INSTALL_DIR} is NOT currently in your PATH."
      printf '%s\n' "  Add this line to your shell profile (~/.bashrc, ~/.zshrc, or ~/.profile):" >&2
      printf '%s\n' "    export PATH=\"${INSTALL_DIR}:\$PATH\"" >&2
      ;;
  esac
fi

if ! command -v yt-dlp >/dev/null 2>&1; then
  info "Tip: for optional YouTube fallback stream extraction, install yt-dlp (e.g. 'pipx install yt-dlp' or https://github.com/yt-dlp/yt-dlp/releases)."
fi

printf '\n%s\n' "${GREEN}Done.${RESET} Run ${BOLD}${BINARY}${RESET} to launch EchoVena." >&2
