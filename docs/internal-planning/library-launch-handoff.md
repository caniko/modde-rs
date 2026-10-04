# Library launch and sandbox handoff

## Current handoff, 2026-10-04

The implementation is published at
`0d3ce666e2362555b90d4d37b084cc8d86450eb9` on
`integration/native-library-gpu` in [Modde PR #3](https://github.com/caniko/modde-rs/pull/3).
Producer and native package CI passed; independent verification reports 2,092
passed tests, zero failures and two ignored tests. The historical Canix consumer
also passed actual-wrapper, real-bubblewrap lifecycle and diagnostic fixtures.
See [Library and sandbox qualification](../src/reference/library-qualification.md)
for exact revisions, artifacts and receipts. Later edits need their own checks;
the approved consumer pin remains `0d3ce666…` until a replacement is selected.

Native Linux is the required target. Flatpak is follow-up work. Performance is
report-only: retain measurements, paired deltas and variability. No real-game
performance or save-continuity qualification has occurred in this worker's scope.
The `cryoglyph`/Iced dependency on `lru 0.16.4` and RUSTSEC-2026-0253 remains.

The coordinator selected Canix
`3b29bb33d125381f9d34b4f18657755cbe4a0379` for scoped non-activating qualification.
The Modde worker completed its single corrected online VM retry and delivered
the measured package/wrapper and remote-policy transfer. The coordinator
independently accepted that transfer and the combined binding/post-checks.
The user explicitly reassigned the sole changed-hook Nomad VM to the coordinator
after the ETL worker became unavailable; that gate also passed. Production ETL
writer authority is still unassigned/unconfirmed.
The coordinator owns the
combined profile-aware binding, final composition, packaged CLI/full local
registry qualification, recovery/adoption, activation and writer release.
Canix Actions remain **skipped-by-operator**. A moving checkout HEAD does not
authorize rebinding, and source equality alone cannot transfer execution evidence.

The corrected online VM passed with one JUnit test, zero failures/errors/skips
and all 13 rendered lifecycle assertions preserved. Worker post-terminal
verification retains the full 1,207-line log, exact private publication and
physical managed output/binding roots:
`/data/scratch/tmp/opencode/modde-3b29-online-terminal-verification-20261004T223136116229Z/receipt-v2.json`
(SHA-256 `d08e7ea2ff6a53f3ec4a240bd400476286a7054dff0f249161e545eb3529333f`).
This is the sole actual corrected retry, using normal admission, one job/two cores
and no additional flake evaluation. Earlier failures/nonexecution remain retained.

Independent verification accepted both online and changed-hook Nomad gates:
`/data/scratch/tmp/opencode/durability-3b29-vm-terminal-independent-v3-20261004T224505764092Z/receipt.json`
(SHA-256 `9d7c8a498c7109dacf0f49399d15c298d435d8d50874fa9ae00ab1691b4bd823`).
Nomad's single selected realization also has one JUnit test with no failures,
errors or skips, exact private publication/root, the unchanged 41 production SQL
checks plus one fixture check, equal source/local/off-host records, and export v2
with exactly 2,000 files/35 directories and all sizes/hashes/no extras.
Scoped local qualification is green; physical production restore, the last
acknowledged-row boundary, writer window and final cohort remain unadmitted.

Modde's measured storage cohort is final-service PostgreSQL `rnyi…`, distinct
from the configured `3hc…` package. Preserve both bindings. Raven's full-stage
Canix lease-launcher artifacts also differ from historical acceptance; complete
stage/runtime/CLI qualification remains a separate coordinator gate, including
literal-safe `PGUSER` and source-revision projections.

PR #2 stays open pending explicit CI/release preservation or retirement; its
distinct release/per-crate CI workflows prevent a supersession claim. PR #3 needs
substantive current-head review and acceptable CI through available guarded
review/merge interfaces. Local docs remain native-qualified, with no replacement
producer pin or inherited hosted qualification. Preserve active/history roots,
generated docs and failed/cancelled receipts while their gates remain pending;
cleanup requires exact-owned inactive regenerated artifacts and preservation
through Doty.

Next, settle producer closeout/review before any replacement producer pin,
and agree the final composition and Atlas runtime window. Entry must
reconfirm Modde inactivity and the actual foreground marker/pause policy.
Live direct/store/manager launches, Proton/UMU, interruption/save continuity and
observed game-renderer evidence remain required. Budget about 80 minutes of
gameplay plus setup for eight alternating five-minute off/on measurement pairs.
Capability status remains Partial until end-to-end evidence supports changing it.

## Historical pre-qualification snapshot

The material below records the 2026-09-30 handoff. Its edits-only authorization,
checkout/patch-transfer recipe and original execution checklist are historical;
use the current exact-source selection and published history for new work.

Snapshot: 2026-09-30. Continue the user's request to **implement full capability**:
make modde a Lutris replacement for selecting an owned game in the GUI and playing
it with saved settings, plus optional Linux sandboxing and measured overhead.

## Checkout and transfer

- Repository: <https://github.com/caniko/modde-rs>, branch `trunk`.
- Base HEAD: `a18a74b3921c5746695c2b673949d716966f6473`
  (`test(manager): exact pre-registration preservation comparison`).
- Original checkout: `/data/nvme0/can/canix/projects/repos/owned/modde-rs`.
- Transfer the local implementation commits plus current tracked changes
  **and untracked files**; a fresh clone or tracked-only diff loses work.
  The working tree also contains pre-existing user feature-gating changes and
  Library UI files. Preserve them; do not treat the whole diff as agent-owned.
- Origin is GitHub; `legacy-codeberg` points to `caniko/rs-modde` with push disabled.
- Rust MSRV is `1.93`, edition 2024. `.envrc` uses the project flake and a sibling
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

## Historical authorization and qualification (2026-09-30)

- **Edits only remains the user's constraint.** Cargo checks/tests, formatting,
  GUI/game execution, containment checks and benchmarks need new authorization.
  Source inspection and whitespace-only Git diff checks are authorized.
- Last verification: tracked `git diff --check` and checks of all 30 untracked
  files passed, before adding this document. No added regression has run.
- `owned_game_library` and `sandboxed_launch` remain **Partial** in
  `docs/capability-matrix.toml`. Nothing establishes working provider integration,
  containment or sandbox overhead yet.
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
    Bisect baselines must match installation/settings, source profile, enabled mod
    order/versions and a completed successful observed exit. Old bisects without
    `<modde_data>/bisect/<session>.installation.json` must be restarted.

## Code map

| Area | Repository paths |
| --- | --- |
| Shared lifecycle, receipts, hooks, observation | `crates/modde-cli/src/commands/library.rs`, `commands/library/{hooks,process}.rs` |
| CLI dispatch/leases/helper arguments | `crates/modde-cli/src/cli/{args/library.rs,runtime.rs,dispatch/,mutation/}` |
| Catalogue, installation/save contexts, prefixes, sandbox | `crates/modde-games/src/library/{providers,context,launch,runtime,sandbox,operations}.rs` |
| Preferences, identities, journals, save transitions | `crates/modde-core/src/library.rs`, `library/{identity,session,save_transition}.rs`, `paths.rs` |
| Capture/ingestion/benchmarks/bisects | `crates/modde-core/src/performance.rs`, `crates/modde-cli/src/commands/perf.rs`, `commands/bisect/` |
| GUI handlers/settings/race guards | `crates/modde-ui/src/app/{update.rs,update_parts/library.rs,update_parts/navigation.rs,tests/library.rs}`, `src/views/library.rs`, `src/views/library/settings.rs` |
| Manager bridge and durable markers | `crates/modde-manager/src/{main,wiring}.rs` (`.modde-library-session.json`) |
| Fingerprints/dependencies | `crates/modde-games/src/save_fingerprint.rs`, `crates/modde-cli/Cargo.toml` (production `tempfile`) |
| Regression source | `crates/modde-cli/tests/cli_library_{preparation,supervision}.rs`, `crates/modde-games/tests/{installation_context_tests,installation_prefix_tests,store_context_tests,library_sandbox_commands}.rs`, `crates/modde-core/tests/{installation_state_tests,save_transition_tests}.rs` |
| Detailed behavior/status | `docs/src/guides/playing.md`, `docs/src/reference/parity.md`, `docs/capability-matrix.toml` |

## Historical next-work checklist (2026-09-30)

1. Finish source-level signature/type/feature consistency review, especially new
   parser APIs, GUI fixtures, prefix helper and fake-bwrap deployment-refresh test.
   Latest fixes cover deployment re-resolution, re-ingestion exit retention,
   measured frame times, timestamp conversion and custom-symlink UMU grants.
2. Finish Heroic first-run UMU/raw-Proton/fallback-runner tracing across native
   launches, inherited environments, transient overrides and explicit prefixes.
   Heroic `verifyWinePrefix` uses UMU `createprefix` or `wineboot --init`, with Proton
   registry checks under `pfx`; UMU can alias absent `pfx` to the configured root.
3. Audit provider/external-service lifetimes, Wine idleness, Steam/Proton/Flatpak,
   cloud-save boundaries and mount coverage. Flatpak still lacks packaged
   bubblewrap and a host-launch bridge; native flake wiring exists. Alias
   retargeting is guarded without a demonstrated explicit rebinding workflow;
   low-level `ProfileManager::*_scoped` methods remain unjournalled.
4. Finish interruption/retry audit: receipts, nested bisect restoration, late
   workers, skip-analysis, manager-marker cleanup, manual ingestion, GUI races and
   measured-frame-time provenance in bisect baselines. Update docs for any fixes.
5. Once execution is authorized: treefmt, focused compilation/tests with minimal
   and full feature sets, then live provider/save-continuity/containment/nested-runtime
   qualification and repeated paired overhead measurements. The fake-bwrap test
   checks executable selection, not containment. Keep capability status Partial
   until end-to-end qualification supports upgrading it.

Reviewed upstream pins (see playing guide for links):

- Heroic `3934a83a0707baad23cd2c06bc94bd23f51e6622`:
  `src/backend/launcher.ts`, `utils.ts`, `utils/compatibility_layers.ts`.
- UMU `e2b203a1fdd2af9f35166f5713cb3f85d72587e2`: `umu/umu_run.py`.
- MangoHud `73931de948402caf3ea80f26d5bbb6b3f1d2c1d9`:
  `src/logging.cpp`, CSV columns and nanosecond timestamps around lines 200–230.
