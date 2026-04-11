#!/usr/bin/env bash

set -euo pipefail

APP_NAME="ppdns"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${ROOT_DIR}/dist"

TARGET="${1:-}"
if [[ -z "${TARGET}" ]]; then
  TARGET="$(rustc -vV | awk '/^host: / { print $2 }')"
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${ROOT_DIR}/Cargo.toml" | head -n 1)"
if [[ -z "${VERSION}" ]]; then
  echo "Could not determine package version from Cargo.toml" >&2
  exit 1
fi

ARCHIVE_BASENAME="${APP_NAME}-${TARGET}"
ARCHIVE_PATH="${DIST_DIR}/${ARCHIVE_BASENAME}.tar.gz"
CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"
STAGE_DIR="${DIST_DIR}/${APP_NAME}-${VERSION}-${TARGET}"
BINARY_PATH="${ROOT_DIR}/target/${TARGET}/release/${APP_NAME}"

mkdir -p "${DIST_DIR}"
rm -rf "${STAGE_DIR}"

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  echo "Building ${APP_NAME} ${VERSION} for ${TARGET}"
  cargo build --release --target "${TARGET}" --manifest-path "${ROOT_DIR}/Cargo.toml"
fi

if [[ ! -f "${BINARY_PATH}" ]]; then
  echo "Expected binary not found: ${BINARY_PATH}" >&2
  exit 1
fi

mkdir -p "${STAGE_DIR}"
cp "${BINARY_PATH}" "${STAGE_DIR}/${APP_NAME}"
cp "${ROOT_DIR}/README.md" "${STAGE_DIR}/README.md"

tar -C "${DIST_DIR}" -czf "${ARCHIVE_PATH}" "$(basename "${STAGE_DIR}")"
rm -f "${CHECKSUM_PATH}"

if command -v sha256sum >/dev/null 2>&1; then
  (
    cd "${DIST_DIR}"
    sha256sum "$(basename "${ARCHIVE_PATH}")" > "$(basename "${CHECKSUM_PATH}")"
  )
elif command -v shasum >/dev/null 2>&1; then
  (
    cd "${DIST_DIR}"
    shasum -a 256 "$(basename "${ARCHIVE_PATH}")" > "$(basename "${CHECKSUM_PATH}")"
  )
else
  echo "Warning: sha256sum/shasum not found, skipping checksum generation" >&2
fi

echo "Created ${ARCHIVE_PATH}"
if [[ -f "${CHECKSUM_PATH}" ]]; then
  echo "Created ${CHECKSUM_PATH}"
fi
