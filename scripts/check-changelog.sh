#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-}"

if [[ -z "${VERSION}" ]]; then
  VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${ROOT_DIR}/Cargo.toml" | head -n 1)"
fi

CHANGELOG_PATH="${ROOT_DIR}/CHANGELOG.md"

if [[ ! -f "${CHANGELOG_PATH}" ]]; then
  echo "CHANGELOG.md not found" >&2
  exit 1
fi

if ! grep -Eq "^## \[${VERSION//./\\.}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$" "${CHANGELOG_PATH}"; then
  echo "CHANGELOG.md is missing a release entry for version ${VERSION}" >&2
  echo "Expected a line like: ## [${VERSION}] - YYYY-MM-DD" >&2
  exit 1
fi

echo "CHANGELOG entry found for version ${VERSION}"
