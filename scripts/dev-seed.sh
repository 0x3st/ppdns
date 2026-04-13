#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PDNSUTIL="${ROOT_DIR}/scripts/pdnsutil-docker.sh"
ZONE="${1:-example.test.}"
PRIMARY_NS="${2:-ns1}"

normalize_zone() {
  local value="${1%.}"
  printf '%s.\n' "${value}"
}

zone_exists() {
  "${PDNSUTIL}" list-all-zones | grep -Fxq "${1}"
}

ZONE="$(normalize_zone "${ZONE}")"
ZONE_BARE="${ZONE%.}"

if [[ "${PRIMARY_NS}" != *.* ]]; then
  PRIMARY_NS="${PRIMARY_NS}.${ZONE_BARE}."
else
  PRIMARY_NS="$(normalize_zone "${PRIMARY_NS}")"
fi

if ! zone_exists "${ZONE}"; then
  "${PDNSUTIL}" create-zone "${ZONE}" "${PRIMARY_NS}"
fi

"${PDNSUTIL}" add-record "${ZONE}" @ A 300 203.0.113.10 || true
"${PDNSUTIL}" add-record "${ZONE}" www A 300 203.0.113.11 || true
"${PDNSUTIL}" add-record "${ZONE}" www A 300 203.0.113.12 || true
"${PDNSUTIL}" add-record "${ZONE}" txt TXT 300 '"hello world"' || true
"${PDNSUTIL}" add-record "${ZONE}" 20260408._domainkey CNAME 300 selector.example.net. || true
"${PDNSUTIL}" increase-serial "${ZONE}" || true

echo "Seeded ${ZONE}"
