#!/usr/bin/env bash

ok() {
  printf 'ok: %s\n' "$*"
}

warn() {
  warnings+=("$*")
  printf 'warn: %s\n' "$*" >&2
}

need_tool() {
  if command -v "$1" >/dev/null 2>&1; then
    ok "tool $1"
  else
    missing+=("tool:$1")
  fi
}

has_env() {
  local name="$1"
  [ -n "${!name:-}" ]
}

need_env() {
  local scope="$1"
  local name="$2"
  if has_env "$name"; then
    ok "$scope $name present"
  else
    missing+=("$scope:$name")
  fi
}

load_env_file() {
  local name="$1"
  local path="$2"
  if has_env "$name" || [ ! -r "$path" ]; then
    return
  fi

  local value
  value="$(cat "$path")"
  printf -v "$name" '%s' "$value"
  export "$name"
}

load_runtime_secret() {
  local name="$1"
  local file="$2"
  local runtime="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
  local path
  path="$(find "$runtime/agenix.d" -type f -name "$file" 2>/dev/null | sort -V | tail -n 1 || true)"
  if [ -n "$path" ]; then
    load_env_file "$name" "$path"
  fi
}

load_canix_release_inputs() {
  local canix_root="${CANIX_ROOT:-/data/nvme0/can/canix}"

  load_runtime_secret CODEBERG_TOKEN can_codeberg_token
  load_runtime_secret MINISIGN_SECRET_KEY can_minisign_secret_key
  load_runtime_secret MINISIGN_PASSWORD can_minisign_password
  load_runtime_secret AUR_SSH_KEY can_aur_ssh_key
  load_runtime_secret COPR_LOGIN can_coppr_login
  load_runtime_secret COPR_TOKEN can_coppr_token
  load_runtime_secret CHOCOLATEY_API_KEY can_choco_api_key
  load_runtime_secret CRATES_IO_API_TOKEN can_crates_io

  load_runtime_secret MODDE_APT_REPO_GPG_KEY can_modde_apt_repo_gpg_key
  load_runtime_secret MODDE_APT_REPO_SSH_KEY can_modde_apt_repo_ssh_key
  load_runtime_secret APT_REPO_GPG_KEY can_modde_apt_repo_gpg_key
  load_runtime_secret APT_REPO_SSH_KEY can_modde_apt_repo_ssh_key

  load_env_file MODDE_APT_REPO_GPG_KEY_ID "$canix_root/age/secrets/root/modules/repos/apt/modde_apt_repo_gpg_key.fingerprint"
  load_env_file MODDE_APT_REPO_GPG_FINGERPRINT "$canix_root/age/secrets/root/modules/repos/apt/modde_apt_repo_gpg_key.fingerprint"
  load_env_file MODDE_APT_REPO_GPG_PUBLIC_KEY "$canix_root/age/secrets/root/modules/repos/apt/modde_apt_repo_gpg_key.asc"
  load_env_file COPR_USERNAME "$canix_root/age/secrets/root/modules/repos/coppr/username"

  if ! has_env APT_REPO_GPG_KEY && has_env MODDE_APT_REPO_GPG_KEY; then
    APT_REPO_GPG_KEY="$MODDE_APT_REPO_GPG_KEY"
    export APT_REPO_GPG_KEY
  fi
  if ! has_env APT_REPO_SSH_KEY && has_env MODDE_APT_REPO_SSH_KEY; then
    APT_REPO_SSH_KEY="$MODDE_APT_REPO_SSH_KEY"
    export APT_REPO_SSH_KEY
  fi
  if ! has_env APT_REPO_GPG_KEY_ID && has_env MODDE_APT_REPO_GPG_KEY_ID; then
    APT_REPO_GPG_KEY_ID="$MODDE_APT_REPO_GPG_KEY_ID"
    export APT_REPO_GPG_KEY_ID
  fi
  if ! has_env SCOOP_BUCKET_TOKEN && has_env CODEBERG_TOKEN; then
    SCOOP_BUCKET_TOKEN="$CODEBERG_TOKEN"
    export SCOOP_BUCKET_TOKEN
  fi
}

optional_env() {
  local name="$1"
  if has_env "$name"; then
    ok "optional $name present"
  else
    warn "optional $name absent; matching release workflow will skip that publisher or fallback"
  fi
}

collect_codeberg_metadata() {
  if ! command -v fj >/dev/null 2>&1; then
    warn "fj unavailable; repo secret/variable metadata checks disabled"
    return
  fi

  repo_secret_names="$(fj -H codeberg.org actions secrets list -r caniko/rs-modde 2>/dev/null | awk '{print $NF}' | tr '\n' ' ' || true)"
  repo_variable_names="$(fj -H codeberg.org actions variables list -r caniko/rs-modde 2>/dev/null | awk -F' = ' 'NF >= 2 {print $1}' | tr '\n' ' ' || true)"
}

metadata_has_name() {
  local names=" $1 "
  local name="$2"
  [[ "$names" == *" $name "* ]]
}

need_env_or_repo_secret() {
  local scope="$1"
  local name="$2"
  if has_env "$name"; then
    ok "$scope $name present locally"
  elif metadata_has_name "$repo_secret_names" "$name"; then
    ok "$scope $name exists on caniko/rs-modde"
  else
    missing+=("$scope:$name")
  fi
}

need_env_or_repo_variable() {
  local scope="$1"
  local name="$2"
  if has_env "$name"; then
    ok "$scope $name present locally"
  elif metadata_has_name "$repo_variable_names" "$name"; then
    ok "$scope $name exists on caniko/rs-modde"
  else
    missing+=("$scope:$name")
  fi
}

need_codeberg_auth() {
  if has_env CODEBERG_TOKEN; then
    ok "global/user secret CODEBERG_TOKEN present"
    return
  fi

  local store="${XDG_DATA_HOME:-$HOME/.local/share}/forgejo-cli/keys.json"
  if [ -r "$store" ] && jq -e '.hosts["codeberg.org"].token | strings | length > 0' "$store" >/dev/null; then
    CODEBERG_TOKEN="$(jq -r '.hosts["codeberg.org"].token' "$store")"
    export CODEBERG_TOKEN
    ok "Codeberg fj auth token present in local store"
    return
  fi

  missing+=("global/user secret:CODEBERG_TOKEN or fj codeberg.org token")
}

need_chocolatey_tool() {
  if command -v choco >/dev/null 2>&1; then
    ok "tool choco"
    return
  fi

  if command -v nix >/dev/null 2>&1 \
    && nix shell github:caniko/nixpkgs/add-chocolatey-scoop#chocolatey git+https://github.com/caniko/simit -c command -v choco >/dev/null 2>&1; then
    ok "tool choco via configured nix_tool"
    return
  fi

  missing+=("tool:choco")
}

require_local_secret_for_publish() {
  local destination="$1"
  local name="$2"
  local source="$3"
  local validation="$4"
  if has_env "$name"; then
    return 0
  fi
  if declare -F record_skipped >/dev/null 2>&1; then
    record_skipped "$destination missing $name"
  else
    printf 'skipped: %s missing %s\n' "$destination" "$name" >&2
  fi
  printf '  source: %s\n' "$source" >&2
  printf '  validate: %s\n' "$validation" >&2
  return 1
}

release_manifest_init() {
  local version="$1"
  local generator="$2"
  local release_dir="${RELEASE_DIR:-release}"
  mkdir -p "$release_dir"
  jq -n --arg version "$version" --arg generator "$generator" \
    '{version: $version, artifacts: [], skipped: [], generated_by: $generator}' \
    > "$release_dir/artifacts.json.tmp"
  mv "$release_dir/artifacts.json.tmp" "$release_dir/artifacts.json"
}

release_manifest_add_file() {
  local path="$1"
  local producer="$2"
  local release_dir="${RELEASE_DIR:-release}"
  [ -f "$path" ] || return 0
  local sha256
  sha256="$(sha256sum "$path" | awk '{print $1}')"
  jq --arg path "$path" --arg sha256 "$sha256" --arg producer "$producer" \
    '.artifacts = (((.artifacts // []) | map(select(.path != $path))) + [{path: $path, sha256: $sha256, producer: $producer}])' \
    "$release_dir/artifacts.json" > "$release_dir/artifacts.json.tmp"
  mv "$release_dir/artifacts.json.tmp" "$release_dir/artifacts.json"
}

release_manifest_skip() {
  local name="$1"
  local reason="$2"
  local release_dir="${RELEASE_DIR:-release}"
  jq --arg name "$name" --arg reason "$reason" \
    '.skipped = (((.skipped // []) | map(select(.name != $name))) + [{name: $name, reason: $reason}])' \
    "$release_dir/artifacts.json" > "$release_dir/artifacts.json.tmp"
  mv "$release_dir/artifacts.json.tmp" "$release_dir/artifacts.json"
}

release_manifest_collect_release_files() {
  local producer="$1"
  local release_dir="${RELEASE_DIR:-release}"
  local file
  shopt -s nullglob
  for file in "$release_dir"/*; do
    [ -f "$file" ] || continue
    case "$file" in
      "$release_dir/artifacts.json" | "$release_dir/artifacts.json.tmp") continue ;;
    esac
    release_manifest_add_file "$file" "$producer"
  done
  shopt -u nullglob
}
