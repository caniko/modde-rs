# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Manager**: Add Classic/OctoWoW reconciliation, verified offline addon import,
  private account snapshots, seed-only account-tree merging, and recovery journals.
- **Manager**: Add declarative runtime wiring (presence-tracking `wiring`
  overlay plus a shared `preset = "octowow-hd"` expansion, schema v3) with
  `onboard status|plan|apply`: numeric Wine-runner selection persisted
  across review and execution (fail-closed record, `--reselect`,
  `--expect-runner`), WoW.exe size/digest/LAA verification, HD patch
  native-letter and rename-dodge checks over name-only directory scans,
  endpoint assertions, launcher file proxies, anchored prefix validation
  with symlink rejection, and owned Lutris registration (structural game
  yml with declared prefix, exact runtime, DLL overrides and explicitly
  disabled anti-cheat runtimes, plus the `pga.db` row).
  A shared preparation step validates ownership, database presence and
  runner before any write; apply is content-aware (repeat apply is a
  no-op), restores the yml if the database step fails, keeps unique
  WAL-safe backups, and owns only Wine prefix creation, the Lutris yml,
  and the Lutris row.
- **Manager**: Add `onboard register-launcher`: the launcher itself (usually
  the installer first) as its own Lutris entry with its own sibling prefix,
  through a shared entry plan/execute adapter (`EntrySpec` render, check,
  plan, and execute agree by construction). Ownership is validated before
  prefix creation; the installer must be a regular file with an optional
  streaming digest. Never launches anything.
- **Manager**: Harden onboarding execution: XDG-compliant Lutris locations
  (explicit `XDG_*` wins; hermetic test dirs), bounded PE parsing via
  `e_lfanew`, session-preserving Wine environment with `WINE*` removal
  plus a `wineserver -w` flush wait, paired native/Flatpak
  config-database selection, fail-closed `pgrep` exit handling, and
  streaming capped reads throughout.
- **Manager**: Remove `onboard upgrade`. Evidence review (2026-09-24)
  showed `VanillaFixes.exe` is an 88 KiB MinGW launcher (`CreateProcessW`,
  plus dynamic `LoadLibrary`/`GetProcAddress`, no update/help strings in
  a static ASCII scan, identical hash on both hosts) — running it is
  only known to *launch* the client, never to update it. No supported
  update procedure has been established: the static scan cannot rule
  out updater behavior elsewhere (companion `VfPatcher.dll` role
  unestablished, dynamic resolution present), so the readiness-bypass
  command was deleted rather than kept as a repair path. It had shipped
  in the deployed `449be6b` build (bypass warning present in the
  shipped binary) and is removed here, not "never shipped". Readiness
  and status hints now point at the operator workflow instead: restore
  the operator-confirmed file or re-pin the declaration after
  confirming provenance, then re-check. Client payloads are never
  fetched by the manager and never by the launcher's Install/Verify.
  The declarative game entry executing `octo/VanillaFixes.exe`
  establishes the launch executable only; the client upgrade procedure
  itself remains operator-gated and unestablished.
- **Manager**: Add `onboard gate`: the Lutris game entry's synthesized
  `system.prefix_command` (`modde-manager onboard gate --instance <name>
  --`, composed behind any declared wrapper). It enforces the same game
  launch readiness a native launch enforces, then execs the appended
  command unchanged. A missing `--config` re-execs at most once through
  the PATH `modde-manager` (marker-bounded, self-skipping) so the bare
  gate token finds its config; the deployed wrapper always passes
  `--config` explicitly, so the fallback only fires for a bare raw
  binary.

### Changed

- **Manager**: Explicitly release mutation leases even when forked children retain
  descriptors, and identify missing source-parent prerequisites in diagnostics.
- **Manager**: Plan and apply compare prepared content and preserve unchanged files.
  Safe migration commits require Linux; pruning is refused. Existing addon locks
  without content verification require an explicit import or update. Online update
  retains its 365-day freshness requirement.
- **Build**: Renamed the shared Harbor flake and xtask sources to `harbor-rs`
  while retaining compatibility aliases for downstream inputs.
- **CI**: Updated Crow jobs to run the project Nix and xtask checks used by the
  development environment.
- **Release**: Centralized Debian/Ubuntu APT and Scoop publication in Simit's
  release-stack templates, with `caniko/apt-modde` Pages and
  `caniko/scoop-modde` as the configured downstream repositories.
- **Release**: Local release helpers now delegate to the same Simit APT
  publisher and resolve canix-managed credentials from the current workspace.

## [0.7.0] - 2026-07-10

### Added

- **CLI**: New discovery commands: `tool doctor`, `tool settings`, `tool
  profiles`, `tool sources` — all read-only, all with `--json`.
- **CLI**: `tool doctor --fix` automatically applies the first recommended
  fix command when issues are detected.
- **CLI**: `tool setup` for guided OptiScaler configuration without the GUI.
- **CLI**: `tool show` and `tool diagnose` for inspecting saved/effective
  config and GPU-specific diagnostics.
- **CLI**: `--dry-run` flag for `tool setup` and `tool apply` — preview what
  would change without writing or saving.
- **CLI**: `configure --reset-key <key>` to restore a single setting to its
  tool-defined default.
- **CLI**: `modde dev completions <shell>` — generates shell completion scripts
  for bash, zsh, fish, powershell, and elvish.
- **OptiScaler**: Global `hardware_tuning` layer auto-selects FSR4 variant
  (`int8_402` for RDNA3, `latest_fp8` for RDNA4).
- **OptiScaler**: `community-dxgi` profile for Cyberpunk 2077.
- **OptiScaler**: Stale proxy cleanup and root-only FSR4 payload validation.
- **Tools guide**: Documented CLI-first workflow for setup, diagnose, and
  hardware tuning.

### Changed

- **OptiScaler**: Removed built-in `community-dxgi-rdna3` profile; stale stored
  references fall back to `community-dxgi`.
- **OptiScaler**: FSR4 variant switching prefers sources with explicit
  `FSR4_INT8/` and `FSR4_LATEST/` directories.
- **CLI**: `tool setup --source auto` is conservative — no upgrade or download
  unless `--upgrade` is passed; reuses cached suitable sources otherwise.
- **Cargo**: Workspace version bumped to 0.7.0; inter-crate dependency pins
  updated to match.

### Fixed

- **OptiScaler**: Scanner no longer classifies backup files
  (`amd_fidelityfx_vk.dll.b`) as unmanaged companions — requires `.dll`
  extension for the `amd_fidelityfx` and `libxess` prefix checks.

## [0.6.0] - 2026-07-06

### Changed

- **modde**: Renamed `modde-cli` crate to `modde` — the CLI binary is now built
  from `crates/modde` and published to crates.io as `modde`. All internal and
  documentation references updated.
- **modde-ui**: Embedded app window icon from `dist/com.tartanoglu.modde.png`.

### Added

- **Packaging**: Desktop file, AppStream metainfo, and app icon installed for
  Nix, RPM, and Debian packages; Chocolatey metadata extended with license,
  icon, docs, bug tracker, and project source URLs.
- **Release**: Release notes now extract only the current version's section
  from CHANGELOG.md instead of posting the entire file.
- **Release**: Windows packaging (Chocolatey, Scoop, Winget) delegated to
  `simit dist windows publish`.

### CI

- **CI**: Consolidated per-crate CI workflows into single jobs on
  `atlas-nix-trusted` (removed multi-tier `codeberg-tiny`/`codeberg-medium`/
  `codeberg-small` orchestration).
- **CI**: Replaced `publish-crate-modde-cli.yaml` with
  `publish-crate-modde.yaml` targeting the renamed `modde` crate.
- **Release**: SRPM artifacts stored at `srpms/` instead of
  `target/modde-release/root-artifacts/srpms/`.

### Fixed

- **modde-games**: Use `Result::is_ok_and` over `ok().is_some_and` in Heroic
  launcher detection.

## [0.5.0] - 2026-06-15

### Changed

- **modde-games**: Split large monolithic files (`detection.rs`, `launcher.rs`,
  `registry.rs`, `tools/mod.rs`, `tools/mangohud.rs`, `traits.rs`) into focused
  submodules.
- **modde-oracle**: Split monolithic `lib.rs` into dedicated modules for
  aggregate, app, error, postgres, rate_limit, store, and tests.
- **modde-ui**: Split large files (`app.rs`, `update.rs`, `action_button.rs`,
  `screenshot.rs`, `sidebar.rs`, `tools.rs`, `wabbajack.rs`, `tests.rs`) into
  focused submodules.

### CI

- **Release**: Relocated COPR Makefile from `.copr/Makefile` to
  `dist/copr/Makefile`, simplified the Makefile build, removed `debug_package`
  and cargo profile overrides from RPM spec, added APT signing key.

### Fixed

- **Lint**: Added `#![allow(clippy::wildcard_imports)]` and normalized import
  formatting across `modde-cli`, `modde-core`, and `modde-sources`.
- **modde-ui**: Fixed private function visibility in `screenshot_parts/fixtures.rs`
  and removed unused imports in `screenshot.rs`.
- **modde-cli**: Added missing `std::path::PathBuf` import in `cli/runtime.rs`.
- **modde-games**: Fixed broken intra-doc links in `tools/settings.rs`,
  `traits/deploy.rs`, and `traits/game_plugin.rs` after module splits.

## [0.4.0] - 2026-06-13

### Added

- **CLI/Core**: Added portable `modde.lock` export, signing, verification, and
  import workflows for reproducible profile snapshots.
- **CLI/Core**: Added `modde bisect` and `modde perf` workflows for isolating
  bad mods and comparing profile performance against recorded baselines.
- **CLI/Core**: Added patcher pipeline validation, single-stage execution,
  reorder support, timeouts, cache hits, and failed-run output rollback.
- **CLI/Core**: Added experimental Cyberpunk 2077 hot-deploy support for
  patching cosmetic mods into a live VFS without a full redeploy.
- **Diagnostics**: Added crash-log correlation and `modde doctor explain`
  support that connects local Crash Logger SSE or Trainwreck evidence to the
  managed profile state.
- **Games**: Added native Bethesda record-reference validation for Skyrim,
  Fallout, and Starfield plugin orders before persisting unsafe changes.
- **Telemetry**: Added an opt-in compatibility oracle reporting path with
  hashed crash signatures, hashed mod identities, retry storage, and
  cohort-threshold query suppression.

### Changed

- **Release ops**: Hardened stable distribution automation for Codeberg release
  assets, Debian/APT, AUR, COPR, Scoop, Chocolatey, Homebrew, winget, Flatpak,
  AppImage, and tarball outputs.

### Fixed

- **Database**: Added the missing diagnostics migration tables needed by the
  crash and doctor workflows.
- **CLI**: Hardened command parsing, crash-action handling, download queue
  sidecar status parsing, and doctor help snapshots.

### Documentation

- **Docs**: Cleaned public install-channel wording so live Nix/Home-Manager
  paths are not confused with staged package-manager channels, fixed stale CLI
  examples, and moved internal planning notes out of the public mdBook source.
- **Docs tooling**: Added `cargo xtask docs-validate` to build mdBook docs,
  check local Markdown links, and reject known-stale command examples.
- **Starfield**: Clarified `.sfs` save capture and the save-contamination
  removal gate in the README, supported-games table, and parity reference.

## [0.3.8] - 2026-06-08

### Fixed

- **Release ops**: Regenerated the release workflow so unsigned Windows archive
  packaging does not rely on `OLDPWD` under strict shell mode, and regenerated
  CI workflows so release tags do not queue per-crate checks ahead of the
  artifact publisher.

## [0.3.7] - 2026-06-08

### Fixed

- **Release ops**: Regenerated the release workflow so unsupported Debian
  package chroot setup warns and continues, allowing Codeberg release assets to
  publish before optional `.deb`/APT artifacts.

## [0.3.6] - 2026-06-08

### Fixed

- **Release ops**: Regenerated the release workflow with a Debian packaging
  step that works on rootful trusted runners without requiring `sudo`, so
  `.deb` asset creation can reach Codeberg release publication.

## [0.3.5] - 2026-06-08

### Fixed

- **Release ops**: Made Codeberg release asset publication run before optional Attic cache pushes so missing runner cache tokens cannot block downloadable release files.

## [0.3.4] - 2026-06-07

### Fixed

- **Release ops**: Retried the release after the trusted runner service was
  restarted during the `0.3.3` artifact build, cancelling the job before
  Codeberg release publication.

## [0.3.3] - 2026-06-07

### Fixed

- **Release ops**: Align the RPM source archive name with the generated
  release workflow so SRPM creation can find the local source archive.

## [0.3.2] - 2026-06-07

### Fixed

- **Release ops**: Allow the workspace's `GPL-3.0-only` license expression in
  the release supply-chain policy.

## [0.3.1] - 2026-06-07

### Fixed

- **Release ops**: Updated the committed minisign public key to match the
  Codeberg release signing secret so checksum signing passes preflight.

## [0.3.0] - 2026-06-03

### Added

- **Core/CLI**: Added an async SQLx-backed database layer with SQLite by
  default, optional PostgreSQL support, migrations, parity tests, and
  `modde config` commands for inspecting and setting the database backend.
- **Release ops**: Added reusable `nix run .#sign-release` and
  `nix run .#verify-release` minisign helpers for local/CI release-signature
  parity.

### Changed

- **GUI**: Moved profile, tool, and settings writes off the iced render thread
  so UI update handlers dispatch database work asynchronously.
- **Release ops**: Regenerated simit-managed crate CI/publish workflows and
  added the generated pre-commit hook module.
- **Release ops**: Renamed the Debian/Ubuntu APT publication repository from
  `caniko/rs-modde-apt` to `caniko/apt-modde`.

### Fixed

- **Release ops**: Hardened Codeberg release dispatch and publish workflows so
  artifact builds run from the validated signed tag and keep the dispatch target
  on `trunk`.
- **Builds**: Fixed Windows release builds and macOS cross-builds, including
  sandboxed SDK wiring, OpenSSL-free local git usage, and osxcross
  `codesign_allocate` handling.

## [0.2.1] - 2026-05-31

### Added

- **Homebrew**: Tap available at `brew tap caniko/modde https://codeberg.org/caniko/homebrew-modde.git`.
- **Release ops**: Release CI supports `rc`, `beta`, and `alpha` prerelease tags, marks Codeberg prereleases correctly, skips stable-only publish channels for prereleases, and routes prerelease COPR builds to `caniko/modde-rs-testing`.
- **Release ops**: Stable releases can announce to Mastodon and Matrix with the release URL and a short changelog excerpt.
- **CLI/UI**: `modde update check` can check for a newer modde release, and the GUI shows a non-modal update banner when one is available.

### Changed

- **Internal**: Homebrew tap publish step in release CI now generated by
  `simit init-ci --with-homebrew`; behaviour unchanged.
- **Release ops**: Release CI now fails early when the tag does not have a matching dated `CHANGELOG.md` section.

### Documentation

- **Release ops**: Documented hotfix, RC, announcement-token, and yank/withdraw release procedures.

## [0.2.0] - 2026-05-19

### Added

- **Skyrim/Wabbajack**: Support Wabbajack `GameFileSourceDownloader` entries used by Legends of the Frost, with local game-file verification through `--game-dir`.
- **Nix**: Home Manager Wabbajack profiles now fetch, install, and deploy declarative modlists, including explicit `gameDir` support.
- **Nix**: Home Manager profiles can now wait non-fatally for a game install through `installMode = "await-game"` or missing `gameDir` prerequisites.
- **Tests**: Hardened Wabbajack game-file-source regressions and the Nix sandbox-sensitive deploy pipeline test.

## [0.1.0] - 2026-04-13

### Added

- **Core**: SQLite-backed profile and mod database with VFS (symlink farm) deployment
- **Core**: Content-addressed mod store with per-file hiding and conflict detection
- **Core**: Profile management with forking, experiment mode (try/rollback/commit), and load order locking
- **Core**: Save management with Git-backed vaults, fingerprinting, and auto-capture
- **Core**: FOMOD installer integration via fomod-oxide (declarative config support)
- **Games**: Bethesda support (Skyrim SE/AE, Fallout 4, Fallout 76, Starfield) with plugins.txt, LOOT sorting, INI management, BSA/BA2 indexing
- **Games**: Cyberpunk 2077 support (REDmod, CET, TweakXL, REDscript, conflict detection, save tracking)
- **Games**: Stellar Blade support (UE4 framework, experimental)
- **Games**: Auto-detection via Steam (Proton) and Heroic (GOG/Epic) launchers
- **Games**: Mod scanning with game-specific filesystem discovery and Wabbajack manifest matching
- **Sources**: Nexus Mods API v1 client (search, trending, mod details, CDN downloads, collections)
- **Sources**: Wabbajack modlist parsing and native installation (no Windows VM required)
- **Sources**: Direct URL, GitHub releases, MEGA, and Google Drive download backends
- **Sources**: BAIN installer support
- **Sources**: nxm:// protocol handler with XDG desktop integration
- **CLI**: 24 top-level commands with 60+ subcommands covering the full modding workflow
- **GUI**: Iced-based GUI with 20+ views (mod list, downloads, saves, settings, FOMOD wizard, Nexus browser, diagnostics, tools)
- **Tools**: Integration with MangoHud, vkBasalt, GameMode, ReShade, and OptiScaler
- **Diagnostics**: Form 43 detection, missing master detection, shadowed mod detection, load order validation
- **Nix**: Flake with binary, docs, and website outputs; home-manager module for declarative configuration

[Unreleased]: https://github.com/caniko/modde-rs/compare/0.7.0...HEAD
[0.7.0]: https://github.com/caniko/modde-rs/compare/0.6.0...0.7.0
[0.6.0]: https://github.com/caniko/modde-rs/compare/0.5.0...0.6.0
[0.5.0]: https://github.com/caniko/modde-rs/compare/0.4.0...0.5.0
[0.4.0]: https://github.com/caniko/modde-rs/compare/0.3.8...0.4.0
[0.3.4]: https://github.com/caniko/modde-rs/compare/0.3.3...0.3.4
[0.3.3]: https://github.com/caniko/modde-rs/compare/0.3.2...0.3.3
[0.3.2]: https://github.com/caniko/modde-rs/compare/0.3.1...0.3.2
[0.3.1]: https://github.com/caniko/modde-rs/compare/0.3.0...0.3.1
[0.3.0]: https://github.com/caniko/modde-rs/compare/0.2.1...0.3.0
[0.2.1]: https://github.com/caniko/modde-rs/compare/0.2.0...0.2.1
