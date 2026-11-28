# Deployment & VFS

## Overview

modde never copies mods into your game directory and never edits the original
files. Instead it builds a **symlink farm**: a virtual filesystem (VFS) assembled
from symlinks that point back at a content-addressed store. The game — and any
external tool — sees a fully merged, modded `Data/` (or `~mods/`, or `mods/`,
depending on the game), while the real install on disk stays pristine.

This is the Linux-native equivalent of Mod Organizer 2's USVFS. USVFS hooks
filesystem calls per process, so only the hooked process sees the merged tree.
modde's symlinks are **globally visible** to every process, including the game,
`xEdit`, packers, and shells, without API hooks.

> The VFS symlink farm is the deploy path for Nexus and Manual (store-backed)
> profiles. Wabbajack profiles take a different, hardlink/copy path — see
> [Wabbajack profiles](#wabbajack-profiles) below.

## The deployment pipeline

Deployment is a typestate pipeline enforced at compile time:

```
SymlinkFarm<Built>        link map computed in memory (paths only, no I/O)
        │  .materialize()
        ▼
SymlinkFarm<Materialized> symlinks written into the profile staging dir
        │  .deploy_to() / deploy_to_install()
        ▼
   (symlinks in the game's mod directory)
```

Each stage has its own Rust type. The compiler rejects `deploy_to()` on a `Built`
farm, preventing deployment before materialization. The zero-sized
`PhantomData` state marker adds no runtime cost.

### 1. Build

`SymlinkFarm::build()` computes a single `HashMap<relative_path, source_path>`
where `source_path` is the **absolute** path of the winning file inside the
store. No filesystem writes happen yet — this phase is pure path resolution.

The build takes:

| Input        | Meaning                                                          |
| ------------ | --------------------------------------------------------------- |
| `profile_name` | Selects the staging directory location                        |
| `resolved`   | The mods in final priority order (first = lowest priority)      |
| `mod_files`  | Per-mod `(relative_path, store_source_path)` listings          |
| `overrides`  | Optional profile-level override files (always win)             |
| `hidden`     | Optional `(mod_id, relative_path)` pairs to exclude            |

#### How files map to winners

The build walks `resolved.order` from lowest to highest priority and inserts
each `(rel_path → source)` into the map. Because later inserts overwrite earlier
ones for the same key, **the highest-priority provider of each path wins**.
The effective priority chain, highest first:

1. **Profile overrides** — files in the profile's `overrides` directory are
   layered last and beat every mod.
2. **Later mods in the load order** — install priority, last-wins.
3. **Earlier mods in the load order**.

A file listed in `hidden` as `(mod_id, rel_path)` is skipped while walking that
mod, so the next-highest provider of that path wins instead. This is how
[per-file hiding](conflicts.md#per-file-hiding) takes effect — the equivalent
of MO2's `.mohidden`. A mod that appears in `resolved.order` but has no entry in
`mod_files` (its store directory is missing) is silently skipped during build;
the deploy command logs the missing mods separately.

> **Install priority is not plugin load order.** The symlink farm resolves which
> *file* wins on disk. Bethesda `.esp`/`.esm`/`.esl` ordering is a separate
> concern handled by LOOT and `plugins.txt`. See
> [Conflicts & Load Order](conflicts.md).

### 2. Materialize

`materialize()` writes the farm to the per-profile **staging directory**:

```
~/.local/share/modde/profiles/<profile>/staging/
```

It removes any existing staging directory, recreates it, then makes the parent
directories for each `(rel_path → source)` and symlinks `staging/<rel_path>` to
the **absolute** store `source`. Rebuilding staging leaves exactly the current
`Built` farm, with no stale files from previous deployments.

The content store these links point into lives at:

```
~/.local/share/modde/store/<mod_id>/...
```

### 3. Deploy

Deploy links staging into the game's resolved mod directory. For each relative
path, it removes any existing destination entry and symlinks to the corresponding
file **inside the staging directory** using an absolute path. The result is a
two-hop chain:

```
<game>/Data/textures/sky.dds   ─sym→   staging/textures/sky.dds   ─sym→   store/<mod_id>/textures/sky.dds
```

The game plugin chooses final placement via `deploy_to_install()`. For example,
UE4 titles route paks into `Content/Paks/~mods/`. After deployment, modde runs
the game's `post_deploy` hook and configures Wine DLL overrides
(`WINEDLLOVERRIDES`) for any proxy DLLs
the mods deploy, such as `version.dll` for CET or `winmm.dll` for ASI loaders.
If the profile has enabled [patcher pipeline](patcher-pipelines.md) stages,
modde then runs each stage, imports its generated files into the configured
output mod, keeps that output mod late in the profile, and redeploys before the
next stage.

### Symlinks: absolute, not relative

Both hops use the symlink **target exactly as passed**: an absolute path.
Linux uses `symlink(2)`; on Windows modde inspects the target to choose
`symlink_file` or `symlink_dir`.

**If a store entry is missing**, the symlink is still created but dangles. modde
does not error at deploy time for a broken link — the missing-mod case is caught
earlier during build (the mod is skipped and reported), and dangling links are
surfaced by [`modde verify`](#verifying-integrity), which flags any
`broken symlink -> <target>`.

## Deploying

### Manual deployment

```bash
# Deploy the active profile
modde deploy

# Deploy a specific profile
modde deploy --profile my-skyrim --game skyrim-se
```

### Deploy via play

`modde play` deploys before launching the game:

```bash
modde play --game skyrim-se
```

### Experimental hot-deploy

Cyberpunk 2077 has an experimental cosmetic-only hot-deploy path:

```bash
modde hot-deploy --profile my-cyberpunk --game cyberpunk2077 --mod body-textures --enable
modde hot-deploy --profile my-cyberpunk --game cyberpunk2077 --mod body-textures --disable --dry-run
```

Hot-deploy patches the existing profile staging tree and the live symlink
deployment without rebuilding the full profile. It is intentionally narrower
than `modde deploy`: Wabbajack profiles, unsupported games, missing store
directories, unknown mods, mixed-content mods, and patches touching config,
script, DLL, or unknown-severity files are refused. It also refuses to run
while the game process appears to be running (checked against `/proc`) — close
the game or pass `--force` to bypass the check. If a patch fails, profile state
is not persisted; recover with the printed
`modde deploy --profile <name> --game <game>` command.

This is a live filesystem patch, not a guarantee that the running game reloads
every asset immediately. UE/Penumbra-style runtime reload support remains a
per-game research path.

### Automatic deployment

After rebuilding your NixOS / home-manager configuration, modde deploys profiles
via an activation script when their prerequisites are present. Wabbajack
profiles can wait non-fatally for the game install: set
`installMode = "await-game"` or leave `gameDir` unset until Steam or Heroic has
installed the game, then set `gameDir` and rebuild. For Home Manager Wabbajack
profiles, the install runs before deployment only when the configured `gameDir`
exists and contains the expected game content directory (for example `Data/` for
Skyrim SE); otherwise activation prints an awaiting message and continues.

## The store link strategy

The symlink farm creates only symlinks. When modde needs a real file instead
(notably for Wabbajack deployment), `link_or_copy` tries three strategies in
order and reports which it used:

| Strategy   | When it applies                                  | Cost                       |
| ---------- | ------------------------------------------------ | -------------------------- |
| Hardlink   | Source and destination on the same filesystem    | Free; shares inode/blocks  |
| Reflink    | Hardlink crossed a filesystem (`EXDEV`) but the FS supports copy-on-write (btrfs, XFS, ZFS) | Free until written; CoW    |
| Copy       | Neither of the above worked                       | Full byte copy             |

The destination is removed first if it exists. A cross-device error
(`EXDEV` on Linux, `ERROR_NOT_SAME_DEVICE` on Windows) triggers the fall to
reflink; a reflink failure falls to a plain copy. This keeps deployments cheap
on a single CoW filesystem and correct everywhere else.

## Rollback

`modde rollback` swaps the profile's `staging` and `staging.bak` directories:

```bash
modde rollback --profile my-skyrim
```

Rollback uses directory renames, which are atomic on a single filesystem:

- If a current `staging` exists, it is renamed aside to `staging.old`,
  `staging.bak` is renamed into place as `staging`, and `staging.old` is
  removed.
- If no current `staging` exists, `staging.bak` is renamed straight into place.

If there is **no `staging.bak`** for the profile, rollback fails with
`no backup staging found for profile '<name>'`. modde does not silently create a
backup on every deploy — `materialize()` rebuilds staging in place — so
`staging.bak` is a deliberate restore point you (or higher-level tooling)
preserve before a risky change. After a successful rollback you must re-run
`modde deploy` to re-project the restored staging tree into the game directory.

## Wabbajack profiles

Wabbajack-installed profiles skip the symlink farm. Their layout under
`~/.local/share/modde/staging/<profile>/mods/<mod>/...` already resolves conflicts,
so the VFS has nothing to resolve. Deploy walks the `mods/` tree and places each
file in the game directory with `link_or_copy` (hardlink → reflink → copy):

- MO2 metadata files (`meta.ini`, `meta.json`) are skipped.
- A file already hardlinked to its source (same inode and device) is skipped, so
  re-deploys are near-instant.
- Wine DLL overrides are configured afterwards, exactly as for store-backed
  profiles.

Hardlinking (rather than symlinking) is used here because the staging tree is the
authoritative, fully-merged layout — the game sees real files, which is what
Wabbajack lists and their tools expect. See the
[Wabbajack guide](wabbajack.md) for the full install flow.

## Verifying integrity

```bash
modde verify --profile my-skyrim
```

`verify` walks every enabled mod's store directory, hashes each file with xxHash,
and checks symlink health. It reports:

- **Total files checked**
- **Missing** — enabled mods absent from the store, or files that vanished
- **Mismatches/errors** — files it could not hash, and any
  `broken symlink -> <target>` where the link target no longer exists

A clean run prints `All files OK.` This is the fastest way to spot a store entry
deleted after deployment.

## Vanilla (stock) snapshots

A stock snapshot records the *original* game installation so you can detect
changes to vanilla files, including changes from mods, tools, or a failed deployment:

```bash
# Capture a snapshot of the detected install
modde stock snapshot skyrim-se

# Re-verify the install against the snapshot later
modde stock verify skyrim-se
```

`snapshot` detects the install directory via the game plugin, records the stock
file set, and prints the snapshot path and file count. `verify` re-checks the
current install against the recorded snapshot and prints either
`Stock snapshot for <game>: OK` or
`Stock snapshot for <game>: MISMATCH (re-snapshot recommended)`. Take the
snapshot *before* you first deploy mods, while the game is genuinely vanilla.

## See also

- [Conflicts & Load Order](conflicts.md) — how the winning file is chosen and
  how Bethesda plugin order differs from install priority
- [Wabbajack](wabbajack.md) — the hardlink/copy deploy path for modlists
- [Profiles](profiles.md) — overrides, hidden files, and per-profile staging
- [Playing](playing.md) — `modde play` and launch integration
- [Troubleshooting](troubleshooting.md) — broken symlinks and missing-store fixes
- [Parity reference](../reference/parity.md) — VFS feature coverage vs MO2
