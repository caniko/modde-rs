#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need tar
need readelf
need timeout

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

check_linux_tarball() {
  local arch="$1" interpreter="$2" machine="$3" archive members out binary elf
  archive="$(find_one "$RELEASE_DIR" "modde-${VERSION}-${arch}-linux.tar.gz" "native Cargo release job")"
  members="$(tar tzf "$archive" | LC_ALL=C sort)"
  test "$members" = $'modde\nmodde-ui' || die "$archive must contain only top-level modde and modde-ui"

  out="$tmpdir/$arch"
  mkdir -p "$out"
  tar xzf "$archive" -C "$out"
  for binary in "$out/modde" "$out/modde-ui"; do
    [[ -f $binary && ! -L $binary && -x $binary ]] || die "$binary is not an executable regular file"
    elf="$(LC_ALL=C readelf -Whld "$binary")" || die "$binary is not a readable ELF executable"
    [[ $elf =~ Machine:[[:blank:]]+$machine$'\n' ]] || die "$binary is not built for $arch"
    [[ $elf != *"/nix/store/"* ]] || die "$binary depends on a Nix store path"
    [[ $elf == *"[Requesting program interpreter: $interpreter]"* ]] || die "$binary does not use $interpreter"
  done

  if [[ ${MODDE_LINUX_SMOKE_ELF_ONLY:-0} == 1 ]]; then
    return
  fi
  if [[ $(uname -m) == "$arch" ]]; then
    timeout 20 "$interpreter" --list "$out/modde-ui"
    run_version_check "$VERSION" "linux $arch tarball modde --version" timeout 20 "$out/modde" --version
  elif [[ $arch == aarch64 ]]; then
    need qemu-aarch64
    local loader="${MODDE_ARM_SMOKE_LOADER:?missing ARM smoke loader}"
    local libs="${MODDE_ARM_SMOKE_LIBS:?missing ARM smoke libraries}"
    timeout 30 qemu-aarch64 "$loader" --library-path "$libs" --list "$out/modde-ui"
    run_version_check "$VERSION" "linux $arch tarball modde --version via qemu-aarch64" \
      timeout 30 qemu-aarch64 "$loader" --library-path "$libs" "$out/modde" --version
  fi
}

check_linux_tarball x86_64 /lib64/ld-linux-x86-64.so.2 "Advanced Micro Devices X86-64"
if [ "${MODDE_LOCAL_DEPLOY_SKIP_AARCH64:-0}" = "1" ] &&
  ! glob_exists "${RELEASE_DIR}/modde-${VERSION}-aarch64-linux.tar.gz"; then
  warn "aarch64 Linux tarball absent because MODDE_LOCAL_DEPLOY_SKIP_AARCH64=1"
  exit 0
fi
check_linux_tarball aarch64 /lib/ld-linux-aarch64.so.1 "AArch64"
