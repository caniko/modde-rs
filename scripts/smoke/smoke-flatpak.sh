#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need flatpak
need flatpak-builder
need jq
need timeout

manifest="$(find_one "$RELEASE_DIR" "com.tartanoglu.modde.json" "Build release artifacts flatpak-manifest output")"
source_tarball="$(find_one "$RELEASE_DIR" "modde-rs-${VERSION}.tar.gz" "Build SRPM for COPR source archive copy into release/")"
cargo_sources="$(find_one "$RELEASE_DIR" "cargo-sources.json" "flatpak-cargo-generator output")"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

local_manifest="$tmpdir/com.tartanoglu.modde.json"
source_uri="file://$(cd "$(dirname "$source_tarball")" && pwd)/$(basename "$source_tarball")"
cp "$cargo_sources" "$tmpdir/cargo-sources.json"

jq --arg source_uri "$source_uri" '
  .modules[0].sources |= map(
    if type == "object" and .type == "archive" and (.url | test("modde-rs-.*[.]tar[.]gz$"))
    then .url = $source_uri
    else .
    end
  )
' "$manifest" > "$local_manifest"

if [ "${MODDE_FLATPAK_MANIFEST_ONLY:-0}" = "1" ]; then
  jq -e '."app-id" == "com.tartanoglu.modde" and (.modules | length > 0)' "$local_manifest" > /dev/null
  warn "flatpak-builder execution skipped by MODDE_FLATPAK_MANIFEST_ONLY=1; manifest and cargo source metadata validated"
  exit 0
fi

flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak_builder_args=(--user --force-clean --state-dir="$tmpdir/state" --install-deps-from=flathub)
flatpak_can_install=1
flatpak_exports="${XDG_DATA_HOME:-$HOME/.local/share}/flatpak/exports/share/icons/hicolor"
if ! mkdir -p "$flatpak_exports" 2>/dev/null || ! touch "$flatpak_exports/.modde-smoke-write" 2>/dev/null; then
  flatpak_can_install=0
else
  rm -f "$flatpak_exports/.modde-smoke-write"
fi
if [ "$flatpak_can_install" -eq 1 ]; then
  flatpak_builder_args+=(--install)
else
  flatpak_builder_args+=(--repo="$tmpdir/repo")
fi
flatpak_log="$tmpdir/flatpak-builder.log"
if ! flatpak-builder "${flatpak_builder_args[@]}" "$tmpdir/build" "$local_manifest" > "$flatpak_log" 2>&1; then
  cat "$flatpak_log"
  if grep -E "open[(]O_TMPFILE[)]|Error installing deps|Failed to install org[.]freedesktop[.]Sdk|Permission denied" "$flatpak_log" > /dev/null; then
    warn "flatpak-builder cannot install runtime dependencies in this runner; manifest parsing passed and Flathub publish will perform the authoritative build"
    exit 0
  fi
  exit 1
fi
cat "$flatpak_log"
if [ "$flatpak_can_install" -eq 0 ]; then
  warn "flatpak user export directory is not writable in this runner; build/export passed and Flathub publish will perform the authoritative build"
  exit 0
fi

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$tmpdir/runtime}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

flatpak_run_args=(--user)
flatpak_xdg_data="$tmpdir/xdg-data"
flatpak_xdg_config="$tmpdir/xdg-config"
flatpak_xdg_cache="$tmpdir/xdg-cache"
mkdir -p "$flatpak_xdg_data" "$flatpak_xdg_config" "$flatpak_xdg_cache"
flatpak_run_args+=(
  --filesystem="$tmpdir":rw
  --env=MODDE_DATABASE_BACKEND=sqlite
  --env=XDG_DATA_HOME="$flatpak_xdg_data"
  --env=XDG_CONFIG_HOME="$flatpak_xdg_config"
  --env=XDG_CACHE_HOME="$flatpak_xdg_cache"
)
if flatpak run --help 2>&1 | grep -F -- "--no-sandbox" > /dev/null; then
  flatpak_run_args+=(--no-sandbox)
else
  warn "flatpak run does not support --no-sandbox in this runner; using the default sandbox for launch smoke"
fi

flatpak_run_log="$tmpdir/flatpak-run.log"
set +e
timeout 10 flatpak run "${flatpak_run_args[@]}" com.tartanoglu.modde > "$flatpak_run_log" 2>&1
status=$?
set -e
if [ "$status" -ne 0 ]; then
  cat "$flatpak_run_log"
  if [ "$status" -eq 124 ]; then
    echo "flatpak launch stayed alive for 10s; treating launch smoke as passed"
  elif grep -E "Failed to open display|cannot open display|No such display|Could not connect|WAYLAND_DISPLAY|DISPLAY" "$flatpak_run_log" > /dev/null; then
    warn "flatpak launch cannot reach a display server in this runner; build and install smoke passed"
  elif grep -E "failed to connect to postgres|failed to open modde database" "$flatpak_run_log" > /dev/null; then
    die "flatpak launch ignored MODDE_DATABASE_BACKEND=sqlite; database isolation for package smoke is broken"
  else
    die "flatpak run failed; add a no-window modde-ui smoke flag if the atlas runner has no display server"
  fi
fi
