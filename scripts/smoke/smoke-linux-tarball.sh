#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need tar
need timeout

linux_x86="$(find_one "$RELEASE_DIR" "modde-${VERSION}-x86_64-linux.tar.gz" "Build release artifacts")"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

mkdir -p "$tmpdir/x86_64"
tar xzf "$linux_x86" -C "$tmpdir/x86_64"
test -x "$tmpdir/x86_64/modde" || die "${linux_x86} did not contain an executable modde binary"
run_version_check "$VERSION" "linux x86_64 tarball modde --version" timeout 20 "$tmpdir/x86_64/modde" --version

if [ "${MODDE_LOCAL_DEPLOY_SKIP_AARCH64:-0}" = "1" ] &&
  ! glob_exists "${RELEASE_DIR}/modde-${VERSION}-aarch64-linux.tar.gz"; then
  warn "aarch64 Linux tarball absent because MODDE_LOCAL_DEPLOY_SKIP_AARCH64=1"
  exit 0
fi
linux_arm="$(find_one "$RELEASE_DIR" "modde-${VERSION}-aarch64-linux.tar.gz" "Build release artifacts")"
mkdir -p "$tmpdir/aarch64"
tar xzf "$linux_arm" -C "$tmpdir/aarch64"
test -x "$tmpdir/aarch64/modde" || die "${linux_arm} did not contain an executable modde binary"

if command -v qemu-aarch64 >/dev/null 2>&1; then
  run_version_check "$VERSION" "linux aarch64 tarball modde --version via qemu-aarch64" \
    timeout 30 qemu-aarch64 "$tmpdir/aarch64/modde" --version
else
  run_version_check "$VERSION" "linux aarch64 tarball modde --version via binfmt" \
    timeout 30 "$tmpdir/aarch64/modde" --version
fi
