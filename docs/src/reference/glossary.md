# Glossary

This glossary covers modde concepts and modding terms such as FOMOD, LOOT,
Wabbajack, and REDmod. Each entry links to a guide. Within each theme, terms
appear roughly in the order you'll encounter them.

## modde core concepts

### Profile

A profile is a per-game bundle with its own ordered mod set, load and plugin
orders, save-vault branch, and deployment record of what was staged. Profiles
are scoped by `(name, game_id)`, so a `default` profile for Skyrim and a
`default` for Fallout 4 coexist without collision. See [Profile management](../guides/profiles.md).

### Instance

An instance is a named, self-contained data directory. A daily setup and a
Wabbajack testbed can run as separate instances without sharing a database,
store, or save vaults. modde records the active instance in
`~/.config/modde/instances.toml`. See
[Data, instances & backups](../guides/data-management.md).

### Store

The content-addressed store (`<data>/store/`) holds each installed mod's staged
bytes in its own directory. VFS symlinks point there. The database's per-mod
manifest tracks exactly which files each profile's mods staged, so modde can
uninstall just those files. See
[Deployment & VFS](../guides/deployment.md).

### Staging

Staging is the per-profile scratch tree (`profiles/<profile>/staging/`) that
`materialize()` writes the symlink farm into before it is projected onto the game
directory. For Wabbajack profiles, "staging" instead means the pre-resolved
`mods/<mod>/…` layout the modlist install produces. See
[Deployment & VFS](../guides/deployment.md).

### Deploy

Deployment projects the active profile's materialized staging tree into the
game's resolved mod directory as symlinks (hardlinks or copies for Wabbajack).
Use `modde deploy` or `modde play`, which deploys before launching. See
[Deployment & VFS](../guides/deployment.md).

### Rollback

`modde rollback` swaps a profile's `staging` and `staging.bak` directories with
atomic renames to restore a retained deploy state. Rollback fails without
`staging.bak`. Afterward, rerun `modde deploy` to project the restored tree. See
[Deployment & VFS](../guides/deployment.md#rollback).

## Ordering and conflicts

### Load order vs. plugin order

**Load order** (mod install priority) tells the VFS which *file* wins when mods
ship the same path: the later mod wins. **Plugin order** is independent: LOOT
and `plugins.txt` control the sequence in which the engine loads Bethesda
`.esp`/`.esm`/`.esl` plugins. See
[Conflicts & load order](../guides/conflicts.md).

### VFS / symlink farm

The VFS (virtual filesystem) is the merged, modded view of the game's data
directory that modde assembles as a **symlink farm**: symlinks pointing back at the
content-addressed store, so the real install on disk stays pristine. Unlike MO2's
USVFS — which hooks file-system calls per process — modde's symlinks live in the
real filesystem and are globally visible to every process without API hooking. See
[Deployment & VFS](../guides/deployment.md).

### Conflict / collision

A conflict (or collision) occurs when two or more enabled mods provide the same
relative path. modde deploys only the highest-priority version in load order.
`modde collisions` reports the pairs, per-file winners, and severities. See
[Conflicts & load order](../guides/conflicts.md).

### Collision severity (cosmetic / config / dangerous)

Each colliding file is graded by extension into a severity level: **Cosmetic**
(visual/audio only — `dds`, `nif`, `wav`; low risk), **Config** (may change
behaviour — `ini`, `json`, `toml`), **Dangerous** (scripts/plugins/DLLs that can
crash or corrupt saves — `esp`, `esm`, `dll`, `pex`), with **Unknown** for
unrecognised extensions. A mod pair's reported severity is the *worst* among its
colliding files, and the classifier is per-game. See
[Collision severity](../guides/conflicts.md#collision-severity).

### Shadowed mod

A shadowed mod contributes nothing to the deployed VFS: higher-priority mods
override every file. The collision report lists it as
`"<mod>" (N files, all overridden by <mods>)`; a diagnostic suggests disabling
it to shrink the deploy. See
[Shadowed mods and redundant files](../guides/conflicts.md#shadowed-mods-and-redundant-files).

### Redundant file

A redundant file is a mod's version of a path that always loses to another
provider. `modde collisions --suggest-hides` gives a `modde profile hide`
command for each one. See
[Shadowed mods and redundant files](../guides/conflicts.md#shadowed-mods-and-redundant-files).

### Hidden file

A hidden file is excluded without disabling its mod (equivalent to MO2's
`.mohidden`). modde records hidden files per profile and skips them when
building the VFS, so the next-highest provider of that path wins. See
[Per-file hiding](../guides/conflicts.md#per-file-hiding).

### Overwrite mod

The overwrite (output) mod is the destination for files a launched executable
writes into the deployed tree, such as xEdit output or generated patches, so
they survive between deploys. You can configure it per executable in the shipped
executable-management feature. See
[Executables & external tools](../guides/executables.md).

## Saves

### Save vault

A save vault keeps save history in one git repository per game under
`<data>/saves/<game_id>/`, with one branch per profile. Each capture is a git
commit, so you can branch and roll a profile's saves back to any captured
point. See [Save management](../guides/saves.md).

### Save snapshot

A save snapshot is a single capture in the vault — a git commit on the profile's
branch containing the live save files at that moment, plus the mod fingerprint
embedded as commit trailers. `modde save history` lists them and `modde save
restore <commit>` restores one. See [Save management](../guides/saves.md).

### Save fingerprint

A save fingerprint is the SHA-256 hash of the profile's sorted, de-duplicated
set of **enabled, save-breaking** mods. Each snapshot stores it as a commit
trailer. On restore, modde compares it with the current profile's fingerprint
and warns if the save-breaking mod set differs.
See [Save-breaking mods and the fingerprint](../guides/saves.md#save-breaking-mods-and-the-fingerprint).

### Save-breaking mod

A save-breaking mod is one whose presence or absence can make a save game
incompatible — typically anything that changes the game's serialized data: script
extenders and their plugins, mods that bake scripts or records into saves,
framework mods. Cosmetic mods (textures, reshades, UI tweaks) are *not*
save-breaking, and each game plugin classifies its own mods. See
[Save management](../guides/saves.md#save-breaking-mods-and-the-fingerprint).

## Experiments, profiles, and instances

### Experiment stack

The experiment stack lets you try profile changes non-destructively, like git
branches for your mod setup: `modde profile try` pushes the current profile and
activates another, `rollback` pops back one level, and `commit` accepts wherever
you landed and clears the history. Saves swap along with each step. See
[Experiment stack](../guides/profiles.md#experiment-stack).

### Stock snapshot

A stock (vanilla) snapshot is a preserved, hardlinked copy of a clean game install,
captured with `modde stock snapshot <game>` before you mod anything. modde computes
a deterministic tree hash over it so `modde stock verify` can later tell whether the
install has drifted from vanilla. See
[Stock snapshots](../guides/data-management.md#stock-vanilla-game-snapshots).

## Install formats and methods

### FOMOD

FOMOD is the most common *scripted installer* format on Nexus: instead of a flat
archive, the mod ships a `fomod/ModuleConfig.xml` script that asks the user
questions (texture resolution, body type, optional patches) and copies a different
file set depending on the answers. modde supports it interactively (a wizard) and
declaratively (a generated config you can pin and replay). See
[FOMOD installer](../guides/fomod.md).

### FOMOD declarative config

A FOMOD declarative config is a file (TOML, JSON, or Nix) that records your FOMOD
answers so an install is reproducible and non-interactive — the path used by Home
Manager and any automation. Generate one with `modde fomod generate`, then pass it
as `--fomod-config`. See
[Declarative installation](../guides/fomod.md#declarative-installation).

### BAIN

BAIN (BAIN Archive INstaller) is the Wrye Bash multi-option layout: an archive of
numbered subdirectories like `00 Core`, `01 Option` that you pick from. modde
auto-*detects* BAIN by the numbered-subdir convention, but the interactive
sub-package selection flow is still `Partial` — until a selection is made,
execution returns `RequiresUserInput`. See
[How mod installation works](../guides/mod-installation.md) and the
[parity audit](parity.md).

### Install method

An install method is modde's classification of an archive's layout
(`InstallMethod`): `BareExtract`, `SingleFileSet`, `DirectoryMod`, `Fomod`,
`REDmod`, `Bain`, `DllOverlay`, and more. Detection is ordered so game-specific
layouts win before generic guesses; most methods stage automatically, while FOMOD
and BAIN may pause for input. See
[How mod installation works](../guides/mod-installation.md).

## Cyberpunk 2077 / REDengine

### REDmod

REDmod is CD Projekt Red's official mod packaging format for Cyberpunk 2077 — a
package keyed off an `info.json` manifest (plus `archives/`) that installs under
`<install>/mods/<name>/`. modde detects it, symlinks the package, and runs a
post-deploy `redmod deploy` pass to register it as a loadable archive. See
[Cyberpunk 2077](../games/cyberpunk2077.md).

### REDscript / CET / TweakXL

These are the major loose-file Cyberpunk modding frameworks modde deploys:
**REDscript** (compiled gameplay scripts under `r6/scripts/`), **CET** (Cyber Engine
Tweaks, a Lua console/UI loaded via a `version.dll` proxy under
`bin/x64/plugins/cyber_engine_tweaks/`), and **TweakXL** (data tweaks under
`r6/tweaks/`). modde deploys their files into the correct roots; each framework
sequences its own loading. See [Cyberpunk 2077](../games/cyberpunk2077.md).

## Bethesda plugins and archives

### BSA / BA2

BSA (Skyrim) and BA2 (Fallout 4) are Bethesda's packed archive container formats
for game assets. modde reads and indexes their contents so a loose
`textures/sky.dds` correctly collides with a `sky.dds` packed inside another mod's
`.bsa`; the container extension itself classifies as cosmetic, with danger judged by
what is inside. Wabbajack lists can also have modde repack loose files back into a
real `.bsa`/`.ba2`. See
[Archive-aware conflicts](../guides/conflicts.md#archive-aware-conflicts-bsa-ba2-pak).

### pak / ucas / utoc

`pak`, `ucas`, and `utoc` are the Unreal Engine 4/5 archive formats — the archive
extensions modde's UE classifier recognises for titles like Stellar Blade and
Oblivion Remastered. UE games route mod paks into a `~mods/` directory at deploy
time. See [Per-game classifiers](../guides/conflicts.md#per-game-classifiers) and
[Supported games](../games/supported-games.md).

### Master / plugin

A plugin is a Bethesda content file (`.esp`, `.esm`, `.esl`); a master is a plugin
that another plugin depends on (declared in its header). modde reads the first ~1 KB
of each active plugin's TES4 header and flags **missing masters** — a plugin
requiring a master not in the active load order, which crashes the game on load. See
[Validating plugins](../guides/conflicts.md#validating-plugins).

### Form 43

Form 43 (record header version `0.94`) is the old Skyrim LE ("Oldrim") plugin
format. Using a Form 43 plugin in Skyrim SE/AE (which expects Form 44, `1.70`) can
crash the game; `modde loot validate` detects it and the fix is to resave the plugin
in the Creation Kit. See [Validating plugins](../guides/conflicts.md#validating-plugins).

### LOOT

LOOT (the Load Order Optimisation Tool) is the community standard for sorting
Bethesda plugin load order. `modde loot sort` parses the LOOT masterlist and derives
load-after / incompatible rules for the plugins you actually have active. See
[LOOT sorting](../guides/conflicts.md#loot-sorting).

### Masterlist

The masterlist is LOOT's community-maintained YAML file of per-plugin `after`,
`requires`, and `incompatible` rules for a game. modde caches it locally and, if it
is missing, prints the exact `curl` command to fetch the right one; masterlists ship
for Skyrim SE/AE, Fallout 4, Fallout 76, and Starfield. See
[LOOT sorting](../guides/conflicts.md#loot-sorting).

## Wabbajack

### Wabbajack modlist

A Wabbajack modlist (a `.wabbajack` file) is a *recipe*, not a mod bundle. It
specifies which archives to download and how to reconstruct the curated setup.
modde runs the recipe natively on Linux without a Windows VM or Wine-hosted
client, verifying every download against the hash specified by the recipe. See
[Wabbajack modlists](../guides/wabbajack.md).

### Directive

A directive is one step in a Wabbajack manifest's ordered install plan. modde
supports the common types: `FromArchive` (extract one file from a source archive),
`PatchedFromArchive` (extract then apply an OctoDiff binary delta),
`InlineFile`/`RemappedInlineFile` (write bytes stored inside the `.wabbajack`), and
`CreateBSA`/BA2 repack. See
[Install directives](../guides/wabbajack.md#install-directives).

## Nexus

### Nexus Collection

A Nexus Collection is a curated, version-pinned set of mods published on Nexus.
modde resolves it by slug (discovering the game domain and latest revision, then
fetching the pinned manifest) and installing it locks the profile to preserve the
curator's intended load order. See
[Nexus Collections](../guides/nexus.md#nexus-collections).

### `nxm://`

`nxm://` is the URI scheme Nexus's "Download with [manager]" buttons emit, carrying
the game domain, mod ID, file ID, and a short-lived `key`/`expires` pair. Run
`modde nxm install` to register modde as the system handler so clicking a Nexus
download link hands the URI to modde. See
[The nxm:// handler](../guides/nexus.md#nxm-protocol-handler).

## Games and engines

### GamePlugin

Each of the 15 built-in titles is represented by a `GamePlugin`, which supplies
its archive layouts, mod directories, save locations, conflict classifiers, and
Wine/Proton DLL overrides. Its analyze hook lets the game claim layouts
such as REDmod or an ENB pack before generic install probes run. See
[Games](../games/supported-games.md) and [Architecture](architecture.md).

### Generic game

A generic game is a user-defined title registered at runtime with `modde game add`
or imported GameSpec TOML, without a hand-coded `GamePlugin`. Support is `Partial`:
deployment, conflict classification, launcher integration, and executable management
work, but there is no game-specific filesystem scanner, and generic games have
no save tracker. See
[Generic & user-defined games](../games/generic-games.md).

## Tools and runtime

### OptiScaler

OptiScaler is an upscaler/frame-generation replacement (DLSS/FSR/XeSS) that hooks a
game through a proxy DLL, which modde manages as a release-backed tool — including
release downloading, community profiles, FSR4 payload variants, and OptiPatcher. It
also subsumes modde's old fgmod DLL-restore logic. See
[OptiScaler](../guides/tools.md#optiscaler).

### Proton / Wine DLL override

A Wine DLL override (`WINEDLLOVERRIDES`) tells Wine/Proton to load a mod's *native*
proxy DLL instead of its own built-in stub — required for frameworks that ship as a
proxy DLL (CET's `version.dll`, ReShade's `dxgi.dll`, OptiScaler). modde detects the
proxy DLLs a deploy places and contributes the overrides automatically, surfaced
through the per-game **Proton** tool integration. See
[Tools & overlays](../guides/tools.md) and
[Cyberpunk Wine DLL overrides](../games/cyberpunk2077.md#wine-dll-overrides).

## See also

- [Conflicts & load order](../guides/conflicts.md) — severity, shadowed mods, LOOT, Form 43
- [Deployment & VFS](../guides/deployment.md) — the symlink farm, staging, deploy, rollback
- [Save management](../guides/saves.md) — vaults, snapshots, fingerprints, save-breaking mods
- [Profile management](../guides/profiles.md) — profiles, locking, the experiment stack
- [How mod installation works](../guides/mod-installation.md) — install methods, FOMOD, BAIN
- [Wabbajack modlists](../guides/wabbajack.md) — modlists, directives, archive sources
- [Nexus Mods](../guides/nexus.md) — Collections, `nxm://`, browse/search
- [Tools & overlays](../guides/tools.md) — OptiScaler, Proton, Wine DLL overrides
- [Generic & user-defined games](../games/generic-games.md) — GamePlugin vs generic games
- [MO2 parity & capability audit](parity.md) — the `Done` / `Partial` status of every feature
- [CLI reference](cli.md) — every command and flag
