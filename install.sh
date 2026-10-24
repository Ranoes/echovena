#!/usr/bin/env sh
# EchoVena installer for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/Ranoes/echovena/main/install.sh | sh
#
# Environment overrides:
#   ECHOVENA_VERSION      install a specific tag (e.g. v1.0.0); default: latest
#   ECHOVENA_INSTALL_DIR  where to put the binary; default: $HOME/.local/bin
set -eu

REPO="Ranoes/echovena"
BINARY="echovena"
INSTALL_DIR="${ECHOVENA_INSTALL_DIR:-$HOME/.local/bin}"

# --- visual formatting -----------------------------------------------------
if [ -t 2 ] && [ -z "${NO_COLOR:-}" ]; then
  BOLD="$(printf '\033[1m')"; DIM="$(printf '\033[2m')"; RED="$(printf '\033[31m')"
  GREEN="$(printf '\033[32m')"; YELLOW="$(printf '\033[33m')"; CYAN="$(printf '\033[36m')"; RESET="$(printf '\033[0m')"
else
  BOLD=""; DIM=""; RED=""; GREEN=""; YELLOW=""; CYAN=""; RESET=""
fi

info()  { printf '%s\n' "${DIM}·${RESET} $*" >&2; }
ok()    { printf '%s\n' "${GREEN}✓${RESET} $*" >&2; }
warn()  { printf '%s\n' "${YELLOW}!${RESET} $*" >&2; }
error() { printf '%s\n' "${RED}✗${RESET} $*" >&2; exit 1; }

printf '%s\n' "${CYAN}"
printf '%s\n' "  ███████╗ ██████╗██╗  ██╗ ██████╗ ██╗   ██╗███████╗███╗   ██╗ █████╗ "
printf '%s\n' "  ██╔════╝██╔════╝██║  ██║██╔═══██╗██║   ██║██╔════╝████╗  ██║██╔══██╗"
printf '%s\n' "  █████╗  ██║     ███████║██║   ██║██║   ██║█████╗  ██╔██╗ ██║███████║"
printf '%s\n' "  ██╔══╝  ██║     ██╔══██║██║   ██║╚██╗ ██╔╝██╔══╝  ██║╚██╗██║██╔══██║"
printf '%s\n' "  ███████╗╚██████╗██║  ██║╚██████╔╝ ╚████╔╝ ███████╗██║ ╚████║██║  ██║"
printf '%s\n' "  ╚══════╝ ╚═════╝╚═╝  ╚═╝ ╚═════╝   ╚═══╝  ╚══════╝╚═╝  ╚═══╝╚═╝  ╚═╝"
printf '%s\n' "      Lossless Hi-Res FLAC Streamer & Live Karaoke TUI (Ranoes/echovena)"
printf '%s\n' "${RESET}"

# --- download helper -------------------------------------------------------
if command -v curl >/dev/null 2>&1; then
  dl() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  dl() { wget -qO "$2" "$1"; }
else
  error "need curl or wget to download EchoVena"
fi

# --- detect platform -------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux)  platform_os="linux" ;;
  Darwin) platform_os="macos" ;;
  *)      error "unsupported OS '$os'. Try building from source: cargo install --git https://github.com/${REPO}.git --locked" ;;
esac

case "$arch" in
  x86_64|amd64)
    platform_arch="x86_64"
    ;;
  aarch64|arm64)
    if [ "$platform_os" = "macos" ]; then
      platform_arch="aarch64"
    else
      platform_arch="aarch64"
    fi
    ;;
  *)
    error "unsupported architecture '$arch'. Try building from source: cargo install --git https://github.com/${REPO}.git --locked"
    ;;
esac

asset="${BINARY}-${platform_os}-${platform_arch}.tar.gz"

# --- check GitHub Releases API or fallback --------------------------------
if [ -n "${ECHOVENA_VERSION:-}" ]; then
  tag="${ECHOVENA_VERSION#v}"
  tag="v${tag}"
  release_url="https://github.com/${REPO}/releases/download/${tag}/${asset}"
  label="$tag"
else
  release_url="https://github.com/${REPO}/releases/latest/download/${asset}"
  label="latest"
fi

info "attempting download of ${BOLD}${asset}${RESET} (${label})..."

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

download_ok=false
if dl "$release_url" "${tmp}/${asset}" 2>/dev/null; then
  download_ok=true
fi

if [ "$download_ok" = "true" ]; then
  info "extracting ${asset}..."
  tar -xzf "${tmp}/${asset}" -C "$tmp"

  if [ ! -f "${tmp}/${BINARY}" ]; then
    found_bin="$(find "$tmp" -name "$BINARY" -type f | head -n 1 || true)"
    if [ -n "$found_bin" ]; then
      cp "$found_bin" "${tmp}/${BINARY}"
    fi
  fi

  [ -f "${tmp}/${BINARY}" ] || error "archive did not contain '${BINARY}' binary"

  mkdir -p "$INSTALL_DIR"
  cp "${tmp}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
  chmod 0755 "${INSTALL_DIR}/${BINARY}"
  ok "installed to ${BOLD}${INSTALL_DIR}/${BINARY}${RESET}"
else
  warn "prebuilt binary ${asset} not available on GitHub Releases yet."
  if command -v cargo >/dev/null 2>&1; then
    info "cargo detected. Falling back to source build: cargo install --git https://github.com/${REPO}.git --locked"
    cargo install --git "https://github.com/${REPO}.git" --locked
    ok "successfully compiled and installed EchoVena from source!"
  else
    error "could not find prebuilt binary and cargo is not installed. Please install Rust (https://rustup.rs) or download a release from https://github.com/${REPO}/releases"
  fi
fi

# --- check PATH ------------------------------------------------------------
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*)
    ok "${INSTALL_DIR} is in your PATH."
    ;;
  *)
    warn "${INSTALL_DIR} is NOT currently in your PATH."
    printf '%s\n' "  To run echovena directly, add it to your shell configuration:" >&2
    printf '%s\n' "    export PATH=\"${INSTALL_DIR}:\$PATH\"" >&2
    ;;
esac

printf '\n%s\n' "${GREEN}Done.${RESET} Run ${BOLD}${BINARY}${RESET} to get started." >&2
