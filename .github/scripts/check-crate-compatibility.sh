#!/usr/bin/env bash
set -euo pipefail

bash .github/scripts/prepare-containment.sh
sudo mkdir -p /var/cache/sccache && sudo chmod 1777 /var/cache/sccache
printf '%s\n' 'extra-sandbox-paths = /var/cache/sccache' 'max-jobs = 1' 'cores = 2' | sudo tee -a /etc/nix/nix.conf
nix develop --no-update-lock-file --max-jobs 1 --cores 2 -c bash -euo pipefail <<'CRATES'
export CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 MODDE_DATABASE_BACKEND=sqlite
for package in modde-core modde-sources modde-games modde-ui modde modde-oracle-api modde-oracle modde-xtask modde-manager; do
  cargo test --locked --jobs 2 -p "$package"
  cargo clippy --locked --jobs 2 -p "$package" --all-targets -- --deny warnings
done
for package in modde-core modde-sources modde-games modde-ui modde modde-manager; do
  cargo test --locked --jobs 2 -p "$package" --no-default-features
  cargo clippy --locked --jobs 2 -p "$package" --all-targets --no-default-features -- --deny warnings
done
cargo test --locked --jobs 2 -p modde-sources --no-default-features --features bethesda-archives --test bsa_edge_cases --test bsa_repack_integration
# Keep platform-tool fixtures covered independently of game/database features.
cargo test --locked --jobs 2 -p modde-ui --no-default-features --features linux-integrations
cargo clippy --locked --jobs 2 -p modde-ui --all-targets --no-default-features --features linux-integrations -- -D warnings
# The subprocess suite exercises the wow-featured manager binary.
cargo test --locked --jobs 2 -p modde-manager --no-default-features --features wow --test config_fallback
# Exercise built-in-game and PostgreSQL metadata fixtures with only their prerequisites.
cargo test --locked --jobs 2 -p modde --no-default-features --features bethesda,cyberpunk,ue4,postgres --test cli_config_tests --test cli_exec --test cli_game_export_import --test cli_patcher_exec --test cli_scan_dispatch
for package in modde-core modde-sources modde-games modde-ui modde modde-oracle-api modde-manager; do
  cargo package --locked -p "$package" --allow-dirty --list
done
cargo run --locked --jobs 2 -p modde-xtask -- check
CRATES
