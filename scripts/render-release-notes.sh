#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-}"
OUTPUT_PATH="${2:-}"

if [[ -z "${VERSION}" ]]; then
  echo "Usage: render-release-notes.sh VERSION [OUTPUT_PATH]" >&2
  exit 1
fi

CHANGELOG_PATH="${ROOT_DIR}/CHANGELOG.md"

if [[ ! -f "${CHANGELOG_PATH}" ]]; then
  echo "CHANGELOG.md not found" >&2
  exit 1
fi

extract_section() {
  awk -v version="${VERSION}" '
    BEGIN {
      in_section = 0
      found = 0
    }
    $0 ~ "^## \\[" version "\\] - " {
      in_section = 1
      found = 1
    }
    in_section {
      if (found && NR > 1 && $0 ~ "^## \\[[^]]+\\] - " && $0 !~ "^## \\[" version "\\] - ") {
        exit
      }
      print
    }
  ' "${CHANGELOG_PATH}"
}

SECTION_CONTENT="$(extract_section)"

if [[ -z "${SECTION_CONTENT}" ]]; then
  echo "Could not extract changelog section for version ${VERSION}" >&2
  exit 1
fi

if [[ -n "${OUTPUT_PATH}" ]]; then
  printf '%s\n' "${SECTION_CONTENT}" > "${OUTPUT_PATH}"
else
  printf '%s\n' "${SECTION_CONTENT}"
fi
