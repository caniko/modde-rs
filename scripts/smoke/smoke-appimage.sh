#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need chmod
need timeout

appimage="$(find_one "$RELEASE_DIR" "modde-${VERSION}-x86_64.AppImage" "Build release artifacts appimage-cli output")"
chmod +x "$appimage"

output=""
if output="$(timeout 30 "$appimage" --version 2>&1)"; then
  printf '%s\n' "$output"
  assert_version_output "$VERSION" "$output" "AppImage modde --version"
else
  status=$?
  printf '%s\n' "$output"
  if grep -E "No such file or directory|FUSE|AppImage" <<<"$output" >/dev/null; then
    warn "AppImage exists but this runner cannot execute it; skipping AppImage runtime smoke"
    exit 0
  fi
  die "AppImage modde --version failed with exit ${status}"
fi
