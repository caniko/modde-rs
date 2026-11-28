# Mod Scanning

## Overview

`modde scan` discovers installed mods from the game directory layout. Use it to
**import an existing setup** (a manual install, an old Windows deployment, or
hand-placed mods) or **track an already-deployed Wabbajack list** without
reinstalling it.

The two scan methods can work together:

1. **Filesystem scan** — a game-specific scanner walks known mod directories and
   recognises mods from their on-disk layout. This needs no manifest and works
   for any supported game.
2. **Wabbajack manifest matching** — when you pass `--manifest <list>.wabbajack`,
   modde matches the manifest's archives against the files on disk, recovering
   each mod's Nexus identity and the list's load order.

```bash
modde scan --game cyberpunk2077
```

modde resolves the game's scanner, auto-detects the install (or uses
`--game-dir`), builds a case-insensitive index of every file under the install,
and reports what it finds. Nothing is written unless you pass `--import-to`.

## Game-specific scanners

Each supported game's scanner uses a small set of composable rules for that
game's mod layout. A new scanner declares which directories hold which kinds
of mods instead of implementing its own directory traversal.

### DirectoryModRule — one subdirectory per mod

Treats each immediate subdirectory of a base directory as a single mod. The
subdirectory name becomes the mod id and display name; all files beneath it are
the mod's footprint. An optional **marker file** raises the confidence score
when present.

Examples: Cyberpunk CET mods under
`bin/x64/plugins/cyber_engine_tweaks/mods/` (marker: `init.lua`), REDscript mods
under `r6/scripts/`, TweakXL mods under `r6/tweaks/`, and REDmod mods under
`mods/`.

### SingleFileModRule — one file per mod

Treats each file with a given extension directly inside a directory as a mod.
The file stem becomes the mod id/name. Configurable **ignored prefixes** skip
known stock/engine files so they are not reported as user mods.

Example: each loose `.archive` under Cyberpunk's `archive/pc/mod/`.

### FileGroupRule — group sidecar files into one mod

Groups files that share a stem across several extensions into a single mod — so
a `.pak` plus its `.ucas`/`.utoc` sidecars (Unreal Engine games) are reported as
one mod, not three. It also descends one level into subdirectories so packaged
mods are grouped correctly.

### What a scan emits

Every rule produces a `DiscoveredMod` with a stable `mod_id` (prefixed by the
rule, e.g. `cet/<name>`, `archive/<stem>`, `redmod/<name>`), a display name, the
list of files it owns, the scan location, and a confidence score. The scanner
also exposes its `scan_directories()` (printed at the top of a scan) and a
`mod_id_footprint()` mapping used by deduplication.

## When to use which

| Situation | Use |
| --------- | --- |
| Imported a manual / hand-placed install, no manifest | **Filesystem scan** — `modde scan --game <id>` |
| You have the original `.wabbajack` for a deployed list | **Manifest matching** — add `--manifest <list>.wabbajack` |
| Both: a Wabbajack base plus your own extra mods | **Both at once** — manifest recovers Nexus identity + load order; the filesystem scan catches your additions |

Use the manifest when available: it recovers Nexus mod/file ids for updates and
the mod-information side panel, and reconstructs the list's load order as a lock.
A filesystem scan works without a manifest and finds mods the manifest didn't
deploy.

## Importing results into a profile

```bash
modde scan --game cyberpunk2077 --import-to my-cyberpunk
```

modde creates the named profile if needed. Merging is **additive and
non-destructive**: modde imports only new `mod_id`s. Existing rows retain all
per-mod state, including notes, categories, and per-mod locks.

## Wabbajack manifest matching

When migrating from a Wabbajack install, match on-disk files against the
manifest:

```bash
modde scan --game skyrim-se \
  --manifest /path/to/modlist.wabbajack \
  --import-to my-modlist
```

This:

1. Parses the `.wabbajack` manifest.
2. Groups its `FromArchive` / `PatchedFromArchive` directives by source archive
   and checks what fraction of each archive's `To` paths exist on disk.
3. Imports matched archives as mods, carrying their **Nexus mod/file id and game
   domain** when the archive is `NexusDownloader`-sourced.
4. Applies a **Wabbajack load-order lock** to the profile, reordering matched
   mods to follow the manifest's install-directive order (the closest
   reproducible approximation of Wabbajack's load order) and stamping a
   `manifest_hash` so the lock is self-identifying. Mods not mentioned by the
   manifest keep their relative order and are appended after.

The `.wabbajack` source file is copied into modde's content-addressed cache so
`profile lock-info` can find it later.

Matched mods use the same canonical `mod_id` scheme as a real
`modde install wabbajack` (Nexus archives → `nexus_<domain>_<mod>_<file>`,
others → `wj_<hash>`), so re-scanning a list you installed through modde matches
the existing rows instead of creating duplicates.

## Threshold filtering

The `--threshold` flag controls how many of a mod's expected files must be
present on disk for the archive to count as matched (present files ÷ expected
files):

```bash
# Require 80% of expected files to be present
modde scan --game skyrim-se --threshold 0.8 --manifest /path/to/modlist.wabbajack

# More lenient matching (default is 0.5)
modde scan --game skyrim-se --threshold 0.3 --manifest /path/to/modlist.wabbajack
```

Raise the threshold to avoid false positives (mods that share a few common
files); lower it when a list does heavy file overwriting and few archives keep
100% of their files intact. The scan report prints `present/total` and a
confidence percentage per matched archive so you can tune it empirically.

## Dry-run mode

Preview what would be discovered without writing to the database:

```bash
modde scan --game skyrim-se --dry-run
```

`--dry-run` works with and without `--manifest` and `--import-to`. With
`--import-to` it reports how many mods *would* be imported but makes no changes —
always run a dry pass first on a large list.

## Pruning duplicates

A manifest can identify a mod by its `nexus_*` id while the filesystem scanner
finds the same mod by directory. For example, the scanner may find a CET mod
folder with a runtime-written `settings.json` that the manifest never deployed.
During a combined scan, modde skips filesystem mods the manifest already covers.
It checks **directory prefixes** for directory-based mods and individual files
for single-file mods, so a stray runtime file doesn't duplicate the whole mod.

Use `--prune-duplicates` to clean up duplicates from earlier buggy scans. It
checks each filesystem-scanner row in the target profile against the manifest
**before** merging, removing rows whose footprint the manifest covers and
keeping your additions. It requires both `--manifest` and
`--import-to`:

```bash
modde scan --game skyrim-se \
  --manifest /path/to/modlist.wabbajack \
  --import-to my-modlist \
  --prune-duplicates
```

For an existing profile you don't want to re-scan, use the standalone equivalent:

```bash
modde profile dedup my-modlist --manifest /path/to/modlist.wabbajack --apply
```

## Post-scan review workflow

A scan is a *proposal*, not a commit. Review before trusting it:

1. **Dry-run first.** `modde scan --game <id> [--manifest …] --dry-run` and read
   the report: the scanner's scan directories, the manifest match table
   (`present/total`, confidence), the filesystem table (file count, location,
   confidence), and how many filesystem mods were skipped as already-covered.
2. **Tune `--threshold`** if the manifest match rate looks wrong — raise
   `--threshold` if low-confidence false positives are being admitted; lower it
   if a list you know is installed yields too few matches.
3. **Import** with `--import-to <profile>` once the proposal looks right.
4. **Dedup** if you imported into a profile that had prior filesystem-scan rows:
   add `--prune-duplicates` (or run `modde profile dedup … --apply`).
5. **Verify the load order.** When a manifest was used, the profile gets a
   Wabbajack lock; check it with `modde profile lock-info <profile>` and review
   the mod order in the UI before deploying.
6. **Deploy.** Scanning only records what modde *found* on disk; run a normal
   deploy to bring the profile under modde's management. See
   [Deployment](deployment.md).

## See also

- [Wabbajack modlists](wabbajack.md) — installing lists; the manifest format scanning matches against
- [Profiles](profiles.md) — load-order locks and `profile dedup`
- [Deployment](deployment.md) — applying a reviewed profile to the game
- [Conflicts](conflicts.md) — resolving overlaps after import
- [Supported games](../games/supported-games.md) — which games have scanners
- [CLI reference](../reference/cli.md) — full `scan` flag list
