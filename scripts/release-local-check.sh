#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
if [ -z "$version" ]; then
  echo "usage: release-local-check <version>" >&2
  exit 2
fi

repo="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$repo"

release_root="${MODDE_LOCAL_RELEASE_ROOT:-$repo/target/modde-release/$version}"
release_dir="${MODDE_LOCAL_RELEASE_DIR:-$release_root/release}"
work_dir="${MODDE_LOCAL_RELEASE_WORKDIR:-$release_root/work}"
mkdir -p "$release_dir" "$work_dir"
export RELEASE_DIR="$release_dir"
export MODDE_LOCAL_RELEASE_WORKDIR="$work_dir"

missing=()
warnings=()
repo_secret_names=""
repo_variable_names=""

. "$repo/scripts/release-local-env.sh"
release_manifest_init "$version" "modde-rs release-local-check"

check_minisign_probe() {
  test -s keys/minisign.pub || missing+=("file:keys/minisign.pub")
  if [ -z "${MINISIGN_SECRET_KEY:-}" ] || [ -z "${MINISIGN_PASSWORD:-}" ] || [ ! -s keys/minisign.pub ]; then
    return
  fi

  umask 077
  local tmpdir key probe sig
  tmpdir="$(mktemp -d)"
  key="$tmpdir/minisign.key"
  probe="$tmpdir/minisign-probe.txt"
  sig="$probe.minisig"
  trap 'rm -rf "$tmpdir"' RETURN

  printf '%s' "$MINISIGN_SECRET_KEY" > "$key"
  printf 'modde-rs local release credentials probe\n' > "$probe"
  printf '%s\n' "$MINISIGN_PASSWORD" | minisign -S -s "$key" -m "$probe" -x "$sig" >/dev/null
  minisign -V -m "$probe" -x "$sig" -p keys/minisign.pub >/dev/null
  ok "minisign probe signs and verifies with keys/minisign.pub"
}

check_cosign_degrade() {
  local tmpdir artifact predicate
  tmpdir="$(mktemp -d)"
  artifact="$tmpdir/artifact.txt"
  predicate="$tmpdir/predicate.json"
  trap 'rm -rf "$tmpdir"' RETURN

  printf 'modde-rs cosign local degrade probe\n' > "$artifact"
  printf '{"_type":"https://in-toto.io/Statement/v1","subject":[],"predicateType":"https://slsa.dev/provenance/v1","predicate":{}}\n' > "$predicate"

  if env -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u COSIGN_PRIVATE_KEY \
    bash -c 'test -z "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" && test -z "${COSIGN_PRIVATE_KEY:-}"'; then
    warn "keyless Sigstore and COSIGN_PRIVATE_KEY unavailable locally; release workflow degrades to warning-only attestation"
  else
    missing+=("cosign:local warning-only degradation probe")
  fi
}

check_workflow_contract() {
  grep -F 'nix develop -c simit dist apt build --version "$VERSION" --release-dir release' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow uses Simit for Debian package builds" \
    || missing+=("workflow:Simit Debian package builder")

  grep -F 'nix develop -c simit dist apt publish --version "$VERSION" --release-dir release --push' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow uses Simit for APT publication" \
    || missing+=("workflow:Simit APT publisher")

  grep -F 'nix develop -c simit dist scoop bump --version "$VERSION"' .forgejo/workflows/release.yml >/dev/null \
    && grep -F 'SCOOP_BUCKET_TOKEN: ${{ secrets.CODEBERG_TOKEN }}' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow uses Simit for Scoop publication" \
    || missing+=("workflow:Simit Scoop publisher")

  ! grep -F 'bash scripts/build-deb.sh' .forgejo/workflows/release.yml >/dev/null \
    && ! grep -F 'reprepro -b' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow contains no project-local APT implementation" \
    || missing+=("workflow:project-local APT implementation")

  grep -F 'keyless Sigstore failed and COSIGN_PRIVATE_KEY unset; continuing without cosign signature or attestation' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow keeps missing cosign fallback warning-only" \
    || missing+=("workflow:cosign warning-only fallback")

  grep -F 'nix run .#copr-cli -- build --nowait "${COPR_PROJECT}" target/modde-release/root-artifacts/srpms/*.src.rpm' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow uses local COPR CLI flake app" \
    || missing+=("workflow:local COPR CLI app")

  grep -F 'target/modde-release/root-artifacts/linux-result \' .forgejo/workflows/release.yml >/dev/null \
    && grep -F 'target/modde-release/root-artifacts/flatpak-result \' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow passes local result paths to nix path-info for Attic" \
    || missing+=("workflow:Attic local result paths")

  grep -F 'Attic login failed for configured Nix closure cache publishing.' .forgejo/workflows/release.yml >/dev/null \
    && grep -F 'Attic push failed for configured Nix closure cache publishing.' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow fails closed when configured Attic publishing fails" \
    || missing+=("workflow:Attic failure contract")

  grep -F 'skipping cosign verification because release signing degrades to warning-only' scripts/smoke/smoke-signatures.sh >/dev/null \
    && ok "smoke keeps missing cosign verification warning-only" \
    || missing+=("smoke:cosign warning-only fallback")

  grep -F 'COPR publish will perform the authoritative remote build' scripts/smoke/smoke-srpm.sh >/dev/null \
    && ok "smoke lets COPR remote build validate SRPM when local podman policy is unavailable" \
    || missing+=("smoke:COPR local podman fallback")

  grep -F 'Authenticode signing is optional and was skipped by the release workflow' scripts/smoke/smoke-windows-zip.sh >/dev/null \
    && ok "smoke treats unsigned Windows artifacts as optional when signing credentials are absent" \
    || missing+=("smoke:Windows optional signing fallback")

  grep -F 'Flathub publish will perform the authoritative build' scripts/smoke/smoke-flatpak.sh >/dev/null \
    && ok "smoke treats unavailable Flatpak runtime install as a runner limitation" \
    || missing+=("smoke:Flatpak runtime fallback")

  grep -F 'wine cannot execute' scripts/smoke/smoke-windows-zip.sh >/dev/null \
    && ok "smoke treats unavailable Wine runtime execution as a runner limitation" \
    || missing+=("smoke:Wine runtime fallback")

  grep -F 'smoke-darwin-tarball.sh) continue' scripts/local-release-deploy.sh >/dev/null \
    && missing+=("local-deploy:Darwin smoke must run by default") \
    || ok "local deploy includes Darwin tarball smoke"

  grep -F 'Homebrew disabled while macOS artifacts are skipped' scripts/local-release-deploy.sh >/dev/null \
    && missing+=("local-deploy:Homebrew must not be globally disabled") \
    || ok "local deploy no longer hard-disables Homebrew"

  grep -F 'publish_homebrew()' scripts/local-release-deploy.sh >/dev/null \
    && grep -F 'HOMEBREW_TAP_TOKEN' scripts/local-release-deploy.sh >/dev/null \
    && grep -F "modde-\${version}-aarch64-darwin.tar.gz" scripts/local-release-deploy.sh >/dev/null \
    && ! grep -F "modde-\${version}-x86_64-" scripts/local-release-deploy.sh | grep -F "darwin.tar.gz" >/dev/null \
    && ok "local deploy has credential-gated Homebrew publisher with arm64 Darwin artifacts" \
    || missing+=("local-deploy:Homebrew publisher with Darwin artifact gates")
}

load_canix_release_inputs

for tool in \
  bash git curl jq cargo rustc cargo-deny cargo-about cargo-sbom cargo-cyclonedx \
  cargo-deb dpkg-deb rpmbuild minisign cosign reprepro fj \
  file gpg ssh node npm copr-cli simit; do
  need_tool "$tool"
done
need_chocolatey_tool
collect_codeberg_metadata

need_codeberg_auth
need_env_or_repo_secret "repo secret" MINISIGN_SECRET_KEY
need_env_or_repo_secret "repo secret" MINISIGN_PASSWORD
need_env_or_repo_secret "repo secret" MODDE_APT_REPO_GPG_KEY
need_env_or_repo_secret "repo secret" MODDE_APT_REPO_SSH_KEY
need_env_or_repo_variable "repo variable" MODDE_APT_REPO_GPG_KEY_ID
need_env_or_repo_variable "repo variable" MODDE_APT_REPO_GPG_FINGERPRINT
need_env_or_repo_variable "repo variable" MODDE_APT_REPO_GPG_PUBLIC_KEY
need_env "global/user secret" AUR_SSH_KEY
need_env "global/user secret" COPR_LOGIN
need_env "global/user variable" COPR_USERNAME
need_env "global/user secret" COPR_TOKEN
need_env "global/user secret" CHOCOLATEY_API_KEY
need_env "global/user secret" CRATES_IO_API_TOKEN

optional_env MODDE_APT_REPO_GPG_PASSPHRASE
optional_env COSIGN_PRIVATE_KEY
optional_env COSIGN_PASSWORD
optional_env WINDOWS_SIGNING_PFX
optional_env WINDOWS_SIGNING_PASS
optional_env FLATHUB_TOKEN
optional_env WINGET_PAT
optional_env HOMEBREW_TAP_TOKEN
optional_env SCOOP_BUCKET_TOKEN

check_workflow_contract
check_minisign_probe
check_cosign_degrade
release_manifest_collect_release_files "existing-release-artifact"

if [ "${#missing[@]}" -ne 0 ]; then
  printf 'missing required local release parity inputs for %s:\n' "$version" >&2
  printf '  - %s\n' "${missing[@]}" >&2
  exit 1
fi

printf 'local release parity dry-run passed for %s; no external publish was attempted\n' "$version"
