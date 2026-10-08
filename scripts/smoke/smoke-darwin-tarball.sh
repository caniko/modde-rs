#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need file
need tar

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

check_darwin_tarball() {
  local arch="$1"
  local expected_file_token="$2"
  local tarball
  tarball="$(find_one "$RELEASE_DIR" "modde-${VERSION}-${arch}-darwin.tar.gz" "Build release artifacts darwin-${arch}")"

  local outdir="$tmpdir/${arch}"
  mkdir -p "$outdir"
  tar xzf "$tarball" -C "$outdir"
  test -f "$outdir/modde" || die "${tarball} did not contain modde"

  local file_output
  file_output="$(file "$outdir/modde")"
  printf '%s\n' "$file_output"
  grep -F "Mach-O" <<<"$file_output" >/dev/null || die "${tarball} modde is not a Mach-O binary"
  grep -F "$expected_file_token" <<<"$file_output" >/dev/null || die "${tarball} modde is not ${expected_file_token}"

  if grep -F "universal binary" <<<"$file_output" >/dev/null; then
    if command -v lipo >/dev/null 2>&1; then
      lipo -info "$outdir/modde"
    else
      warn "lipo is unavailable; universal Mach-O slice list could not be printed"
    fi
  fi
}

check_darwin_tarball "aarch64" "arm64"
