# Starfield

Starfield is one of modde's five built-in Bethesda Creation Engine titles. It
shares the data-driven `BethesdaGame` plugin with Skyrim SE/AE and the Fallout
games, so its deploy strategy, conflict classification, and plugin handling are
the same engine-wide machinery — only the per-title metadata (app ID, save
folder, INI names, archive extension) differs.

## Engine & overall status

| Property | Value |
| -------- | ----- |
| Engine family | Bethesda Creation Engine |
| modde game id | `starfield` |
| Display name | `Starfield` |
| Steam App ID | `1716740` |
| Nexus domain | `starfield` |
| Nexus numeric game id | `4187` |
| Wabbajack name | `Starfield` |
| Mod directory | `Data/` (under the install root) |
| Archive extension | `.ba2` |
| Managed INI files | `StarfieldPrefs.ini`, `StarfieldCustom.ini` |
| Save extension | `.sfs` |

**Overall status: `Partial`.** The scanner, conflict classifier, plugin/load-order
handling, and diagnostics are all wired in and exercised, but Starfield's overall
path is held at `Partial` because the broader modding workflow around it is still
maturing relative to the fully battle-tested Skyrim SE / Fallout 4 paths.

**Save tracking is `Done`.** Starfield ships a dedicated `.sfs` save tracker that
is reachable end to end through the same git-backed save vault every other game
uses. See [Save tracking](#save-tracking) below and the
[canonical status table](supported-games.md) for how this lines up with the
other titles.

> Status vocabulary follows the repository's
> [`docs/capability-matrix.toml`](https://github.com/caniko/rs-modde/blob/trunk/docs/capability-matrix.toml)
> and the [parity audit](../reference/parity.md). `Done` means shipped end to
> end and user-reachable; `Partial` means core logic exists but the surrounding
> workflow is not yet fully trustworthy.

## How modde detects the install

Starfield is registered with a Steam App ID (`1716740`) and the Steam library
folder name `Starfield`. modde's launcher detection uses these to locate the
install under your Steam libraries. No Heroic GOG or Epic IDs are registered for
Starfield, so launcher auto-detection is Steam-oriented; for any other source you
can always point a profile at the install directory explicitly.

Because Starfield runs through Proton on Linux, modde reads its Windows-side game
data — `plugins.txt` and the per-profile INI files — from inside the Proton
prefix rather than from a native Linux path. The relevant locations under the
Steam compatibility prefix are:

```text
# Save games (auto-detected by the plugin)
~/.local/share/Steam/steamapps/compatdata/1716740/pfx/drive_c/Users/steamuser/Documents/My Games/Starfield/Saves

# Load order (read by the scanner)
~/.local/share/Steam/steamapps/compatdata/1716740/pfx/drive_c/users/steamuser/AppData/Local/Starfield/plugins.txt
```

The save directory is resolved relative to your detected Steam `common/` folder,
so a non-default Steam library still resolves correctly as long as the
`compatdata/1716740/pfx` prefix exists. If the prefix has not been created yet
(you have never launched the game), save detection returns nothing rather than
guessing a path.

## Mod directory & deploy strategy

Like every Bethesda title, Starfield's mod directory is `Data/` under the game's
install root. modde deploys with the standard **symlink-farm virtual filesystem**:
mods are staged in an intermediate directory and symlinked into `Data/`, leaving
the original game files untouched and the whole deployment reversible.

The build/materialize/deploy pipeline, priority ordering (profile overrides win,
then later mods in the load order), rollback, and `modde verify` all behave
exactly as documented in [Deployment & VFS](../guides/deployment.md). Nothing
about Starfield changes the deploy model — it is the same engine-wide path used
by Skyrim SE and Fallout 4.

When modde recognizes a **bare** (loose-files) mod layout during extraction, it
keys off the shared Bethesda layout policy: recognized root directories include
`data`, `meshes`, `textures`, `scripts`, `interface`, `sound`, `music`,
`materials`, `seq`, `shadersfx`, and `strings` (matched case-insensitively), plus
root-level files with `.esp`, `.esm`, `.esl`, `.bsa`, or `.ba2` extensions. A mod
archive that unpacks into one of these is treated as already rooted at `Data/`.

## What scanning finds

Starfield uses the data-driven `BethesdaScanner`, the same plugins.txt-aware
scanner as Skyrim SE and Fallout 4 (Fallout 76 is the exception — it uses the
loose-archive scanner). Scanning the `Data/` directory does the following:

1. **Reads `plugins.txt` for authoritative load order.** modde locates
   `plugins.txt` inside the Proton prefix (path above) and parses it. Each line is
   a plugin; a leading `*` marks it **enabled**, an unprefixed line is
   **disabled**, and `#`/blank lines are ignored.
2. **Emits plugins listed in `plugins.txt` first**, at high confidence (`0.95`).
   For each plugin it pairs companion archives that share the plugin's stem —
   both `<stem>.ba2` and the texture-suffixed `<stem> - Textures.ba2` — into the
   same discovered mod.
3. **Also scans for plugins not in `plugins.txt`** (disabled or unmanaged) by
   walking `Data/` for `.esp`, `.esm`, and `.esl` files, emitting them at lower
   confidence (`0.8`) so you can see plugins the load order does not yet track.

Each discovered mod's footprint is recorded `Data`-relative, which is how modde
compares against MO2-style manifests when detecting stale duplicates. Plugin file
extensions recognized are `esp`, `esm`, `esl`; companion archive extensions are
`bsa` and `ba2` (Starfield ships `.ba2`).

## Conflict classification

Starfield uses the shared `BethesdaCollisionClassifier`. When two mods provide the
same relative path, modde classifies the **severity** of the overlap by file type.
It can also read inside `.bsa`/`.ba2` archives (via the archive index) to classify
conflicts on archived contents, not just loose files.

| Severity | Extensions |
| -------- | ---------- |
| **Dangerous** | `esp`, `esm`, `esl`, `pex`, `dll`, `psc` |
| **Config** | `ini`, `cfg`, `json`, `toml`, `xml` |
| **Cosmetic** | `dds`, `png`, `tga`, `jpg`, `nif`, `hkx`, `fuz`, `wav`, `xwm`, `swf`, `btr`, `bto`, `btt`, `bsa`, `ba2` |

"Dangerous" overlaps (plugins, script bytecode/source, native DLLs) are the ones
that change game logic and most often need a real resolution; cosmetic texture and
mesh overlaps are usually a matter of which look wins. The same extension set
feeds the **save-breaking** fingerprint described below. For the full conflict
workflow — how to inspect, hide files, and reorder — see the
[Conflicts guide](../guides/conflicts.md).

## Save tracking

Save tracking for Starfield is **`Done`** for file capture and restore. Unlike
the older Bethesda titles, which parse a binary `.ess` header for the character
name and save number, Starfield's saves are `.sfs` files and are tracked with
modde's declarative pattern-based tracker.

> **Save-contamination gate:** Starfield does **not** currently have a
> record-level save dependency analyzer. `modde mod remove --dry-run` does not
> inspect `.sfs` records for plugin or script dependencies, because the project
> does not yet have a verified Starfield `.sfs` magic/header source or fixture.
> Until that evidence exists, modde will not invent a parser or silently claim
> removal-safety coverage for Starfield saves.

What the tracker captures:

- **File type:** `.sfs` files in the Saves directory (non-recursive).
- **Slot category**, derived from the filename prefix:

  | Prefix | Category |
  | ------ | -------- |
  | `Autosave` | `auto` |
  | `Quicksave` | `quick` |
  | `Exitsave` | `exit` |
  | `Save` | `manual` |

  Any `.sfs` that does not match a known prefix still falls back to `manual`, so
  no save is silently dropped.
- **Label:** the file stem (the save's filename without the `.sfs` extension).
- **Capture summary:** grouped **by category**, so a capture reads like
  `capture: 6 saves (2 auto, 1 exit, 3 manual)` rather than a flat count.

These detected saves feed the same **git-backed save vault** as every other game:
a per-game git repository with per-profile branches, automatic capture on profile
switch and on game exit, history browsing, and restore. Each snapshot embeds a
**SHA-256 fingerprint** of the profile's enabled save-breaking mods (the
`Dangerous`-classified extensions above), so restoring a save that was made under
a different mod set warns you about which save-breaking mods were added or removed.
The end-to-end save workflow — adopt, capture, watch, history, restore,
fingerprint semantics — is documented in the [Save management guide](../guides/saves.md).

## Plugin & load-order handling

Starfield has a plugin system (`has_plugin_system()` is true). modde reads and
writes `plugins.txt` in the `*`-prefix enabled/disabled format. When modde writes
the file it prepends a generated header marker and emits one line per plugin,
prefixing enabled plugins with `*`:

```text
# This file is generated by modde. Do not edit manually.
*Starfield.esm
*BlueprintShips-Starfield.esm
SomeDisabledMod.esp
```

The load order read from `plugins.txt` is authoritative for the scanner and is the
order modde uses when resolving conflicts (later wins).

### Diagnostics

Starfield participates in the shared Bethesda diagnostics engine, which runs:

- **Missing masters** (`Error`): flags an active plugin whose required master is
  not present in the load order — the game would crash on load. The suggested fix
  is to install and enable the mod that provides the missing master.
- **Orphaned overrides** (`Info`): notes when the profile's overrides directory
  contains files, since those take top priority over every mod.
- **Empty / store-presence** checks from the base diagnostics engine.

> The **Form 43** ("Oldrim" plugin format) diagnostic is **Skyrim SE/AE only** —
> it does not apply to Starfield. Do not expect a Form-43 warning here.

## Installer specifics

- **FOMOD:** Starfield benefits from the same scripted-installer support as the
  rest of the engine. modde's FOMOD support is provided by the `fomod-oxide`
  installer, so option-tree FOMOD packages (the common Nexus install format for
  Bethesda mods) are handled by the shared installer path. See the
  [FOMOD guide](../guides/fomod.md) for how the option tree is presented.
- **Loose archives / plugins:** Mods that are just `.ba2` archives and/or
  `.esp`/`.esm`/`.esl` plugins dropped into `Data/` are recognized directly by the
  scanner and bare-layout policy; no scripted installer is required.
- There is **no REDmod, SMAPI, or `.pak` step** for Starfield — those belong to
  other engines (Cyberpunk, Stardew Valley, and Unreal/Larian titles
  respectively). Starfield is a plugin + `.ba2` Creation Engine title.

## Gaming tools that matter

Starfield ships with **no built-in OptiScaler profiles** (`optiscaler_profiles`
is empty in its registration), so unlike Stellar Blade it has no preconfigured
OptiScaler/OptiPatcher setup. You can still manage the generic gaming tools
through modde's tool system:

- **`proton`** — per-game Proton version, Wine prefix, environment variables, and
  DLL overrides. This is the most relevant tool for Starfield on Linux; several
  script-extender and ASI-loader mods need forced DLL overrides (see below).
- **`mangohud`**, **`vkbasalt`**, **`gamemode`** — performance overlay, Vulkan
  post-processing, and the GameMode daemon.
- **`reshade`** / **`optiscaler`** — file-patching tools that place DLLs and
  configs into the game directory; modde records what it patched so `tool revert`
  can cleanly remove them.

Named **executables** (script extenders, xEdit/SF1Edit-style tools, ASI managers)
are managed through modde's Executables system with args, working directory,
environment, Wine DLL overrides, a configurable output mod, and overwrite capture.
See the [Tools & overlays guide](../guides/tools.md) for `modde tool` and
`modde exec` usage.

## Linux / Proton notes & known gotchas

These notes cover the game's runtime on Linux and macOS, where Starfield runs
through Proton/Wine. On Windows the game runs natively — there is no Wine prefix,
and the Saves directory, `plugins.txt`, and INI files live at their native
Windows locations. modde itself runs natively on all three platforms.

- **Launch the game once before expecting save or load-order detection.** Both the
  Saves directory and `plugins.txt` live inside `compatdata/1716740/pfx`, which
  Proton only creates after the first launch. Until then, save detection returns
  empty and the scanner falls back to scanning loose `Data/` plugins without an
  authoritative order.
- **DLL-based mods (script extenders, ASI loaders) usually need DLL overrides.**
  Configure them through the `proton` tool rather than hand-editing the prefix:

  ```bash
  modde tool configure proton --game starfield dll_override_mode=forced forced_dll_overrides=sfse_loader
  ```

  Use whatever proxy/loader DLL name the specific mod requires.
- **`StarfieldCustom.ini` may need creating.** Many loose-file and archive-invalidation
  workflows expect a `StarfieldCustom.ini` in the My Games folder; modde manages
  both `StarfieldPrefs.ini` and `StarfieldCustom.ini` per profile, but the game
  itself does not always ship a `Custom` INI by default.
- **Steam Cloud-aware saves.** modde preserves Steam's cloud marker and parks
  inactive root saves under its own `.modde/` directory during profile switches,
  so cloud-synced Starfield saves are not clobbered. See the
  [Save management guide](../guides/saves.md) for the Steam Cloud behavior.

## Worked example

### Home-Manager profile

Declare a Starfield profile with the home-manager module. You can declare it
before the game is installed and let modde wait for the install:

```nix
{ inputs, ... }:
{
  imports = [ inputs.modde.homeManagerModules.modde ];

  programs.modde = {
    enable = true;

    profiles.my-starfield = {
      game = "starfield";
      # Wait for Steam/Proton to create the install before deploying.
      installMode = "await-game";
      # Once Steam has installed the game, set the Data/-bearing install root:
      # gameDir = "/path/to/SteamLibrary/steamapps/common/Starfield";
    };
  };
}
```

After Steam has installed Starfield, set `gameDir` to the install root (the folder
containing `Data/`) and rebuild; modde deploys the profile via its activation
script.

### CLI

```bash
# Deploy the Starfield profile, then launch through modde (captures saves on exit)
modde play --game starfield --profile my-starfield

# Or deploy without launching
modde deploy --profile my-starfield --game starfield

# Scan the install for plugins and loose .ba2 archives
modde scan --game starfield

# Adopt pre-existing saves into a profile (first capture in the vault)
modde save adopt --game starfield --profile my-starfield

# Manually capture .sfs saves with a message
modde save capture --game starfield --profile my-starfield -m "before the Unity"

# Watch for new saves when launching outside `modde play`
modde save watch --game starfield --interval 60

# Configure forced DLL overrides for a script-extender / ASI loader
modde tool configure proton --game starfield dll_override_mode=forced forced_dll_overrides=sfse_loader

# Run an external tool (e.g. a conflict cleaner) with overwrite capture
modde tool run /path/to/sf-tool --game starfield -- --some-flag
```

## See also

- [Supported games](supported-games.md) — the canonical status table and Wabbajack mapping
- [Deployment & VFS](../guides/deployment.md) — the symlink-farm deploy model used for `Data/`
- [Save management](../guides/saves.md) — the git-backed save vault and fingerprinting
- [Conflicts](../guides/conflicts.md) — inspecting and resolving the conflicts modde classifies
- [FOMOD installers](../guides/fomod.md) — scripted-installer option trees
- [Tools & overlays](../guides/tools.md) — Proton, OptiScaler, executables, and `modde tool`
- [Scanning](../guides/scanning.md) — how `modde scan` discovers installed mods
- [Parity audit](../reference/parity.md) — what is `Done` vs `Partial`
