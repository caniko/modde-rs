#!/usr/bin/env bash
set -euo pipefail

# Ubuntu's unprofiled user-namespace restriction allows creating a namespace
# but denies capabilities needed to configure its loopback interface. This
# adjustment applies only to the disposable hosted runner, never game launches.
test "${GITHUB_ACTIONS:-}" = true
if [[ -e /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]]; then
  sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
fi
