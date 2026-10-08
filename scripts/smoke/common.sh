#!/usr/bin/env bash

die() {
  echo "error: $*" >&2
  exit 1
}

warn() {
  echo "warning: $*" >&2
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

require_args() {
  if [ "$#" -ne 2 ]; then
    die "usage: $0 VERSION RELEASE_DIR"
  fi
}

find_one() {
  local dir="$1"
  local pattern="$2"
  local producer="${3:-the release artifact producer}"

  local matches=()
  collect_glob matches "${dir}/${pattern}"

  if [ "${#matches[@]}" -eq 0 ]; then
    die "missing artifact matching ${dir}/${pattern}; required upstream producer: ${producer}"
  fi
  if [ "${#matches[@]}" -ne 1 ]; then
    printf 'matched artifacts:\n' >&2
    printf '  %s\n' "${matches[@]}" >&2
    die "expected exactly one artifact matching ${dir}/${pattern}"
  fi

  printf '%s\n' "${matches[0]}"
}

find_many() {
  local dir="$1"
  local pattern="$2"
  local producer="${3:-the release artifact producer}"

  local matches=()
  collect_glob matches "${dir}/${pattern}"

  if [ "${#matches[@]}" -eq 0 ]; then
    die "missing artifacts matching ${dir}/${pattern}; required upstream producer: ${producer}"
  fi

  printf '%s\n' "${matches[@]}"
}

collect_many() {
  local -n __modde_collect_many_out="$1"
  local dir="$2"
  local pattern="$3"
  local producer="${4:-the release artifact producer}"

  collect_glob __modde_collect_many_out "${dir}/${pattern}"

  if [ "${#__modde_collect_many_out[@]}" -eq 0 ]; then
    die "missing artifacts matching ${dir}/${pattern}; required upstream producer: ${producer}"
  fi
}

collect_glob() {
  local -n __modde_collect_glob_out="$1"
  local pattern="$2"

  __modde_collect_glob_out=()
  local globbed=()
  shopt -s nullglob
  # shellcheck disable=SC2206
  globbed=($pattern)
  shopt -u nullglob
  local existing=()
  local candidate
  for candidate in "${globbed[@]}"; do
    [ -e "$candidate" ] || continue
    existing+=("$candidate")
  done
  if [ "${#existing[@]}" -gt 0 ]; then
    mapfile -t __modde_collect_glob_out < <(printf '%s\n' "${existing[@]}" | sort)
  fi
}

glob_exists() {
  local pattern="$1"
  local matches=()
  collect_glob matches "$pattern"
  [ "${#matches[@]}" -gt 0 ]
}

assert_version_output() {
  local expected="$1"
  local output="$2"
  local label="$3"

  if ! grep -F -- "$expected" <<<"$output" >/dev/null; then
    printf '%s\n' "$output" >&2
    die "${label} did not report expected version ${expected}"
  fi
}

run_version_check() {
  local expected="$1"
  local label="$2"
  shift 2

  local output
  if ! output="$("$@" 2>&1)"; then
    printf '%s\n' "$output" >&2
    die "${label} failed"
  fi

  printf '%s\n' "$output"
  assert_version_output "$expected" "$output" "$label"
}

root_run() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    die "this check needs root privileges for: $*; run on the atlas runner with sudo or provide a root-capable container workflow"
  fi
}
