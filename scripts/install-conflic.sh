#!/usr/bin/env bash
set -euo pipefail

# Install conflic binary for GitHub Actions
# Usage: CONFLIC_VERSION=latest RUNNER_OS=Linux RUNNER_ARCH=X64 ./install-conflic.sh

VERSION="${CONFLIC_VERSION:-latest}"
REPO="onplt/conflic"

# --- Platform detection ---
detect_target() {
  local os="${RUNNER_OS:-$(uname -s)}"
  local arch="${RUNNER_ARCH:-$(uname -m)}"

  case "$os" in
    Linux)
      case "$arch" in
        X64|x86_64)  echo "x86_64-unknown-linux-gnu" ;;
        ARM64|aarch64) echo "aarch64-unknown-linux-gnu" ;;
        *) echo "::error::Unsupported Linux architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    macOS)
      case "$arch" in
        X64|x86_64)  echo "x86_64-apple-darwin" ;;
        ARM64|aarch64|arm64) echo "aarch64-apple-darwin" ;;
        *) echo "::error::Unsupported macOS architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    Windows)
      case "$arch" in
        X64|x86_64) echo "x86_64-pc-windows-msvc" ;;
        *) echo "::error::Unsupported Windows architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    *)
      echo "::error::Unsupported OS: $os" >&2
      exit 1
      ;;
  esac
}

# --- Version resolution ---
resolve_version() {
  local ver="$1"
  if [ "$ver" = "latest" ]; then
    local api_url="https://api.github.com/repos/${REPO}/releases/latest"
    local tag
    tag=$(curl -fsSL -H "Accept: application/vnd.github+json" \
      ${GITHUB_TOKEN:+-H "Authorization: Bearer $GITHUB_TOKEN"} \
      "$api_url" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')
    if [ -z "$tag" ]; then
      echo "::error::Failed to resolve latest version from GitHub API" >&2
      exit 1
    fi
    # Strip leading 'v' if present
    echo "${tag#v}"
  else
    # Strip leading 'v' if user passed it
    echo "${ver#v}"
  fi
}

# --- Main ---
TARGET=$(detect_target)
VERSION=$(resolve_version "$VERSION")

echo "::group::Installing conflic v${VERSION} for ${TARGET}"

# Check cache
INSTALL_DIR="${RUNNER_TOOL_CACHE:-/tmp}/conflic/${VERSION}"
BINARY_NAME="conflic"
if [ "${RUNNER_OS:-}" = "Windows" ]; then
  BINARY_NAME="conflic.exe"
fi

if [ -x "${INSTALL_DIR}/${BINARY_NAME}" ]; then
  echo "conflic v${VERSION} found in cache"
  echo "${INSTALL_DIR}" >> "$GITHUB_PATH"
  echo "::endgroup::"
  exit 0
fi

mkdir -p "$INSTALL_DIR"

# Determine archive format and download URL
if [ "${RUNNER_OS:-$(uname -s)}" = "Windows" ]; then
  ARCHIVE="conflic-v${VERSION}-${TARGET}.zip"
else
  ARCHIVE="conflic-v${VERSION}-${TARGET}.tar.gz"
fi

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ARCHIVE}"
echo "Downloading ${DOWNLOAD_URL}"

TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

curl -fsSL -o "${TEMP_DIR}/${ARCHIVE}" "$DOWNLOAD_URL"

# Extract
if [[ "$ARCHIVE" == *.zip ]]; then
  unzip -q "${TEMP_DIR}/${ARCHIVE}" -d "$INSTALL_DIR"
else
  tar xzf "${TEMP_DIR}/${ARCHIVE}" -C "$INSTALL_DIR"
fi

chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

# Add to PATH
echo "${INSTALL_DIR}" >> "$GITHUB_PATH"
echo "Installed conflic v${VERSION} to ${INSTALL_DIR}"
echo "::endgroup::"
