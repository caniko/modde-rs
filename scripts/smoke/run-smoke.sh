#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 VERSION [RELEASE_DIR]" >&2
  exit 1
fi

VERSION="$1"
RELEASE_DIR_INPUT="${2:-release}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "$RELEASE_DIR_INPUT"
RELEASE_DIR="$(cd "$RELEASE_DIR_INPUT" && pwd)"
REPORT="${RELEASE_DIR}/smoke-report.txt"
TMP_REPORT="${REPORT}.tmp"

rm -f "$TMP_REPORT"

log() {
  printf '%s\n' "$*" | tee -a "$TMP_REPORT"
}

pass=0
fail=0

log "modde release smoke report"
log "version: ${VERSION}"
log "release_dir: ${RELEASE_DIR}"
log "policy: all script failures are blocking; lintian and rpmlint diagnostics are warnings inside their scripts"
log ""

shopt -s nullglob
scripts=("${SCRIPT_DIR}"/smoke-*.sh)
shopt -u nullglob

if [ "${#scripts[@]}" -eq 0 ]; then
  log "[FAIL] no smoke scripts found in ${SCRIPT_DIR}"
  mv "$TMP_REPORT" "$REPORT"
  exit 1
fi

for script in "${scripts[@]}"; do
  name="$(basename "$script" .sh)"
  step_log="$(mktemp)"
  log "== ${name} =="

  if "$script" "$VERSION" "$RELEASE_DIR" >"$step_log" 2>&1; then
    sed 's/^/  /' "$step_log" | tee -a "$TMP_REPORT"
    log "[PASS] ${name}"
    pass=$((pass + 1))
  else
    status=$?
    sed 's/^/  /' "$step_log" | tee -a "$TMP_REPORT"
    log "[FAIL] ${name} (exit ${status})"
    fail=$((fail + 1))
  fi

  rm -f "$step_log"
  log ""
done

log "smoke: ${pass} pass, ${fail} fail"
mv "$TMP_REPORT" "$REPORT"

test "$fail" -eq 0
