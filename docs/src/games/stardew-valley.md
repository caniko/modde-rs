# Stardew Valley

Stardew Valley is modde's SMAPI-engine title. It is the only game in the registry
on the `Smapi` engine family, and unlike the Bethesda, REDengine, and Unreal games
it is a managed-content loader: mods are self-describing directories under `Mods/`
rather than archives, plugins, or `.pak` overlays. This page documents exactly
what modde knows about the title, sourced from
[`crates/modde-games/src/stardew/`](https://github.com/caniko/rs-modde/blob/trunk/crates/modde-games/src/stardew).

## Engine and overall status

| Property | Value |
| -------- | ----- |
| `game_id` | `stardew-valley` |
| Display name | Stardew Valley |
| Engine family | `Smapi` |
| Loader | SMAPI (Stardew Modding API) |
| Steam App ID | `413150` |
| Nexus domain | `stardewvalley` |
| Overall status | **Partial** |
| Scanner | Yes |
| Conflict detection | Yes (generic policy classifier) |
| Save tracking | Done |

The **Partial** rating matches [the supported-games matrix](supported-games.md):
the core path — detection, the SMAPI mod layout, scanning, conflict
classification, and save tracking — is implemented, but it has not been promoted
to `Done` the way the Bethesda Creation Engine titles and Cyberpunk 2077 have.
Treat installer coverage and end-to-end UX as work in progress, not as a turnkey
SMAPI manager. The canonical, test-coupled status baseline lives in
[`docs/capability-matrix.toml`](https://github.com/caniko/rs-modde/blob/trunk/docs/capability-matrix.toml).

## How modde detects the install

modde resolves the Stardew Valley install through its launcher IDs. The
registration carries a Steam App ID of `413150` and a Steam library directory name
of `Stardew Valley`, so a Steam install is located by walking your Steam libraries
for `steamapps/common/Stardew Valley`. No Heroic GOG or Epic IDs are registered for
this title, so Heroic auto-detection is not wired up — point modde at the install
directory directly if you run it from GOG or another launcher.

Stardew Valley runs natively on Linux. The default Steam Linux build is a native
Linux binary, so there is **no Proton prefix involved** for the base game, and
modde does not need to resolve a Wine prefix to find the install or the saves. (You
can still choose to run the Windows build under Proton — see
[Linux/Proton notes](#linux-and-proton-notes) — but that is not the default path.)

You can always override detection and pass the resolved game directory explicitly
when a command needs it; the game directory is the folder that contains the
`Stardew Valley` executable and the `Mods/` directory.

## Mod directory and deploy strategy

modde's plugin resolves the mod directory to `Mods/` directly under the install
root (`<install>/Mods`). This is SMAPI's standard mod folder: SMAPI loads every
subdirectory of `Mods/` that contains a `manifest.json`.

Deployment uses modde's standard **symlink-farm VFS** — the same pipeline as every
other game. Mods are staged in `~/.local/share/modde/staging/<profile>/`, the
winning file for each relative path is resolved by load-order priority, and the
staging tree is symlinked into `Mods/`. Your real Stardew install is never mutated
in place, so deactivating modde or switching profiles is fully reversible. The
plugin's `executable_dir` is the install root itself, which is where overlay-style
tools (proxy DLLs) land when applicable.

See [Deployment & VFS](../guides/deployment.md) for the full pipeline and rollback
behaviour.

## What scanning finds

`modde scan --game stardew-valley` discovers SMAPI mods already on disk. The
scanner walks the `Mods/` directory and applies a directory-mod rule:

- Each immediate subdirectory of `Mods/` is a candidate mod.
- The presence of a **`manifest.json`** marker file inside a candidate raises
  detection confidence to `0.98` — that file is SMAPI's own mod manifest and is the
  authoritative signal that a directory is a SMAPI mod.
- Directories without the marker are still reported at a lower base confidence
  (`0.75`), so loose content packs are not silently dropped.
- Discovered mods are namespaced under a `smapi/` mod-id prefix, and a mod's
  on-disk footprint is recorded as `mods/<name>/` (lower-cased), reflecting that
  SMAPI directory names are matched case-insensitively.

Import discovered mods straight into a profile:

```bash
modde scan --game stardew-valley --import-to my-stardew
```

This is the path for adopting an existing hand-managed `Mods/` folder into modde.
See [Mod Scanning](../guides/scanning.md) for dry-run mode and threshold filtering.

## Conflict classification specifics

Stardew Valley uses modde's **generic policy collision classifier** (it has no
bespoke per-game collision classifier the way Bethesda and Cyberpunk do). The
generic classifier registers **no archive extensions** — SMAPI mods are loose
directory content, not packed archives — and applies the default severity table to
overlapping files:

| Extension | Severity |
| --------- | -------- |
| `dll`, `lua` | Dangerous (critical) |
| `json`, `xml`, `ini` | Config |
| `png`, `jpg`, `dds`, `tga`, `nif` | Cosmetic |

In practice that means two content packs that both ship `assets/foo.png` collide
as a **cosmetic** conflict (last in load order wins), while two mods that both
ship a SMAPI `.dll` or a Content Patcher-style `content.json` are flagged at the
more serious end. Resolve conflicts by load-order priority exactly as for any other
game:

```bash
modde collisions --profile my-stardew
modde collisions --profile my-stardew --all   # include cosmetic conflicts
```

Separately from collision severity, the plugin also classifies each *mod* for
**save safety**, which feeds save fingerprinting (next section):

| Class | Extensions |
| ----- | ---------- |
| Save-breaking | `dll`, `json`, `xnb`, `tmx`, `tbin` |
| Cosmetic (save-safe) | `png`, `jpg`, `xnb` |

A mod containing any save-breaking extension is classified `SaveBreaking`; a mod
that contains only cosmetic files is `SaveSafe`; an empty or unrecognised mod is
`Unknown` and treated conservatively as save-breaking for fingerprinting. Note that
`xnb` appears in both lists — it is content that can be either gameplay data
(save-breaking) or a recolour (cosmetic), and modde errs toward the save-breaking
reading because any save-breaking extension in the mod wins. For per-file content
categorisation, extensions map as: `dll` → binary, `json`/`tmx`/`tbin` → config,
`xnb` → archive, `png`/`jpg` → texture.

See [Conflicts & Load Order](../guides/conflicts.md) for severity meanings and the
hide workflow.

## Save tracking

Save tracking is **Done** for Stardew Valley. The save directory is the native
Linux XDG config location:

```
~/.config/StardewValley/Saves
```

modde resolves this through its platform-aware config-directory helper, so on Linux
it honours `$XDG_CONFIG_HOME` (defaulting to `~/.config`). Each farm is its own
subdirectory named `<FarmName>_<id>` (e.g. `Pelican_123456789`).

The save tracker enumerates **directories** under the saves folder whose name
contains an underscore — that is the `<name>_<id>` farm convention — and reports
each as a save in the `farm` category, labelled by its directory name. The
`startup_preferences` entry is excluded. Results are sorted newest-first by
modification time.

`supports_save_profiles` is `true`, so Stardew participates in modde's full
git-backed save vault: each profile is a branch, switching profiles captures and
restores the appropriate farm snapshots, and every snapshot embeds a **SHA-256
fingerprint** computed from the sorted list of enabled save-breaking mods (the
classification above). On restore, if the snapshot's fingerprint does not match
your current profile, modde warns you which save-breaking mods were added or
removed — so you do not load a farm into a mod set it was never saved against.

```bash
# Adopt an existing farm into a profile
modde save adopt --game stardew-valley --profile my-stardew

# Capture, browse history, and restore
modde save capture  --game stardew-valley --profile my-stardew -m "year 2 spring"
modde save history  --game stardew-valley --profile my-stardew
modde save restore  abc12345 --game stardew-valley --profile my-stardew
```

See [Save Management](../guides/saves.md) for the vault model, auto-capture, and
the fingerprint trailer format.

## Plugin and load-order handling

Stardew Valley has **no Bethesda-style plugin (`.esp`/`.esm`) load order**. SMAPI
loads mods by their `manifest.json` and resolves inter-mod dependencies itself at
runtime. modde therefore does not manage an external plugin/load-order file for
this title; ordering is handled entirely through modde's standard mod load-order
priority (later mods win file conflicts during deployment), which is what the
collision classifier and the VFS builder consume. There is no separate plugin
table to edit.

## Installer specifics

There are no FOMOD, REDmod, BAIN, or `.pak` flows for Stardew Valley — those are
Bethesda/Cyberpunk/Unreal concepts. The plugin recognises two SMAPI-native archive
layouts when you install a downloaded mod:

1. **SMAPI directory mod** — if the extracted archive root contains a
   `manifest.json`, the archive *is* one SMAPI mod. It installs as a
   `DirectoryMod` (the whole directory is staged under a stable mod directory). This
   is the common case for a single SMAPI mod packaged at the archive root.
2. **`Mods/`-rooted archive** — if the extracted root instead contains a `Mods/`
   directory, modde strips that content root (`StripContentRoot { root: "Mods" }`)
   so the inner mod folders stage directly into the game's `Mods/`. This handles
   archives that bundle one or more mods already nested under a `Mods/` folder.

For bare-layout recognition (used when an archive has no obvious nesting), the
plugin accepts a root `manifest.json`, or a `mods/` directory, or root files with a
`json`/`dll` extension; directory matching is case-insensitive. Anything modde
cannot classify falls through to the generic installer, which writes a dossier so
the layout can be handled later — it is not silently discarded. See the
[FOMOD Installer](../guides/fomod.md) guide for the interactive/declarative
installer machinery that the Bethesda titles use; Stardew does not need it.

## Gaming tools that matter

Stardew Valley registers **no OptiScaler profiles** — it is a 2D pixel-art game, so
DLSS/FSR upscaling is not relevant, and there is no Unreal-style upscaler swap to
manage. The tools that matter here are modde's general overlay/launcher tools:

- **MangoHud / GameMode** — useful as always for an FPS overlay and CPU governor
  tuning, even on a lightweight title.
- **Proton** — only relevant if you deliberately run the Windows build under Proton
  (see below). The `proton` tool stores launch/compat settings; it does not install
  Proton or the game.

See [Tools & Overlays](../guides/tools.md) for enabling and configuring these.

## Linux and Proton notes

- **Native Linux is the default and the happy path.** The Steam Linux depot ships a
  native build, SMAPI has a first-class Linux installer, and the save directory is
  the native `~/.config/StardewValley/Saves`. No Wine prefix is involved, and modde
  does not look for one.
- **SMAPI itself must be installed and set as the launch command.** modde manages
  your `Mods/` content; it does **not** install or bootstrap SMAPI. After running
  SMAPI's installer, set Stardew's Steam launch options to run SMAPI (the standard
  `StardewModdingAPI %command%` pattern) so the loader actually starts.
- **If you run the Windows build under Proton on Linux instead**, the save directory
  moves into the Proton prefix's emulated `%APPDATA%`
  (`steamapps/compatdata/413150/pfx/drive_c/users/steamuser/AppData/Roaming/StardewValley/Saves`)
  rather than `~/.config/StardewValley/Saves`. On Linux modde's save tracker targets
  the native Linux path, so prefer the native Linux build to keep save tracking
  accurate; running the Windows build under Proton on Linux is a known gotcha for the
  save vault. (On Windows itself, where the native build is the only option, the
  tracker targets the native Windows save path, and on macOS the native build's
  config path.) modde itself runs natively on Linux, macOS, and Windows.
- **Case sensitivity.** SMAPI mod directory names are matched case-insensitively by
  modde's scanner, which is the right behaviour for mods authored on Windows but
  deployed onto a case-sensitive Linux filesystem.

## Worked example

### Home-Manager profile

Declare a Stardew profile in your home-manager configuration. You can declare it
before the game is installed and let modde wait for the Steam install:

```nix
{ inputs, ... }:
{
  imports = [ inputs.modde.homeManagerModules.modde ];

  programs.modde = {
    enable = true;

    profiles.my-stardew = {
      game = "stardew-valley";
      # Wait for Steam to install the game, then set gameDir and rebuild.
      installMode = "await-game";
    };
  };
}
```

Once Steam has installed the game, set `gameDir` to the resolved install path and
rebuild; modde deploys the profile through its activation script. See the
[Home-Manager module reference](../configuration/hm-module.md) for the full option
set.

### CLI

```bash
# 1. Adopt an already-managed Mods/ folder into a profile
modde scan --game stardew-valley --import-to my-stardew

# 2. Inspect conflicts (cosmetic png overlaps are expected and harmless)
modde collisions --profile my-stardew --all

# 3. Deploy the symlink farm into <install>/Mods
modde deploy --profile my-stardew --game stardew-valley

# 4. Adopt an existing farm and capture a baseline snapshot
modde save adopt   --game stardew-valley --profile my-stardew
modde save capture --game stardew-valley --profile my-stardew -m "baseline"

# 5. Play (SMAPI must already be set as the launch command); saves capture on exit
modde play --game stardew-valley
```

## See also

- [Supported Games](supported-games.md)
- [Baldur's Gate 3](baldurs-gate3.md) — the other non-Bethesda/non-Creation-Engine title
- [Deployment & VFS](../guides/deployment.md)
- [Mod Scanning](../guides/scanning.md)
- [Conflicts & Load Order](../guides/conflicts.md)
- [Save Management](../guides/saves.md)
- [Tools & Overlays](../guides/tools.md)
- [Playing a Game](../guides/playing.md)
- [Feature Parity](../reference/parity.md)
