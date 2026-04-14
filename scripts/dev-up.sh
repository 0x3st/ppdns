#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${ROOT_DIR}/docker-compose.yml"
SERVICE="${PPDNS_DOCKER_SERVICE:-pdns48}"

docker compose -f "${COMPOSE_FILE}" up -d --build "${SERVICE}"

for _ in $(seq 1 60); do
  if docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" pdnsutil list-all-zones >/dev/null 2>&1; then
    echo "PowerDNS sandbox is ready."
    echo "pdnsutil wrapper: ${ROOT_DIR}/scripts/pdnsutil-docker.sh"
    exit 0
  fi
  sleep 1
done

echo "PowerDNS sandbox did not become ready in time." >&2
exit 1
