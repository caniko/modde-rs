#!/usr/bin/env bash
# Compatibility entry point. The release stack owns APT rendering and
# publication; keep this path for local-release-deploy callers that have not
# moved to `simit dist apt publish` yet.
set -euo pipefail

VERSION="${VERSION:?VERSION must be set to the release tag}"
RELEASE_DIR="${RELEASE_DIR:-release}"
APT_REPO_REMOTE="${APT_REPO_REMOTE:-ssh://git@github.com/caniko/apt-modde.git}"
work_dir="${APT_REPO_WORKDIR:-${MODDE_LOCAL_RELEASE_WORKDIR:-target/modde-release/work}/apt}"
simit_bin="${SIMIT_BIN:-simit}"
args=(
  dist apt publish
  --version "$VERSION"
  --release-dir "$RELEASE_DIR"
  --work-dir "$work_dir"
  --apt-repo-url "$APT_REPO_REMOTE"
  --push
)

if [ "${MODDE_LOCAL_DEPLOY_IN_DEVSHELL:-0}" = "1" ]; then
  exec "$simit_bin" "${args[@]}"
fi
exec nix develop -c "$simit_bin" "${args[@]}"
