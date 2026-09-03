# Architecture

This is the system-design reference for modde — the durable "why" and "how" behind
the pieces that make up the application. It is meant for contributors and for anyone
who wants to understand a subsystem before changing it. Where a behaviour is settled
and load-bearing, the rationale lives here so it does not have to be re-derived from
the source each time.

Implementation history lives in `git log`; this document records the design that the
code currently encodes. When the two disagree, the source is the truth — please file
a fix.

## Crate map

modde is a Cargo workspace. Five crates carry the weight, layered so that the lower
crates know nothing about the higher ones.

**`modde-core`** is the engine. It owns the persistent data model (the SQLite database
and the XDG path layout), the load-order resolver, the collision engine, the VFS
symlink farm, the installer pipeline, the save vault, the Wabbajack manifest model and
its native install/extract path, hashing, and the content-addressed mod store.
`modde-core` deliberately contains **no game-specific knowledge** — every place that
would otherwise hard-code a game's quirks instead takes a trait object or a callback so
the game layer can supply the behaviour. This is what keeps the core testable in
isolation and lets new games land without touching it.

**`modde-games`** is the game knowledge layer. It defines the `GamePlugin`,
`ModScanner`, and `SaveTracker` traits, the central `GAME_REGISTRY`, launcher detection
(Steam and Heroic), and one module per supported engine family (Bethesda, Gamebryo,
Cyberpunk RED, Larian, UE4/UE5, Witcher, Bannerlord, SMAPI) plus the generic
user-defined game loader. It depends on `modde-core` and supplies the trait
implementations the core calls back into. Fifteen built-in games ship in the registry.

**`modde-sources`** is the acquisition layer. It implements download backends (direct
HTTP with resume, Google Drive, MediaFire, Nexus, Wabbajack-authored CDN chunks), the
native archive decompressors (zip, 7z, BSA, BA2, optional RAR), the Wabbajack installer
and its per-archive batching, the inline-zip index, the byte LRU cache, and the
streaming verify path. It depends on `modde-core` for the manifest model, hashing, and
the link/store helpers.

**`modde`** is the command surface. Each subcommand is a thin handler that
wires user input to a core/games/sources call: `install`, `deploy`, `profile`, `scan`,
`exec`, `tool`, `save`, `wabbajack`, `loot`, `game`, and friends. It owns argument
parsing and human-readable reporting but very little logic of its own — the goal is that
anything worth testing lives below the CLI.

**`modde-ui`** is the egui desktop application (`modde-ui`, also reachable as
`modde gui`). It renders the mod list, load order, conflict views, the Nexus browse
panel, the Executables view, and the install wizard against the same core APIs the CLI
uses. UI and CLI are two front-ends over one engine; shared flows (deploy, install) are
factored into the core rather than duplicated.

```text
        ┌────────────┐      ┌────────────┐
        │   modde    │      │  modde-ui  │      front-ends
        └─────┬──────┘      └─────┬──────┘
              │                   │
              ├───────────────────┤
              ▼                   ▼
        ┌──────────────┐   ┌──────────────┐
        │ modde-games  │   │ modde-sources│   knowledge + acquisition
        └──────┬───────┘   └──────┬───────┘
               │                  │
               └────────┬─────────┘
                        ▼
                 ┌────────────┐
                 │ modde-core │              engine + data model
                 └────────────┘
```

## Data model

### The SQLite database

modde's persistent state is a single SQLite database at `<modde_data>/modde.db`, opened
in WAL mode with foreign keys enforced. Schema evolution is handled by an idempotent
migration ladder keyed off SQLite's `user_version` pragma — each step runs only if the
stored version is below it, and additive column changes go through an
`add_column_if_missing` guard so re-running a migration is always safe. **The current
schema version is 10.**

The key tables:

| Table | Holds |
| ----- | ----- |
| `profiles` | One row per profile: `name`, `game_id`, source type/data (manual, Nexus collection, Wabbajack…), the overrides directory, and the profile-level load-order lock. `UNIQUE(name, game_id)`. |
| `profile_mods` | The mod list for a profile, ordered by `sort_index`. Carries enabled state, version, display name, FOMOD config, Nexus IDs, category/notes/tags, the per-mod load-order lock reason, and the installer-pipeline columns (`install_method`, `source_archive_hash`, `install_status`). |
| `load_order_rules` | `LoadAfter` / `LoadBefore` / `Incompatible` constraints between mods, consumed by the resolver. |
| `installed_mod_files` | The exact file manifest for every installed mod: `rel_path`, `origin_rel_path`, `size`, and a reserved `merge_group`. This is what makes uninstall surgical — modde removes exactly the files it staged. |
| `saves` | Save files/directories assigned to a profile (the DB-level assignment layer, distinct from the git save vault). |
| `stock_snapshots` | Per-game vanilla-game snapshot metadata: snapshot path, tree hash, and file count, used by Stock-Game / clean-baseline flows. |
| `active_profiles` | The currently active profile per game (`game_id` primary key). |
| `experiment_stack` | A per-game stack of profiles for the "try a change, then pop back" experiment workflow. |
| `hidden_files` | Per-profile, per-mod file hides (the MO2 `.mohidden` equivalent) — excluded from the symlink farm at build time. |
| `plugin_order` | Plugin (`.esp`/`.esm`/`.esl`) ordering and enable state, kept independent of mod install priority. |
| `mod_categories` | Collapsible category separators with colour and sort index. |
| `game_tools` | Current per-game tool/overlay state (MangoHud, vkBasalt, GameMode, OptiScaler, ReShade, Proton…): enabled flag plus a free-form JSON `settings` blob. |
| `tool_applied_files` | Files a tool wrote into a game directory, tracked so tool changes can be reverted. |
| `executable_configs` | MO2-style named launch targets: path, args, working dir, environment, Wine DLL overrides, and the configurable output (overwrite) mod. |
| `tool_setting_nodes` / `tool_setting_edges` | A DAG of tool-setting history (schema V10): `game_tools` stays the current state, and every mutation also appends a node plus an edge from the previous current node, so restores can branch. |

Two design choices are worth calling out because they recur:

- **TOML/JSON-encoded columns.** Structured values that the database does not need to
  query on — `InstallMethod`, the load-order lock, FOMOD config, tool settings — are
  serialized into text columns. The type-safety contract is enforced at the Rust
  boundary on read/write, not by SQL. This keeps the schema stable as those structures
  evolve.
- **Install + manifest atomicity.** `record_install` writes the method, archive hash,
  and status onto the `profile_mods` row *and* replaces the `installed_mod_files`
  manifest in one transaction, wiping any prior manifest first so retries never leave
  orphaned rows. `remove_installed_mod` is the symmetric operation: it returns the
  staged files and deletes both the manifest and the mod row together.

### XDG path layout

`modde-core::paths` is the single source of truth for where things live. It is
platform-aware (honouring `XDG_DATA_HOME` / `XDG_CONFIG_HOME` / `XDG_CACHE_HOME` on
Linux, with the equivalent `dirs` fallbacks on macOS and Windows) and supports a data-dir
override — either via `--data-dir` / `MODDE_DATA_DIR`, or via an active instance recorded
in the instance registry — so portable and multi-instance setups work without touching
the rest of the code.

```text
<data_dir>/modde/                      e.g. ~/.local/share/modde/
├── modde.db                           SQLite (see above)
├── store/                             content-addressed mod file store
├── staging/                           scratch space for installs
├── profiles/<name>/staging/           per-profile symlink farm
├── downloads/                         downloaded archives (durable across restarts)
├── stock/                             vanilla-game snapshots
├── saves/<game_id>/                   per-game git save vault
└── wabbajack_cache/<hash>.wabbajack   content-addressed .wabbajack manifests

<config_dir>/modde/                    settings (app settings, game-path overrides)
<cache_dir>/modde/                     non-essential cache
```

The store, downloads, and save vaults are all durable across restarts; staging is
disposable. This separation is what lets an interrupted install resume without
re-downloading and lets a botched deploy roll back without losing anything.

## The `GamePlugin` trait and registry

A supported game is described by a `GameRegistration` in the static `GAME_REGISTRY`. The
registration is pure data — it binds a `game_id` to a display name, an `EngineFamily`,
launcher IDs (Steam app/dir, Heroic GOG/Epic IDs), Wabbajack manifest names, Nexus
domain and numeric game ID, a `supports_save_profiles` flag, and the trait objects that
implement behaviour:

```rust
pub struct GameRegistration {
    pub game_id: &'static str,
    pub display_name: &'static str,
    pub engine: EngineFamily,
    pub launcher: LauncherIds,
    pub wabbajack_names: &'static [&'static str],
    pub nexus_domain: Option<&'static str>,
    pub nexus_game_id: Option<u32>,
    pub supports_save_profiles: bool,
    pub plugin: &'static dyn GamePlugin,
    pub scanner: Option<&'static dyn ModScanner>,
    pub save_tracker: Option<&'static dyn SaveTracker>,
    pub collision_classifier: Option<CollisionClassifierFactory>,
    pub optiscaler_profiles: &'static [OptiScalerProfile],
}
```

The `GamePlugin` trait is the behavioural contract. It is deliberately wide but almost
entirely default-implemented, so a new game overrides only what is genuinely
game-specific. The salient methods:

- **Install layout.** `mod_directory` / `mod_root` (where mods deploy), `executable_dir`
  (where proxy DLLs live), `deploy` / `deploy_to_install` / `post_deploy`.
- **Install-method detection.** `analyze_mod_archive` runs *before* the generic installer
  probes so a game can authoritatively claim a layout it recognises (Cyberpunk's REDmod
  via `info.json`, for example); `recognizes_bare_layout` is the last-resort fallback
  before the analyzer emits `Unknown`.
- **Classification.** `classify_extension` / `summarize_content` bucket files into
  `ContentCategory`; `classify_mod` decides whether a mod is save-breaking (feeding the
  save fingerprint).
- **Alternate deploy targets.** `deploy_targets` advertises named roots outside the game
  install dir (per-user config, saves), and `resolve_deploy_target` resolves an ID to a
  real path at deploy time — deferred so plugins can fold in runtime context like a Wine
  prefix or Steam `compatdata`.
- **Wine integration.** `wine_dll_overrides` / `wine_dll_overrides_from_staging` report
  proxy/hook DLL base names that need `WINEDLLOVERRIDES=name=n,b`.
- **Metadata accessors.** `nexus_game_domain`, `nexus_game_id_u32`, `steam_app_id_u32`,
  `archive_extensions`, `has_plugin_system`, `plugins_txt_folder`, and so on, which let
  the core and CLI stay game-agnostic.

The registry is wrapped in an `OnceLock<RwLock<&'static [GameRegistration]>>`. The first
access builds a snapshot from the static built-ins **chained with user-defined games**
loaded from disk; `reload_registry` rebuilds it so newly added user games appear without
a restart. `resolve_game`, `all_games`, `resolve_game_by_nexus_domain`, and
`launcher_games` are the lookups everything else uses.

### Adding a built-in game

A new built-in game is a new `GameRegistration` appended to `GAME_REGISTRY` plus the
trait implementations it points at. In practice:

1. Implement (or reuse) a `GamePlugin` for the engine family. Most games reuse a shared
   data-driven plugin (the Bethesda and UE4 modules are parameterized) rather than a
   bespoke type.
2. Optionally implement a `ModScanner` (filesystem discovery of already-installed mods)
   and a `SaveTracker` (save detection + capture description). Both are `Option` in the
   registration — a game can ship without them.
3. Pick a `collision_classifier` factory (or one of the shared policy classifiers keyed
   by archive extension).
4. Add the `GameRegistration` with launcher IDs and Nexus/Wabbajack identifiers.

The per-game documentation lives under [Supported games](../games/supported-games.md).

### User-defined games

Generic / user-defined game support is **Partial**. A user can describe a game with a
`GameSpec` TOML and register it via `modde game add` (and import/export the spec); the
generic loader turns that spec into a `GameRegistration` that the snapshot picks up
through the same `load_user_games` chain. Deployment, conflict analysis, and launcher
wiring work for these games, but there is no bespoke scanner or save tracker behind a
generic spec, and the UX is thinner than for a built-in. See
[Generic & user-defined games](../games/generic-games.md) for the spec format and its
limits.

## Detection

Detection answers "where is this game installed?" without asking the user. It scans the
two launchers modde understands and caches the result.

**Steam.** modde reads `steamapps/libraryfolders.vdf` to enumerate every library root
(Steam supports libraries spread across drives), and the platform-specific default
library is always included. Within each library's `steamapps/` directory it parses every
`appmanifest_*.acf` for its `appid` / `name` / `installdir`, matches the app ID against
the registry's `LauncherIds`, and resolves the install to
`steamapps/common/<installdir>`. A `common/<steam_dir>` directory-name fallback catches
installs whose manifest is missing or unreadable. The VDF and ACF parsers are
intentionally lightweight line scanners rather than full KeyValues parsers — they extract
only the quoted values they need and ignore everything else.

**Heroic.** modde reads Heroic's config directory and three store databases:
`gog_store/installed.json`, `legendary_store/installed.json` (Epic), and
`sideload_apps/installed.json`. GOG and Epic entries are matched by their store app ID
against the registry; sideloaded apps are matched heuristically by install-directory name
against each game's known `steam_dir`. Each match records its `LauncherSource` so the
front-ends can both display provenance and launch the game (Steam via a
`steam://rungameid/<id>` URI, Heroic via `heroic --no-gui --launch <id>`).

Results flow through a process-wide detection cache so the UI can resolve many games
without rescanning every launcher per game. `find_game_install` consults a settings
override first, then the cache, before falling back to a fresh scan.

## VFS symlink farm

Deployment is a **symlink farm**: rather than copying mod files into the game directory,
modde builds a staging tree of symlinks pointing into the content-addressed store, then
deploys that tree into the game's mod directory as a second layer of symlinks. Nothing is
copied; toggling a mod or reordering load order is cheap.

The farm uses a **typestate** to make misuse a compile error. `SymlinkFarm<S>` carries a
zero-sized `PhantomData<S>` marker that is `Built` after construction and `Materialized`
after it is written to disk. `build()` returns a `Built` farm; `materialize()` consumes
it and returns a `Materialized` farm; only a `Materialized` farm exposes `deploy_to()`.
You cannot deploy a farm you have not materialized, and the compiler enforces it.

```text
build()  ──►  SymlinkFarm<Built>
                   │  materialize()  (writes staging symlinks to disk)
                   ▼
              SymlinkFarm<Materialized>
                   │  deploy_to(game_mod_dir)
                   ▼
              game directory populated with symlinks
```

**Build is priority-aware.** `build()` walks the resolved load order in order, inserting
each mod's `(rel_path → store_path)` links into a map. Because later mods overwrite
earlier entries for the same relative path, **the last mod in load order wins** a file
conflict — the same rule the collision engine reports on. Profile-level overrides are
layered last and win over all mods, and hidden `(mod_id, rel_path)` pairs are skipped
entirely.

**Store link strategy.** Links point at the content-addressed store (and, for overrides,
at the override directory). Materialization cleans any existing staging directory,
recreates the parent directory tree for each link, and creates the symlinks; deployment
recreates the target tree and replaces any pre-existing file at each destination before
linking. The store is the single source of truth for bytes; the farm is a cheap,
disposable view of it.

**Atomic rollback.** Before a risky redeploy, the previous staging tree is kept as
`staging.bak`. `rollback()` performs a rename-based swap: the current `staging` is moved
aside to `staging.old`, `staging.bak` is renamed into place as `staging`, and the old
tree is removed. Renames within a directory are atomic on POSIX filesystems, so a profile
is never left with a half-built staging tree.

## Load-order resolver

The resolver turns a profile's mod list plus its `LoadOrderRule`s into a single
deterministic order. It is a **stable Kahn topological sort** with input-position
tiebreaking.

The mod and game identifiers it works with — `ModId` and `GameId` — are
`#[repr(transparent)]` newtypes over `String`, generated by a macro. They are zero-cost
at runtime but make it a type error to pass a mod ID where a game ID is expected.

**Rule types.** Three rules constrain the order:

- `LoadAfter { mod_id, after }` — `mod_id` must come after `after`.
- `LoadBefore { mod_id, before }` — `mod_id` must come before `before`.
- `Incompatible { mod_a, mod_b }` — error if both are enabled.

`Incompatible` rules are checked up front and short-circuit resolution with a conflict
error. `LoadAfter` / `LoadBefore` become directed edges; edges that reference a disabled
or unknown mod are silently dropped.

**Algorithm.** Enabled mods are collected in input order, each recording its input
position. Adjacency and in-degree are built from the rules. A min-heap keyed on input
position seeds every zero-in-degree node; the resolver repeatedly pops the
lowest-input-position ready node, emits it, and decrements its successors, pushing any
that reach zero in-degree.

```text
enabled mods (input order) ──► in-degree + adjacency from rules
                                   │
                                   ▼
        min-heap of ready nodes (keyed by input position)
                                   │  pop lowest, emit, relax successors
                                   ▼
                          resolved load order
        (fewer emitted than enabled ⇒ cycle ⇒ DependencyCycle error)
```

**Determinism and stability guarantees.** The input-position tiebreak gives four
properties the UI relies on:

1. **No rules → exact input order.** With no rules, the resolved order is exactly the
   enabled subset of the mod list, unchanged.
2. **Reorder round-trips.** Swapping two adjacent mods in the list and re-resolving
   produces the swapped order — so a drag-reorder in the UI is actually visible (a plain
   topological sort could return any valid order and silently discard the move).
3. **Minimal change under rules.** When a rule forces movement, only the rule-involved
   mods shift; unrelated neighbours stay put.
4. **Deterministic.** Identical inputs always produce identical outputs; no `HashMap`
   iteration order leaks into the result.

**Cycle detection.** If fewer nodes are emitted than were enabled, some node never
reached zero in-degree — a cycle. The resolver names one surviving node in a
`DependencyCycle` error rather than silently truncating the order.

## Collision engine

The collision engine reports which mods provide the same file, who wins, and how risky
the overlap is. It builds on the resolver's `ConflictMap` (a `file_path → set of
provider mods` index whose `winner_for` applies the same last-in-load-order-wins rule as
the symlink farm).

**Graph of file providers.** `build_full_conflict_map` walks each mod's store directory
in resolved order, registering every loose file. It is **archive-aware**: for files whose
extension marks them as an archive (per the game's `CollisionClassifier`), it indexes the
archive's contents and registers those inner paths too, tracking each entry's
`FileOrigin` — `Loose` or `Archive { archive_rel }`. This is what lets modde detect that
a loose texture shadows the same texture packed inside another mod's BSA, which a
loose-file-only diff would miss. Mods whose store directory is missing are collected and
reported rather than silently skipped.

**Severity classification.** Each colliding file is classified by the game's classifier
into `Cosmetic` (textures, meshes, sounds — low risk), `Config` (INI/config — may change
behaviour), `Dangerous` (scripts, plugins, DLLs — potential crashes or save corruption),
or `Unknown`. Severity is an ordered enum, so a mod-pair's worst collision is just the
max.

**The report.** `analyze_collisions` produces a `CollisionReport` that groups collisions
by `(loser, winner)` mod pair (sorted most-severe-first, then by file count), flags
`loose_vs_archive` overrides, lists `redundant_files` (files a mod provides but always
loses), and identifies fully `shadowed_mods` (every file they provide is overridden by a
higher-priority mod). Hidden files are honoured in winner selection. The classifier is a
trait so the severity tables and archive formats stay in the game layer; the analysis is
game-agnostic.

## Save vault

modde gives every game a **git-backed save vault** at `<modde_data>/saves/<game_id>/`.
Profiles map to branches, so branching, history, and stacking come for free from git.
The core `SaveManager` drives the vault; the game's `SaveTracker` tells it *what* to look
for. Saves are committed via `git2`, and capture skips committing when the tree is
identical to `HEAD` so no-op captures do not litter history.

The activate flow captures the current profile's saves (committing to its branch), parks
them in a live `.modde/profiles/<name>/` directory so Steam Cloud sees a move rather than
a delete, then checks out and deploys the new profile's branch. Steam Cloud markers and
modde's own live-state metadata are preserved across capture/deploy/restore so the
launcher's cloud sync is never confused.

**Fingerprinting.** A `SaveFingerprint` is a SHA-256 over the sorted, de-duplicated list
of *enabled, save-breaking* mod IDs. "Save-breaking" is decided by the game plugin's
`classify_mod` (passed in as a callback so the core stays game-agnostic). The fingerprint
is written into the capture commit message as `Mod-Fingerprint:` /
`Save-Breaking-Mods:` trailers. Before restoring an older snapshot, modde re-derives the
current fingerprint and compares: a match is `Compatible`, a missing trailer is
`NoFingerprint`, and a difference is a `Mismatch` listing exactly which save-breaking
mods were added or removed — so the user is warned before loading a save whose mod set no
longer matches. Cosmetic mods do not affect the fingerprint, so freely toggling textures
never triggers a false warning.

## Installer pipeline

Installing a mod is a three-stage pipeline: **analyze → execute → record**.

1. **Analyze** inspects an extracted archive and produces an `InstallPlan` describing how
   to lay its files out (the `InstallMethod`), optionally stripping a wrapper directory
   (`strip_prefix`), and carrying the source archive's xxh64 hash.
2. **Execute** moves files from the staging directory into the mod's store directory per
   the plan, producing the concrete `StagedFile` manifest.
3. **Record** persists the method, archive hash, status, and file manifest into the
   database (atomically, as described in the data-model section) so uninstall is precise.

**`InstallMethod` variants.** The enum is the extensibility point. It is `serde`-tagged
and ordered by detection specificity (game-specific layouts win over generic ones):

| Variant | Meaning |
| ------- | ------- |
| `BareExtract` | Contents map 1:1 into the game's mod dir. |
| `StripContentRoot { root }` | A content root (e.g. `Data/`) is stripped before staging. |
| `DirectoryMod { directory_name }` | The archive root is one directory-style mod. |
| `DirectoryModFromXml { marker, id_attr, fallback_name }` | Directory mod whose stable name is read from an XML marker (e.g. Bannerlord `SubModule.xml`). |
| `MultiRootOverlay { roots }` | Several game-root overlay directories (`mods/`, `dlc/`, `bin/`…). |
| `SingleFileSet` | One or more loose files that stage straight into the mod root. |
| `Fomod { module_config, config_toml }` | FOMOD installer; `config_toml` is the recorded declarative selection (`None` ⇒ the wizard still has to run). |
| `REDmod { manifest }` | Cyberpunk REDmod package keyed by `info.json`. |
| `Bain { selected_subdirs }` | BAIN (Wrye Bash) numbered option subdirs; empty ⇒ wizard pending. |
| `DllOverlay { target_dir_hint }` | Proxy DLL / overlay (dxvk, ENB) into the executable dir. |
| `UserConfigOverlay { target_id }` | Files routed to a plugin-advertised alternate root (UE config, Bethesda My Games INIs) at deploy time. |
| `ScriptMerge { merge_group, base }` | Reserved: stages per `base` and tags the merge group; actual merging is future work. |
| `Unknown { reason }` | Detection failed; a dossier is written. |

`InstallMethod::is_ready()` reports whether execution can proceed without user input —
FOMOD needs its config, BAIN needs its selection, `Unknown` never proceeds. The matching
`InstallStatus` (`Installed`, `PendingUserInput`, `Failed`, `Unknown`) is persisted so
the UI shows the right action (install / retry / resume).

**The dossier extension mechanism.** When detection cannot classify a layout, the
analyzer emits `Unknown { reason }` and the caller writes a *dossier* — a dump of the
archive's shape and hash. The dossier is the seam by which the installer is taught a new
layout: a maintainer (or a Claude Code skill) extends the `InstallMethod` enum with a new
variant and the detection rule that recognises it, rather than patching ad-hoc special
cases into the analyzer. `modde install dossier` surfaces the path for an undetected mod.

### Designing for slow-and-steady large installs

The Wabbajack apply path (in `modde-sources`) is the stress case: a list like Twisted
Skyrim has on the order of 6,000 archives and 680,000 install directives. The pipeline is
built around four root causes that an order-naive implementation hits, and the durable
design decisions that correct them:

- **Per-archive batching.** Directives are grouped by source archive and applied with
  bounded concurrency over *archives*, not directives. Each archive's decoder is opened
  once and driven forward through its entries in order, which eliminates the
  solid-7z re-decompression amplification (extracting N files from a solid archive must
  not cost N full decompressions). `CreateBSA` runs as a separate pass after the
  `FromArchive` work its inputs depend on.
- **Native, in-process decompression.** zip, 7z, BSA, and BA2 are decoded by Rust crates
  in-process (RAR is an optional `rar` feature; without it RAR is reported as unsupported
  rather than shelling out). No `7zz` / `unrar` subprocess remains in the apply path, so
  there is no per-file `execve`, no re-mmap, and no subprocess stdout to buffer.
- **Streaming verification.** Downloads are hashed *as they are written* (the
  `copy_and_hash_compat` adapter computes the Wabbajack-compatible xxHash64/XXH3 while
  copying), instead of re-reading hundreds of gigabytes in a separate verify pass after
  download. Cached files keep the cheap existing-cache check.
- **Resumable apply.** Each completed archive batch (and each `CreateBSA`) writes a JSON
  sentinel under `<staging>/_state/`. A sentinel is honoured only when the pipeline
  version, archive hash, archive size, directive indices, and expected output files all
  match the current manifest; a crash mid-apply resumes at the next pending batch rather
  than at directive zero. Failed or interrupted batches leave no sentinel and rerun whole.

Supporting pieces — a shared inline-zip index (the `.wabbajack` is opened and indexed
once), a bounded byte-LRU cache for patch source bytes, and hardlink-then-reflink-then-copy
linking for Stock Game and deploy — round out the memory and disk-bounded behaviour. See
the [Wabbajack guide](../guides/wabbajack.md) for the operator-facing view.

## Mod scanner subsystem

The scanner answers the inverse of installation: given a game directory (and optionally a
Wabbajack manifest), *what mods are already installed?* This is what lets a profile with
files on disk but no database rows be reconstructed.

**The `ModScanner` trait.** Each game implements `ModScanner`: `scan_directories` lists
the install-relative directories to inspect, and `scan_filesystem` returns
`DiscoveredMod`s (each with a `mod_id`, display name, optional version, file list,
`ModSource`, and a confidence score). A third method, `mod_id_footprint`, is the inverse
of the scanner's ID scheme — given an ID the scanner would produce, it returns the
filesystem footprint (directory subtree or single file) that mod owns. That inverse is
what lets the dedup pass correlate filesystem-scanner rows against a manifest.

**Three pattern rules.** Game scanners recognise three structural patterns when grouping
files into mods:

1. **One subdirectory = one mod.** Used where a mod loader reads a directory of named mod
   folders (Cyberpunk CET / REDscript / TweakXL mods under their respective roots,
   REDmods under `mods/` whose `info.json` supplies name and version).
2. **One file = one mod.** Used for loose-file mod loaders (e.g. each `.archive` under
   `archive/pc/mod/`).
3. **Plugin + companion archives = one mod.** Used for plugin-based games, where an
   `.esp`/`.esm`/`.esl` and its sidecar `.bsa`/`.ba2` are grouped as a single mod.

**Wabbajack manifest matching.** `match_wabbajack_manifest` (in `modde-core::scanner`) is
the game-agnostic correlator. It groups a manifest's `FromArchive` /
`PatchedFromArchive` directives by source archive hash, normalises the `to` paths
(backslash → forward slash, lowercased, MO2 `mods/<name>/` prefix stripped), and for each
archive computes the fraction of its `to` paths that actually exist on disk. Archives
whose present fraction meets a threshold become `ManifestMatch`es, carrying the archive's
Nexus identity when the directive's `ArchiveState` is a `NexusDownloader`. The crucial
invariant is that `archive_mod_id` derives the **same** canonical `mod_id`
(`nexus_<domain>_<mod>_<file>` or `wj_<hash>`) for both the Wabbajack installer and the
scanner, so installing a list and later re-scanning it dedup against each other instead of
producing duplicate rows.

Two more pure helpers build on this. `apply_wabbajack_lock` reorders a profile to follow
the manifest's install-directive order (matched mods first, unmatched preserved after) and
stamps a Wabbajack lock — the engine behind retroactive locking. `detect_stale_duplicates`
classifies a profile's filesystem-scanner rows into "leaked duplicates" (footprint covered
by the manifest, so a `nexus_*` row already deploys those files) and "genuine additions"
(the user's own additions on top of the list), which powers `modde scan
--prune-duplicates`. See the [scanning guide](../guides/scanning.md) for usage.

## Release, packaging, and tooling decisions

These are the enduring choices about how modde is built, released, and distributed. They
are recorded here so a contributor knows *why* something is the way it is before changing
it.

**Releases go through `simit`.** `cargo xtask release {patch|minor|major|prerelease}`
delegates to `simit release` from the devShell. `simit` standardizes the semver bump,
CHANGELOG promotion, commit, and tag across the maintainer's Rust projects, which prevents
per-project drift. Tags are bare semver with no `v` prefix (`0.2.0`, `1.0.0-rc.1`); the
release workflow triggers on `[0-9]*`. The literal `## [Unreleased]` heading in
`CHANGELOG.md` must stay exactly as-is because simit's promotion logic keys off it. Major
versions are cut whenever they make sense per SemVer — there is no 1.0 milestone and no
"save up breaking changes" policy.

**The release workflow is generated by `simit init release`.** It runs on the trusted
Forgejo Nix runner, uses the runner's preinstalled Nix instead of installing Nix in the
job, and can be manually dispatched with an existing tag through the `version` input. Tag
validation fetches and verifies the requested tag, checks release metadata from that tag's
tree, then builds from the verified tag commit before creating the Codeberg release and
uploading every file under `release/`.

**xtask is a thin per-project binary over a shared library.** Reusable build-tooling logic
lives in `harbor-xtask` (in harbor-rs); modde ships a thin `modde-xtask` binary, invoked as
`cargo xtask`, that wires the project-specific bindings — the RPM spec path (`dist/rpm/modde.spec`),
the COPR vendor tarball, the docs bindings, the Nix package names
(`modde` / `site` / `modde-windows` / `appimage-*` / `flatpak-manifest`), and
`cargo xtask gui`. The shared harbor-rs CLI deliberately does **not** grow top-level
`release` / `copr` / `docs` subcommands, because that would couple it to downstream project
layouts. The RPM spec `Version:` rewrite happens in CI against the tagged tree, not
committed back to trunk.

**CI and the flake stay bespoke.** modde does not run `simit init-ci --check` or
`simit init-flake --check`. The non-release Forgejo workflows and `flake.nix` are
hand-maintained because they model things the generic generators do not: the `.#flatpak-manifest`,
`.#appimage-*`, `.#modde-windows`, and `.#docs` builds; the Attic closure push; the
coverage gate (`cargo xtask coverage --ci`); cross-compilation (Windows, aarch64-linux,
macOS aarch64 via osxcross); the Rust-generated website and mdBook docs outputs; and the
self-hosted `atlas` runner. Running those checks would flag all of that intentional
customization as drift. This is revisited only if simit becomes configurable enough to
express those jobs, or harbor-rs publishes a helper that preserves them.

**Home-Manager `tools` contract.** The `programs.modde.profiles.<name>.tools` option is
`attrsOf toolSubmodule` with `enable`, free-form `settings`, a reserved `release`, and
`applyOnActivation`. Only `gamemode`, `vkbasalt`, and `reshade` carry strict per-setting
typing; `mangohud`, `optiscaler`, and `proton` stay `attrsOf anything`, because
`ToolConfig.settings` is stored as a JSON blob in SQLite — the type-safety contract is at
Nix evaluation time, not at storage, and strict typing for sprawling settings (114
MangoHud knobs, 33 Proton knobs, OptiScaler's per-game fields) buys little. Activation runs
*after* `modde install` / `modde deploy`: each enabled tool gets an idempotent
`modde tool enable`, then `configure` when settings are non-empty, then `apply` only when
`applyOnActivation = true`; disabled tools call `disable`. Every `modde tool` non-zero exit
**warns and continues** so a single tool error never fails activation. HM-managed tool
releases are eager Nix fetches with pinned hashes — activation must never call networked
`modde tool install-release`. See the [Home-Manager module reference](../configuration/hm-module.md).

**Flatpak app ID is `com.tartanoglu.modde`.** The reverse-DNS of `tartanoglu.com`, a domain
owned by the project author. Flathub reserves provider-owned prefixes, and a
maintainer-owned domain keeps the application identity portable across forges (a
Codeberg-namespace ID would tie identity to the forge host). It is encoded in
`dist/com.tartanoglu.modde.metainfo.xml`, `dist/modde-ui.desktop`, and the Flatpak manifest
output in `flake.nix`.

**Distribution channels.** A single release fans out from the
`.forgejo/workflows/release.yml` run to every supported platform and channel:
direct Codeberg release archives for Linux (x86_64, aarch64), macOS (aarch64),
and Windows (x86_64); Linux packages via the AUR, Fedora COPR, a signed
apt repository, Flatpak, and AppImage; macOS via a Homebrew tap; Windows via winget,
Scoop, and Chocolatey; the CLI crate to crates.io; and the Nix flake plus
home-manager module. The authoritative per-channel commands live in the
[installation guide](../getting-started/installation.md).

## See also

- [CLI reference](cli.md)
- [Glossary](glossary.md)
- [MO2 parity & capability audit](parity.md)
