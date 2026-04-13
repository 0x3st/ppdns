#!/usr/bin/env bash

set -euo pipefail

DB_PATH="/var/lib/powerdns/pdns.sqlite3"
SCHEMA_PATH="/usr/share/doc/pdns-backend-sqlite3/schema.sqlite3.sql"

mkdir -p /var/lib/powerdns

if [[ ! -f "${DB_PATH}" ]]; then
  sqlite3 "${DB_PATH}" < "${SCHEMA_PATH}"
fi

chown -R pdns:pdns /var/lib/powerdns

exec pdns_server \
  --config-dir=/etc/powerdns \
  --daemon=no \
  --guardian=no \
  --disable-syslog
