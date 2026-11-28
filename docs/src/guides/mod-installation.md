# How mod installation works

## Overview

Archives come in different layouts: loose Skyrim files for `Data/`, Cyberpunk
REDmod packages keyed off `info.json`, texture packs with FOMOD installers, or
Wrye Bash mods with numbered option folders. One pipeline classifies each
archive, stages files in a per-mod store directory, and records them for exact
removal.

## The three-stage pipeline

Every install is `analyze → execute → record`.

### 1. Analyze

`installer::analyze` inspects an extracted archive and decides how to stage it,
producing an `InstallPlan`. The plan carries:

- a `method` — one of the [`InstallMethod`](#install-methods) variants below;
- an optional `strip_prefix` — set when the archive is wrapped in a single
  directory (e.g. `ModName-1.0/<real contents>`), so everything downstream is
  resolved relative to the inner directory;
- the `source_archive_hash` — the xxh64 digest of the original download, computed
  during download (it cannot be derived from the extracted tree);
- an empty `staged_files` list, filled in by the next stage.

Detection is **ordered** so authoritative game-specific rules beat generic
guesses:

1. **Normalize** — if the extracted directory is a single wrapper directory with
   no files, recurse into it and record `strip_prefix`. Unwrapping stops at a
   directory whose only child is recognized mod content (`Data/`, `r6/`,
   `fomod/`, …) so real content roots are never mistaken for wrappers.
2. **Game plugin** — `GamePlugin::analyze_mod_archive` gets first crack. A game
   can claim a layout it knows (Cyberpunk's REDmod, an ENB pack for Bethesda)
   before any generic probe runs.
3. **FOMOD** — presence of `fomod/ModuleConfig.xml`.
4. **BAIN** — numbered option subdirectories (`00 Core`, `01 Option`, …).
5. **DLL overlay** — a top-level `.dll` with no nested asset directories.
6. **Bare extract** — the game plugin's `recognizes_bare_layout` recognizer.
7. **User-config overlay** — the plugin advertised a `UserConfig` deploy target
   *and* every file in the tree looks like a config file.
8. **Unknown** — fall through; the caller dumps a [dossier](#unknown-archives-and-the-dossier-system).

### 2. Execute

`installer::execute` stages files from the temporary extraction directory into
the mod's store directory according to the plan, and returns a concrete
`StagedFile` manifest. It copies files (or renames them on the same filesystem)
and records each file's store-relative path, original archive path, and size.
It does **not** touch the database.

Two variants can stop here and ask for input: a `Fomod` with no config and a
`Bain` with no selection both return `RequiresUserInput`, which the GUI routes to
a wizard. An `Unknown` method returns `UnknownMethod`.

### 3. Record

The caller passes the returned `Vec<StagedFile>` to `ModdeDb::record_install`,
which persists the method and manifest in the database. [`modde mod remove`](../reference/cli.md)
uses that manifest to unlink all and only the files this mod staged.

The mod's row also carries an `InstallStatus`:

| Status | Meaning |
| ------ | ------- |
| `installed` | Files are staged and tracked. |
| `pending_user_input` | Detection succeeded but execution needs answers (FOMOD wizard, BAIN picker). The archive sits extracted in a staging dir. |
| `unknown` | Detection failed; a dossier was written. Retry is blocked until the installer is extended. |
| `failed` | Execution started but failed partway; the store dir may hold partial files. |

## Install methods

`InstallMethod` is the heart of detection — and the extensibility point. Variants
are ordered by specificity, so game-specific layouts take precedence over generic
ones (a Cyberpunk archive with both `info.json` and a `Data/` folder is `REDmod`,
not a Bethesda bare-extract).

| Method | What triggers it |
| ------ | ---------------- |
| `BareExtract` | Game plugin's `recognizes_bare_layout` returns true — the archive maps 1:1 onto the game's mod dir (e.g. loose files that drop straight into Skyrim's `Data/`). |
| `StripContentRoot { root }` | Archive nests its payload under a content root that must be peeled before staging (e.g. `Data/Foo.esp` → `Foo.esp` when the deploy root is already `Data/`). Emitted by a game plugin's analyze hook. |
| `DirectoryMod { directory_name }` | Archive root is one self-contained directory-style mod; files stage under a stable subdirectory (defaults to the store dir name). |
| `DirectoryModFromXml { marker, id_attr, fallback_name }` | Same as `DirectoryMod`, but the stable directory name is read from an XML marker attribute — Bannerlord's `SubModule.xml` is the canonical case. |
| `MultiRootOverlay { roots }` | Archive carries several game-root overlay directories at once (`mods/`, `dlc/`, `bin/`, …); each is staged under its own root. |
| `SingleFileSet` | One or more loose files that stage directly into the resolved mod root. |
| `Fomod { module_config, config_toml }` | `fomod/ModuleConfig.xml` is present. `config_toml` holds the chosen options; `None` means the wizard must run before execution. See the [FOMOD guide](fomod.md). |
| `REDmod { manifest }` | A Cyberpunk REDmod package — detected by a game plugin via its `info.json` manifest (plus `archives/`). Staged under `mods/<name>/`. |
| `Bain { selected_subdirs }` | Wrye Bash layout: ≥2 numbered option subdirs like `00 Core`, `01 Option`. `selected_subdirs` is empty until the user picks options. |
| `DllOverlay { target_dir_hint }` | A top-level `.dll` with no nested asset directories (dxvk, ENB, proxy hooks). Files route to the game's executable directory at deploy time. |
| `ScriptMerge { merge_group, base }` | Reserved for script-merge support. Today it stages per `base` and tags every file with `merge_group`; actual merging is not yet wired in. |
| `UserConfigOverlay { target_id }` | The game plugin advertised a `UserConfig` deploy target *and* every file in the archive is a config file (`.ini`, `.cfg`, `.json`, `.xml`, …). Staged like a bare extract; the alternate routing only applies at deploy time. |
| `Unknown { reason }` | Nothing matched. A dossier is dumped so the installer can be extended. |

### Auto-detected vs. needs input

Most methods are fully automatic — analysis classifies the archive and execution
stages it with no further interaction. The exception is the two interactive
formats:

- **FOMOD** — auto-*detected* (the `fomod/` directory is unmistakable), but
  execution needs the user's option choices. Supply them interactively via the
  GUI wizard, or non-interactively with a declarative config (`--fomod-config`,
  or `modde fomod apply`). Until a config is present the mod sits
  `pending_user_input`. See the [FOMOD guide](fomod.md).
- **BAIN** — auto-*detected* by the numbered-subdir convention, but modde cannot
  know which options you want; `selected_subdirs` starts empty and the GUI prompts
  you to pick. Without a selection, execution returns `RequiresUserInput`.

Everything else (`BareExtract`, `StripContentRoot`, `DirectoryMod`,
`DirectoryModFromXml`, `MultiRootOverlay`, `SingleFileSet`, `REDmod`,
`DllOverlay`, `UserConfigOverlay`) is `is_ready()` the moment analysis finishes.

**Wrapper stripping** works with any of these layouts. The analyzer records a
`strip_prefix` during normalization so execution uses the inner directory.
There's no need to repack an archive with a `ModName-1.0/` wrapper.

## Unknown archives and the dossier system

When detection falls through to `Unknown` (or execution surfaces
`UnknownMethod`), modde does not silently fail or guess. It writes a **dossier** —
a self-contained bundle that captures everything a developer (or a Claude Code
skill) needs to teach the installer this new layout.

### Where it lives

```text
$XDG_DATA_HOME/modde/unknown-installers/<slug>/
  ├─ metadata.json        — mod + context (game, Nexus ids, name, author, hash)
  ├─ archive_tree.txt     — recursive listing of the extracted archive (≤500 entries)
  ├─ file_samples/        — up to 5 small text files copied verbatim
  │                         (READMEs, info.json, ModuleConfig.xml, meta.ini, …)
  ├─ analyzer_trace.json  — which probes ran and how each voted
  └─ PROMPT.md            — a self-contained skill prompt
```

The `<slug>` is stable across retries — usually `<domain>_<mod>_<file>` for a
Nexus install, falling back to `<game>_<hash-prefix>` otherwise — so retrying the
same mod overwrites the same dossier rather than piling up duplicates.

### What `PROMPT.md` contains

`PROMPT.md` is the actionable core. It restates the mod's identity and the
detection verdict, then lays out exactly how to extend support:

1. Read `archive_tree.txt` and `file_samples/` to understand the layout.
2. Decide where the fix belongs — a **generic** package format gets a new
   `InstallMethod` variant plus a detection branch in `analyze.rs`; a
   **game-specific** quirk extends that game's `analyze_mod_archive` impl in
   `crates/modde-games/`.
3. Extend `execute.rs` if the new variant needs custom staging.
4. Add a unit test built from the dossier's file samples.
5. Run the installer test suite.
6. Rename the dossier directory with a `.resolved` suffix so the UI surfaces a
   **Retry Install** button.

### Reading a dossier from the CLI

```bash
modde mod diagnose <mod_id>
```

`mod diagnose` finds the unknown install's dossier and writes the dossier path
to stdout, followed by the contents of `PROMPT.md`. It's read-only, so you can
pipe the output to an agent or paste it into a chat:

```bash
modde mod diagnose skyrimspecialedition_12345_67890 | claude
```

This is how an unrecognized layout becomes supported: the dossier turns "modde
doesn't know what this is" into a precise, reproducible task, and once a fix lands
and the dossier is marked `.resolved`, the original install can be retried with no
re-download.

## See also

- [FOMOD installer](fomod.md) — the interactive/declarative install method in
  depth.
- [Deployment](deployment.md) — how staged files reach the game directory.
- [Conflicts & load order](conflicts.md) — what happens when two installs stage
  the same file.
- [Mod scanning](scanning.md) — discovering mods already installed on disk.
- [Generic & user-defined games](../games/generic-games.md) — how a game plugin
  advertises the deploy targets that `UserConfigOverlay` routes to.
- [CLI reference](../reference/cli.md) — `modde install`, `modde mod`,
  `modde fomod`.
