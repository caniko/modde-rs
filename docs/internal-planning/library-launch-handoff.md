# Library launch and sandbox handoff

Commit-pass update (2026-09-30): the implementation below is now committed
locally in `f3ae6aa`, `7bdaf80`, `9b78a06`, `33f17fc` and `4c35e97`.
Nothing has been pushed. The checkout/patch-transfer instructions below describe
the pre-commit snapshot; transfer the committed history for a current handoff.
Formatting, compilation and focused regression checks are authorized. The user's
2026-10-08 instruction to complete the work also authorizes isolated live probes;
publication, provider configuration changes and host deployment remain separate.

Source-review continuation (2026-09-30 → 2026-10-08, from `9649bef`, uncommitted):

- Reviewed parser callers, GUI fixtures/mutation guards, prefix/save contexts and
  deployment re-resolution. Re-traced the pinned Heroic initialization, UMU
  selection and fallback-runner flow; effective layout is resolved at its wrapper.
- Fixed manual ingestion after skipped analysis to reuse matching retained process
  evidence. Skip retains that evidence best-effort without blocking release on
  unavailable evidence/output. Relative imported CSV paths are now saved absolute.
- Added a shared measured-CSV provenance gate for already-ingested capture retries
  and bisect start/run/completion. Stored samples and summaries must match a
  measured reparse of the retained CSV; legacy FPS estimates cannot qualify.
- Added passing regressions for skipped-analysis successful/failed exits, relative
  CSV paths and legacy/changed/missing measured CSV provenance.
- Fixed SHA-256 ID formatting for the dependency update, enabled Steam request
  queries, and preserved multicall/Wine invocation names while resolving scripted
  Proton/UMU launchers. Minimal/full-feature compilation and focused core,
  CLI, game-adapter and GUI regressions passed on 2026-09-30.
- Added explicit, expected-target-checked alias rebinding. Old settings, save
  bindings and deployment state remain with the old physical copy. Configurations
  still referencing the moved alias block rebinding rather than redirect saves.
- The 2026-10-08 source review fixed detached legacy-ID fallback, re-entered the
  public Nix CLI wrapper for generated hooks and detached workers, and checked
  physical Wine prefixes when an existing server exports a Proton compat root.
- A real bubblewrap probe caught redundant bind mounts onto Nix symlink entries.
  The shared builder now reuses already-exposed trees and grants hidden physical
  targets separately, including writable save aliases. The fake-bwrap test alone
  did not catch this failure.
- A detached save-writing child exposed bubblewrap's built-in PID-1 lifetime:
  it killed remaining children when the launcher exited. Sandboxed sessions now
  use a namespace-init reaper sharing the outer observer's descendant handling.
  Its executable/runtime is preflighted before deployment or save replacement.
  Unknown generated bubblewrap options fail closed; literal option values and
  game argv are preserved. Child signals become conventional nonzero exit codes.
- Public XDG desktop discovery hints are retained without extra filesystem or
  session-bus grants, preventing Wine desktop helpers from failing solely because
  those variables were cleared.
- Capability status remains Partial until the remaining provider, packaged-client,
  nested-runtime and representative-game qualification is complete.

Snapshot: 2026-10-08. Continue the user's request to **implement full capability**:
make modde a Lutris replacement for selecting an owned game in the GUI and playing
it with saved settings, plus optional Linux sandboxing and measured overhead.

## Checkout and transfer

- Repository: <https://github.com/caniko/modde-rs>, branch `trunk`.
- Base HEAD: `a18a74b3921c5746695c2b673949d716966f6473`
  (`test(manager): exact pre-registration preservation comparison`).
- Original checkout: `/data/nvme0/can/canix/projects/repos/owned/modde-rs`.
- The original pre-commit snapshot contained tracked and untracked implementation,
  including pre-existing user feature-gating changes and Library UI files. Those
  changes are now committed in the history listed above. Transfer that history
  plus any current working-tree changes; the source-review continuation is still
  uncommitted. Preserve user changes rather than treating every diff as agent-owned.
- Origin is GitHub; `legacy-codeberg` points to `caniko/rs-modde` with push disabled.
- The combined tree's Rust MSRV is `1.94`, edition 2024. Checks used cached Rust
  `1.99.0-nightly-2026-07-18`, not an MSRV qualification. `.envrc` uses the project flake and a sibling
  `../nix-opencode-lsp` checkout; account for that dependency on the new machine.
  `target/`, `.direnv/` and other build caches are not the implementation.

Optional transfer recipe, run from this checkout; these commands have not been
executed as part of this handoff:

```sh
git diff --binary HEAD > /path/to/transfer/modde-tracked.patch
git ls-files --others --exclude-standard -z | tar --null -T - -czf /path/to/transfer/modde-untracked.tar.gz
```

In a clean destination checkout at the base HEAD, apply the patch and extract
the archive at the repository root. Compare `git status --short` with the source.

## Authorization and qualification

- The user authorized `treefmt`, minimal/full-feature Cargo checks and focused
  regressions on 2026-09-30, then continued implementation and requested completion
  on 2026-10-08. Keep live save/config probes isolated from the user's real state.
- Current-tree verification (2026-10-08): 227 core unit tests passed (one existing
  ignored test was not run), all five installation/save-transition tests passed,
  137 CLI unit tests and all 19 Library preparation/supervision tests passed,
  27 game Library units plus all four selected game integrations passed, and all
  13 GUI Library regressions passed: 432 tests in the selected suites. Minimal CLI
  compilation and all-feature/all-target compilation for CLI/core/games/UI/manager
  passed after the namespace-init fix. This is not a full workspace test run.
- Final CLI/game/GUI/check logs use `modde-*-qualified.log`; core units use
  `modde-core-unit-final.log`, and installation/save transitions use
  `modde-core-save-current.log`, all under the evidence root below. Checks used
  `--locked` and two Cargo jobs. Formatting used cached project `treefmt`.
- Checks use the combined working tree, including concurrent staged dependency/
  database updates; they are not receipts for an independently packaged revision.
- `owned_game_library` and `sandboxed_launch` remain **Partial** in
  `docs/capability-matrix.toml`. Nothing establishes working provider integration,
  full provider compatibility or representative-game sandbox overhead yet.
- Original workspace requires formatting through `treefmt`, never direct
  `cargo fmt`/`rustfmt`. Read applicable instructions on the destination machine.

## Requirements and implemented source

- Library-first GUI: filterable installed/uninstalled owned catalogue, ordered
  favorites, modde-managed games, then other games. Favorites are entitlement-wide;
  launch settings, active profiles and save vaults are installation-scoped.
- Steam ownership sync/cache, local manifests and Heroic GOG/Epic/sideload discovery;
  installation reconciliation preserves established IDs and legacy-vault bindings.
  Heroic is the assistant's chosen optional adapter, not a separately approved
  user preference. Steam sync needs SteamID64 and `MODDE_STEAM_API_KEY`; incomplete
  or private responses retain cached ownership. Credentials are not persisted.
- Saved executable/runner/prefix, literal argv, wrappers, environment, working
  directory, profile, save directory and sandbox grants. GUI includes pickers,
  JSON import/export, hook setup, manager controls, save adoption and recovery.
  Revision guards protect asynchronous edits; dirty/saving settings block Play.
- GUI calls the companion `modde` CLI: one profile/deploy/save/launch/completion
  lifecycle, also used by performance runs and bisect candidates.
- Bubblewrap defaults **off**, networking **on**, `store_hook` **false**.
  `<modde_data>/library.json` remains schema version 1.
- Journalled save transitions, immutable outgoing snapshots, experiment-stack
  restoration, durable process evidence, completion receipts and retry handling.
- Alternating paired sandbox-off/on capture, settings/evidence sidecars, warmup
  and sample-quality gates. No performance results have been collected.

### Isolated live evidence (2026-10-08)

Evidence is retained under `/home/can/.cache/opencode/scratch/opencode/`:

- `modde-sandbox-descendants-red/commands.log`: real bubblewrap killed an orphaned
  save writer with SIGKILL before the namespace-init fix. The corresponding green
  and final fixture runs passed. The fixture checks original → other → original
  save continuity, hidden HOME/credentials, read-only assets, separate networking,
  logical executable/save symlinks, consumed requests and a child exiting 23.
  The failed child remains nonzero and its progress is captured, never graded.
- `modde-native-qualified-20261008/receipt.json`: the final native fixture passed
  all four observed runs (three successful, one deliberate child failure), with
  the binary digest retained. Its `commands.log` contains the exact CLI invocations.
- `modde-wine-20261008/receipt-qualified.json`: real host Wine 11.15 `cmd.exe` writes captured
  saves with sandbox off and on. Both runs have completed successful descendant
  evidence. The receipt pins their observation directories and binary digest.
  Earlier failures remain in `commands.log`: malformed fixture redirection,
  missing desktop discovery hints, and the pre-fix namespace killing Wine children.
  This qualifies the initialized command fixture, not Proton/UMU or a real game.
  The final receipt matches the final native fixture's binary digest.
- `modde-graphics-20261008/failed-probe.json`: paired Vulkan-demo capture timed out
  after 240 seconds without CSV output; standalone Wayland and XCB `vkcube` also
  timed out after 20 seconds outside modde. Zero pairs completed. After verifying
  probe processes were gone, fixture saves were finalized with explicit exit
  confirmation and skipped analysis. No overhead figure or successful performance
  receipt was produced. This is a live-graphics qualification blocker.

All probes use isolated config, data, HOME, prefix and save paths. The native
fixture is repeatable with:

```sh
python3 scripts/smoke/library-containment.py --modde /path/to/modde --bwrap /path/to/bwrap --output /fresh/evidence/directory
```

## Contracts to preserve

1. Explicit `LaunchSettings.profile` wins; otherwise `use_active_profile` controls
   installation-active-profile selection. Exact-install Steam `%command%` or
   Heroic wrappers claim URI requests after mutation ownership is released.
   Heroic's hook belongs after the last fgmod wrapper. Hooks pin absolute CLI,
   config and data paths and must be regenerated after relocation.
2. Linux uses a separate subreaper, optionally a namespace-compatible systemd
   user service with `ExitType=cgroup`. Descendant exit evidence controls completion,
   not initial launcher exit. Nested runtimes use an isolated local helper when
   host-service compatibility is unavailable. Sandbox failure never launches
   unsandboxed. `UMU_CONTAINER_NSENTER=1` is rejected; direct commands clear an
   inherited value unless explicitly configured, because services escape observation.
3. `<modde_config>/sessions/` contains launch journals and `run-*/request.json`,
   `evidence.json`, `observer.lock`, `completion.log`. Save capture and a durable
   `pending-completion.json` receipt precede analysis/evidence-status reads.
   `Captured` retries do not recapture; `library finish --skip-analysis` keeps saves
   and releases continuation without grading or advancing pending bisects.
4. `PendingSession::load_blocking()` includes completion receipts; `load()` only
   loads launch/save journals so completion can journal bisect source restoration.
   Failed nested restoration may require `library recover` before another finish.
   Late workers are pinned to the observation directory. Recovery restores saves,
   active profile and experiment stack; deployment/patcher effects are not generally
   reversible, so interrupted preparation/profile-only changes force redeployment.
5. `--config-dir` affects modde only. Native saves, provider discovery and audio
   credentials use the desktop user's configuration root. Native boundaries suppress
   stale prefix/legacy-save inference. `steam_user_path(..., None, ...)` means native;
   prefix inference belongs in `installation_prefix`.
6. Saves/deployment target the physical Wine prefix. Proton/UMU may retain a
   runner-facing container-root `WINEPREFIX` only when its
   `STEAM_COMPAT_DATA_PATH/pfx` resolves to that physical prefix. Store hooks reject
   a saved environment `WINEPREFIX` differing from the store's runner-facing path.
   Profile-managed Heroic saves require explicit `"autoSyncSaves": false` in the
   matching config and a Heroic restart. Initialize runners before adopting saves;
   runtime save-scope changes stop experiments rather than silently move vaults.
7. Sandbox grants cover resolved executables, dedicated Wine/Proton/UMU runtime
   trees and validated writable compat roots alongside the physical prefix.
   Commands/grants are rebuilt after deployment to follow changed symlinks.
8. Shared save fingerprints conservatively classify missing/unknown mod content
   and Wabbajack archive IDs as save-affecting. GUI uses the outgoing profile.
9. Capture/grading use `parse_mangohud_csv_measured`: no FPS-derived frame times.
   The legacy display parser estimates only if the entire frame-time column is
   absent, never for invalid cells. MangoHud `elapsed`/`elapsed_ns` are nanoseconds;
   `time`, `time_s`, `elapsed_seconds` are seconds. Positive warmup requires finite
   elapsed timestamps; all-warmup traces fail. Metrics must be finite and positive.
10. `perf sandbox INSTALLATION_ID --profile PROFILE --pairs N` requires a direct
    executable and 2–30 pairs, alternating launch order. Each successful run needs
    100 usable post-warmup FPS samples and 100 measured frame-time samples.
    Duration controls logging, not termination; startup timing is a 100-ms-polled
    launch-to-first-CSV-sample proxy. Replay the same scene for valid comparisons.
11. Manual `perf ingest --run RUN_ID --csv PATH --warmup-seconds N` preserves
    recorded capture warmup and observed exit status, including re-ingestion after
    receipt clearance. Legacy runs without `configuration.json` use explicit warmup.
    Skipped analysis reuses matching retained completed process evidence when
    available; missing evidence remains unknown. Imported CSV paths are absolute.
    Bisect baselines must match installation/settings, source profile, enabled mod
    order/versions and a completed successful observed exit. Stored series and
    summaries must match a measured reparse of their retained CSV before grading. Old bisects without
    `<modde_data>/bisect/<session>.installation.json` must be restarted.

## Code map

| Area                                                     | Repository paths                                                                                                                                                                                                                                                                    |
| -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Shared lifecycle, receipts, hooks, observation           | `crates/modde-cli/src/commands/library.rs`, `commands/library/{hooks,process}.rs`                                                                                                                                                                                                   |
| CLI dispatch/leases/helper arguments                     | `crates/modde-cli/src/cli/{args/library.rs,runtime.rs,dispatch/,mutation/}`                                                                                                                                                                                                         |
| Catalogue, installation/save contexts, prefixes, sandbox | `crates/modde-games/src/library/{providers,context,launch,runtime,sandbox,operations}.rs`                                                                                                                                                                                           |
| Preferences, identities, journals, save transitions      | `crates/modde-core/src/library.rs`, `library/{identity,session,save_transition}.rs`, `paths.rs`                                                                                                                                                                                     |
| Capture/ingestion/benchmarks/bisects                     | `crates/modde-core/src/performance.rs`, `crates/modde-cli/src/commands/perf.rs`, `commands/bisect/`                                                                                                                                                                                 |
| GUI handlers/settings/race guards                        | `crates/modde-ui/src/app/{update.rs,update_parts/library.rs,update_parts/navigation.rs,tests/library.rs}`, `src/views/library.rs`, `src/views/library/settings.rs`                                                                                                                  |
| Manager bridge and durable markers                       | `crates/modde-manager/src/{main,wiring}.rs` (`.modde-library-session.json`)                                                                                                                                                                                                         |
| Fingerprints/dependencies                                | `crates/modde-games/src/save_fingerprint.rs`, `crates/modde-cli/Cargo.toml` (production `tempfile`)                                                                                                                                                                                 |
| Regression source                                        | `crates/modde-cli/tests/cli_library_{preparation,supervision}.rs`, `crates/modde-games/tests/{installation_context_tests,installation_prefix_tests,store_context_tests,library_sandbox_commands}.rs`, `crates/modde-core/tests/{installation_state_tests,save_transition_tests}.rs` |
| Detailed behavior/status                                 | `docs/src/guides/playing.md`, `docs/src/reference/parity.md`, `docs/capability-matrix.toml`                                                                                                                                                                                         |

## Remaining qualification

1. Qualify the native packaged client and GUI against committed source/pinned
   inputs through PR CI, then real provider wrappers and nested runtimes. Native
   wrapper wiring includes bwrap/coreutils/systemd; generated hooks now pin the
   public wrapper rather than the hidden Nix executable. No PR is published without
   explicit push authorization. Flatpak still requires a separately qualified
   host-launch bridge and packaging; native support is not Flatpak support.
2. Run representative Steam/Heroic game and cloud-save qualification. Heroic
   automatic cloud sync must be disabled and its runner initialized before save
   adoption. Steam Cloud and other existing-service launches remain outside the
   observed descendant boundary. Do not infer safety from successful fixture exits.
3. Repair/qualify the live graphics environment, then collect repeated alternating
   sandbox-off/on measurements of the same real game
   scene. Any graphics-demo result applies to that demo only, not game overhead.
   Retain measured CSVs, warmup/quality gates and successful exit evidence.
4. Review capability upgrades only after the remaining end-to-end evidence exists.
   Existing production CLI/GUI save operations use PendingSession transitions;
   legacy low-level ProfileManager scoped methods remain non-journalled APIs and
   are not a replacement for that guarded lifecycle.

Reviewed upstream pins (see playing guide for links):

- Heroic `3934a83a0707baad23cd2c06bc94bd23f51e6622`:
  `src/backend/launcher.ts`, `utils.ts`, `utils/compatibility_layers.ts`.
- UMU `e2b203a1fdd2af9f35166f5713cb3f85d72587e2`: `umu/umu_run.py`.
- MangoHud `73931de948402caf3ea80f26d5bbb6b3f1d2c1d9`:
  `src/logging.cpp`, CSV columns and nanosecond timestamps around lines 200–230.
