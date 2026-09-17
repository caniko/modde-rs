#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need awk
need grep
need test

assert_file() {
  local path="$1"
  local producer="$2"

  test -s "$path" || die "missing ${path}; required upstream producer: ${producer}"
}

assert_optional_glob() {
  local pattern="$1"
  local producer="$2"

  if ! glob_exists "$pattern"; then
    die "missing artifact matching ${pattern}; required upstream producer: ${producer}"
  fi
}

warn_missing_glob() {
  local pattern="$1"
  local producer="$2"

  if ! glob_exists "$pattern"; then
    warn "missing artifact matching ${pattern}; ${producer} did not produce a runner-usable release asset in this environment"
  fi
}

assert_file Cargo.toml "workspace manifest"
assert_file flake.nix "release linuxDistributionSupport table"
assert_file dist/apt/conf/distributions "APT repository metadata"
assert_file dist/aur/modde/PKGBUILD "AUR source package template"
assert_file dist/aur/modde/.SRCINFO "AUR source package metadata"
assert_file dist/aur/modde-bin/PKGBUILD "AUR binary package template"
assert_file dist/aur/modde-bin/.SRCINFO "AUR binary package metadata"
assert_file dist/aur/modde-git/PKGBUILD "AUR development package template"
assert_file dist/aur/modde-git/.SRCINFO "AUR development package metadata"
assert_file dist/rpm/modde.spec "COPR SRPM spec"

grep -q "linuxDistributionSupport" flake.nix || \
  die "flake.nix does not define linuxDistributionSupport; required upstream producer: release configuration"
for channel in apt copr aur nix flatpak appimage tarball; do
  grep -q "${channel} = {" flake.nix || \
    die "linux_distribution_support is missing channel ${channel}; required upstream producer: release configuration"
done

grep -q '^Architectures: amd64$' dist/apt/conf/distributions || \
  die "APT metadata must keep Architectures: amd64 until arm64 .deb builds are produced"

if [ ! -s dist/apt/key.gpg.asc ]; then
  warn "missing dist/apt/key.gpg.asc; APT publish can still use MODDE_APT_REPO_GPG_PUBLIC_KEY from Actions vars, but the committed public key should be regenerated with: gpg --armor --export \"\$MODDE_APT_REPO_GPG_KEY_ID\" > dist/apt/key.gpg.asc"
fi

assert_optional_glob "${RELEASE_DIR}/modde-${VERSION}-x86_64-linux.tar.gz" "Build release artifacts tarball output"
if [ "${MODDE_LOCAL_DEPLOY_SKIP_AARCH64:-0}" = "1" ]; then
  warn "aarch64 Linux tarball absent because MODDE_LOCAL_DEPLOY_SKIP_AARCH64=1"
else
  assert_optional_glob "${RELEASE_DIR}/modde-${VERSION}-aarch64-linux.tar.gz" "Build release artifacts aarch64 tarball output"
fi
warn_missing_glob "${RELEASE_DIR}/modde-${VERSION}-x86_64.AppImage" "Build release artifacts appimage-cli output"
warn_missing_glob "${RELEASE_DIR}/modde-ui-${VERSION}-x86_64.AppImage" "Build release artifacts appimage-ui output"
assert_optional_glob "${RELEASE_DIR}/com.tartanoglu.modde.json" "Build release artifacts flatpak-manifest output"
assert_optional_glob "${RELEASE_DIR}/cargo-sources.json" "flatpak-cargo-generator output"
assert_optional_glob "${RELEASE_DIR}/modde-rs-${VERSION}.tar.gz" "Build release artifacts source archive"

debs=()
collect_glob debs "${RELEASE_DIR}/*.deb"
if [ "${#debs[@]}" -eq 0 ]; then
  warn "no .deb artifacts found; Debian-family release channel remains gated off until Build Debian packages produces .deb files"
fi

srpms=()
collect_glob srpms "${RELEASE_DIR}/*.src.rpm"
if [ "${#srpms[@]}" -eq 0 ]; then
  die "missing .src.rpm artifact; required upstream producer: Build SRPM for COPR"
fi
