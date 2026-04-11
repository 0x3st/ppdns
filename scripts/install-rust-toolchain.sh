#!/usr/bin/env bash

set -euo pipefail

TOOLCHAIN="${1:-stable}"
shift || true

TARGETS=()
while [[ $# -gt 0 ]]; do
  TARGETS+=("$1")
  shift
done

IFS=', ' read -r -a COMPONENTS <<< "${RUST_COMPONENTS:-}"
MAX_ATTEMPTS="${RUST_INSTALL_MAX_ATTEMPTS:-5}"
INITIAL_DELAY_SECONDS="${RUST_INSTALL_INITIAL_DELAY_SECONDS:-2}"

retry() {
  local attempt=1
  local delay="${INITIAL_DELAY_SECONDS}"
  local exit_code=0

  while true; do
    exit_code=0
    "$@" || exit_code=$?
    if (( exit_code == 0 )); then
      return 0
    fi

    if (( attempt >= MAX_ATTEMPTS )); then
      echo "Command failed after ${attempt} attempts: $*" >&2
      return "${exit_code}"
    fi

    echo "Command failed with exit code ${exit_code}; retrying in ${delay}s (${attempt}/${MAX_ATTEMPTS}): $*" >&2
    sleep "${delay}"
    attempt=$((attempt + 1))
    delay=$((delay * 2))
  done
}

install_rustup() {
  curl --proto '=https' --tlsv1.2 --retry 10 --retry-connrefused --location --silent --show-error --fail https://sh.rustup.rs | sh -s -- --default-toolchain none -y
}

if ! command -v rustup >/dev/null 2>&1; then
  retry install_rustup
fi

if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

retry rustup toolchain install "${TOOLCHAIN}" --profile minimal --no-self-update
rustup default "${TOOLCHAIN}"

for component in "${COMPONENTS[@]}"; do
  if [[ -n "${component}" ]]; then
    retry rustup component add "${component}" --toolchain "${TOOLCHAIN}"
  fi
done

for target in "${TARGETS[@]}"; do
  if [[ -n "${target}" ]]; then
    retry rustup target add "${target}" --toolchain "${TOOLCHAIN}"
  fi
done
