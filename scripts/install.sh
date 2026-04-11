#!/bin/sh

set -eu

APP_NAME="ppdns"
REPO="${PPDNS_REPO:-0x3st/ppdns}"
VERSION="${PPDNS_VERSION:-latest}"
BIN_DIR="${PPDNS_BIN_DIR:-}"
DIRECT_URL="${PPDNS_DOWNLOAD_URL:-}"

usage() {
  cat <<'EOF'
Install ppdns from a prebuilt Linux release.

Usage:
  install.sh [--repo OWNER/REPO] [--version VERSION] [--bin-dir DIR]
  install.sh --url DIRECT_ARCHIVE_URL [--bin-dir DIR]

Options:
  --repo OWNER/REPO    GitHub repository that hosts Releases, default: 0x3st/ppdns
  --version VERSION    Release version without or with leading v, default: latest
  --bin-dir DIR        Install directory, default: /usr/local/bin or ~/.local/bin
  --url URL            Direct archive URL, bypass GitHub repo resolution
  -h, --help           Show help
EOF
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Error: required command not found: $1" >&2
    exit 1
  fi
}

while [ $# -gt 0 ]; do
  case "$1" in
    --repo)
      REPO="${2:?missing value for --repo}"
      shift 2
      ;;
    --version)
      VERSION="${2:?missing value for --version}"
      shift 2
      ;;
    --bin-dir)
      BIN_DIR="${2:?missing value for --bin-dir}"
      shift 2
      ;;
    --url)
      DIRECT_URL="${2:?missing value for --url}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" != "Linux" ]; then
  echo "Error: this installer currently supports Linux only" >&2
  exit 1
fi

case "$ARCH" in
  x86_64|amd64)
    TARGET="x86_64-unknown-linux-musl"
    ;;
  aarch64|arm64)
    TARGET="aarch64-unknown-linux-musl"
    ;;
  *)
    echo "Error: unsupported Linux architecture: $ARCH" >&2
    exit 1
    ;;
esac

ARCHIVE_NAME="${APP_NAME}-${TARGET}.tar.gz"

if [ -z "$DIRECT_URL" ]; then
  if [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ARCHIVE_NAME}"
  else
    case "$VERSION" in
      v*)
        TAG="$VERSION"
        ;;
      *)
        TAG="v$VERSION"
        ;;
    esac
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE_NAME}"
  fi
else
  DOWNLOAD_URL="$DIRECT_URL"
fi

if [ -z "$BIN_DIR" ]; then
  if [ -w "/usr/local/bin" ] || [ "$(id -u)" -eq 0 ]; then
    BIN_DIR="/usr/local/bin"
  else
    BIN_DIR="${HOME}/.local/bin"
  fi
fi

need_cmd tar
need_cmd mktemp

DOWNLOAD_TOOL=""
if command -v curl >/dev/null 2>&1; then
  DOWNLOAD_TOOL="curl"
elif command -v wget >/dev/null 2>&1; then
  DOWNLOAD_TOOL="wget"
else
  echo "Error: curl or wget is required" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

ARCHIVE_PATH="${TMP_DIR}/${ARCHIVE_NAME}"

echo "Downloading ${DOWNLOAD_URL}"
if [ "$DOWNLOAD_TOOL" = "curl" ]; then
  curl -fL "$DOWNLOAD_URL" -o "$ARCHIVE_PATH"
else
  wget -O "$ARCHIVE_PATH" "$DOWNLOAD_URL"
fi

mkdir -p "$BIN_DIR"
tar -xzf "$ARCHIVE_PATH" -C "$TMP_DIR"

BIN_PATH="$(find "$TMP_DIR" -type f -name "$APP_NAME" | head -n 1)"
if [ -z "$BIN_PATH" ]; then
  echo "Error: could not find ${APP_NAME} in downloaded archive" >&2
  exit 1
fi

if command -v install >/dev/null 2>&1; then
  install -m 0755 "$BIN_PATH" "${BIN_DIR}/${APP_NAME}"
else
  cp "$BIN_PATH" "${BIN_DIR}/${APP_NAME}"
  chmod 0755 "${BIN_DIR}/${APP_NAME}"
fi

echo "Installed ${APP_NAME} to ${BIN_DIR}/${APP_NAME}"
if [ "$BIN_DIR" = "${HOME}/.local/bin" ]; then
  echo "Make sure ${HOME}/.local/bin is in your PATH."
fi
