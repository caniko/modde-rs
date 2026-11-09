# modde vision

modde is a Linux-first game mod manager written in Rust, with experimental macOS
and Windows builds. It manages installs, load order, profiles, saves, and
conflicts for fifteen games across the Creation Engine, Gamebryo, REDengine,
Unreal 4/5, Larian, SMAPI, and Bannerlord engines, and lets users define
additional games at runtime. Both a CLI and a desktop UI drive the same core.

## Problem

Modding is treated as a Windows-only activity. Existing managers assume Windows
APIs, the registry, and a single game directory that is edited in place, so Linux
users end up running installers under Wine/Proton, hand-copying files, and
losing the ability to roll back. Installs that overwrite the game directory are
irreversible, plugin conflicts and load order are opaque, and swapping mod sets
can break saves that were never tracked.

## Principles

- **The game directory stays clean.** Deployment is a symlink farm over an
  unmodified installation, with atomic rollback to a previous deployment.
- **Profiles are experiments.** Profiles can be created, forked, switched, and
  deleted, and stackable experiments roll back like git branches, with load order
  locking.
- **Saves are versioned.** Save vaults are git-backed with SHA-256 fingerprints,
  compatibility warnings, and auto-capture where a game has a real save tracker.
- **Conflicts are analysed, not listed.** A graph-based collision analysis
  classifies dangerous versus cosmetic conflicts and suggests resolutions, and
  diagnostics use the real plugin order plus resolved conflicts rather than
  placeholder inputs.
- **Claims have a baseline.** `docs/capability-matrix.toml` is the canonical
  status for what each supported title actually does, and the README distinguishes
  `Done` from `Partial` per game.
- **Linux paths first, other platforms honestly.** Proton and Heroic installs are
  detected as the primary route; macOS and Windows builds are described as
  experimental.

## Scope

- virtual filesystem deployment and rollback
- Nexus Mods installs (REST and GraphQL), `nxm://`, Wabbajack modlists, and
  Nexus Collections; GitHub, Direct, Google Drive, MEGA, and MediaFire backends
  mainly for Wabbajack and directive installs
- FOMOD installer, with BAIN detection still requiring user selection
- profiles, load order, and save management
- conflict detection and diagnostics
- game tool and executable configuration (MangoHud, vkBasalt, GameMode, ReShade,
  OptiScaler, Proton, external tools with overwrite capture)
- CLI, Iced desktop UI, and per-game modules under `crates/`
- distribution packaging across several package managers

## Direction

The unreleased changelog covers site builds from a qualified Plinth revision and
a dependency-safety backport that removes a vulnerable `lru` path while keeping
the current graphics API. Recent commits are a broad test-coverage push across
CLI, games, sources, and core, alongside GUI rough-edge fixes and a performance
fix that stops rescanning game libraries on every redraw.

## Place in the fleet

Canix consumes modde as a pinned flake input and registers the checkout in its
development profile. Canix operations documentation also tracks the release
secret reachability for modde's publishing pipeline.
