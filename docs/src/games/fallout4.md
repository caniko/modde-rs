# Fallout 4

Fallout 4 (`game_id` **`fallout4`**, Steam App ID **377160**) is a fully shipped
Bethesda title in modde: scanning, conflict detection, plugin load order, and
save tracking all work end to end. Its overall status is **`Done`** in the
[supported-games table](supported-games.md) and the
[parity audit](../reference/parity.md). It is one of five Creation Engine games
modde drives through a single data-driven `BethesdaGame` plugin, sharing its
deploy strategy and conflict logic with Skyrim SE/AE, Fallout 76, and Starfield.

## Engine & overall status

| Property | Value |
| -------- | ----- |
| Engine family | Bethesda Creation Engine (`EngineFamily::Bethesda`) |
| `game_id` | `fallout4` |
| Display name | Fallout 4 |
| Steam App ID | `377160` |
| Heroic (GOG) app ID | `1998527297` |
| Nexus domain | `fallout4` (numeric game ID `1151`) |
| Wabbajack name | `Fallout4` |
| Mod directory | `Data/` (relative to the install root) |
| Archive format | `.ba2` (Bethesda Archive v2) |
| Managed INI files | `Fallout4.ini`, `Fallout4Prefs.ini`, `Fallout4Custom.ini` |
| Save format | `.ess` (`FO4_SAVEGAME` magic) |
| Plugin system | Yes (`plugins.txt`, ESP/ESM/ESL) |
| Save profiles | Enabled (`supports_save_profiles = true`) |
| Overall status | **`Done`** |

Because Fallout 4 is part of the Bethesda plugin family, it inherits the full
Creation Engine feature set documented for [Skyrim SE/AE](skyrim-se.md) and in
the [Conflicts & load order](../guides/conflicts.md) guide:
`plugins.txt` read/write, a pure-Rust LOOT masterlist parser, binary plugin
header validation (Form 43 / missing-master detection), and archive-aware
conflict analysis.

## Install detection

modde locates a Fallout 4 install through its registered launcher IDs, in this
priority order:

1. **Steam** — App ID `377160`, install directory
   `steamapps/common/Fallout 4`. This is the primary, best-tested detection path.
2. **Heroic (GOG)** — GOG app ID `1998527297`, for the GOG build (on Linux/macOS
   run through Heroic + Proton/Wine; on Windows it runs natively).

There is no Epic launcher mapping for Fallout 4. The install root is the
directory that contains `Fallout4.exe` and the `Data/` folder; you can also point
modde at it explicitly with `gameDir` (home-manager) or `--game-dir` (CLI) when
auto-detection cannot reach it.

### Proton prefix

On Linux, Fallout 4 runs under Proton, so the Windows-side paths modde needs
(`plugins.txt`, the save folder, the per-user `My Games` tree) live inside the
Steam compatibility prefix rather than in your real home directory. modde
resolves them under the App-ID-keyed `compatdata` prefix:

```text
~/.local/share/Steam/steamapps/compatdata/377160/pfx/drive_c/users/steamuser/
```

`plugins.txt` is read from and written to:

```text
.../compatdata/377160/pfx/drive_c/users/steamuser/AppData/Local/Fallout4/plugins.txt
```

and saves from the `My Games` tree (see [Save tracking](#save-tracking)). If the
prefix has not been created yet — for example you have installed the game but
never launched it — these paths will not exist, and modde degrades gracefully:
the scanner falls back to a plain directory walk and the save tracker reports no
saves rather than erroring.

## Mod directory & deploy strategy

The mod directory is **`Data/`** under the install root (`mod_directory` returns
`install.join("Data")`). Every plugin, archive, and loose asset Fallout 4 loads
lives here.

modde deploys with its standard **symlink farm**: mods are staged in
`~/.local/share/modde/staging/<profile>/`, conflicts are resolved into a single
winning layout, and that layout is symlinked into `Data/`. Your real game
install is never overwritten, and a rollback restores the previous deployment
atomically. See [Deployment & VFS](../guides/deployment.md) for the full build →
materialize → deploy pipeline and `modde rollback`.

`.ba2` archives are deployed **as-is, not extracted** — the Creation Engine loads
them natively, so modde places the archive straight into `Data/` and lets the
engine mount it. This matches the bare-layout recognizer, which treats a top-level
`Data/`, loose `.esp`/`.esm`/`.esl`/`.ba2` files, or the usual asset folders
(`meshes/`, `textures/`, `scripts/`, `interface/`, `sound/`, `materials/`,
`strings/`, …) as a valid Bethesda mod root even when the archive was packed
without a containing folder.

## What scanning finds

The Fallout 4 scanner (`FALLOUT4_SCANNER`, a `BethesdaScanner`) discovers
installed mods by walking `Data/` and cross-referencing `plugins.txt`:

1. **Authoritative pass.** It reads `plugins.txt` from the Proton prefix and, for
   every listed plugin that exists on disk, emits a `plugin/<filename>` mod at
   confidence **0.95**. For each plugin it also pairs **companion archives** that
   share the plugin's stem — both `<stem>.ba2`/`<stem>.bsa` and the texture-archive
   convention `<stem> - Textures.ba2` — so a plugin and its assets are reported as
   one mod.
2. **Unmanaged pass.** It then scans `Data/` for any `.esp`/`.esm`/`.esl` files
   **not** present in `plugins.txt` (disabled or hand-dropped plugins) and reports
   them at confidence **0.8**, again pulling in same-stem archives.

Scan results feed duplicate detection: each discovered mod exposes a Data-relative
footprint (`plugin/Foo.esp` → `foo.esp`) so modde can spot files that are already
deployed loose in the game folder versus managed by a profile. See
[Scanning installed mods](../guides/scanning.md) for the shared workflow.

## Conflict classification

Conflicts are classified by the shared `BethesdaCollisionClassifier`, which is
**archive-aware**: it reads the file listing inside each `.ba2`/`.bsa` and folds
those entries into the same collision map as loose files, so two archives that
both ship `textures/foo.dds` are flagged even though neither was extracted.

Severity is assigned per file extension:

| Severity | Extensions | Meaning |
| -------- | ---------- | ------- |
| **Dangerous** | `.esp`, `.esm`, `.esl`, `.pex`, `.dll`, `.psc` | Plugins, compiled/source Papyrus scripts, native DLLs — overrides here change game logic and can break saves |
| **Config** | `.ini`, `.cfg`, `.json`, `.toml`, `.xml` | Settings files; later mod wins but the change is usually intentional |
| **Cosmetic** | `.dds`, `.png`, `.tga`, `.jpg`, `.nif`, `.hkx`, `.fuz`, `.wav`, `.xwm`, `.swf`, `.btr`, `.bto`, `.btt`, `.bsa`, `.ba2` | Meshes, textures, sound, animation, UI — last-writer-wins is normally safe |

This is the same severity ladder modde uses to decide which collisions to surface
loudly. See [Conflict detection](../guides/conflicts.md) for how severities drive
the conflict report and how load-order priority resolves a winner.

### Mod-safety classification

Independently of collisions, modde tags whole mods as **save-breaking** when they
contain `.esp`/`.esm`/`.esl`/`.pex`/`.dll`/`.psc`, and **cosmetic** when they only
contain art/sound/archive/config assets. This drives the warnings you see before
enabling a mod mid-playthrough.

## Plugin & load-order handling

Fallout 4 has a real plugin system (`has_plugin_system()` is `true`), and modde
manages it as a first-class, dual-pane concern:

- **`plugins.txt` read/write.** modde parses the standard format — `*Plugin.esp`
  = enabled, `Plugin.esp` = disabled, `#…` = comment — from the Proton prefix and
  writes it back in the same `*`-prefixed form.
- **Dual-pane order.** Mod install priority ("which mod's files win") is kept
  separate from plugin load order ("which `.esp` loads first"), stored in a
  dedicated `plugin_order` table per profile — MO2's core insight.
- **LOOT masterlist.** modde ships a pure-Rust parser for LOOT's public YAML
  masterlist; for Fallout 4 it pulls the `loot/fallout4` repository and turns the
  `after`/`requires`/`incompatible` rules for your active plugins into modde load-order
  rules.
- **Header validation.** A binary header reader inspects the first ~1 KB of each
  plugin to extract the form version, ESM/ESL flags, and master list, flagging
  Form 43 plugins and missing masters before they cause a crash on load.

```bash
# Auto-sort the Fallout 4 load order against the LOOT masterlist
modde loot sort --game fallout4

# Validate plugin headers (Form version, missing masters)
modde loot validate --game fallout4
```

> Not yet shipped for any Bethesda title: plugin **pinning** ("lock load order"),
> a dedicated Archives tab, archive content preview/packing, and per-mod INI
> tweak editing. LOOT sorting prints rules to the terminal rather than a rich
> report. See the [MO2 parity audit](../reference/parity.md) for the full
> feature comparison.

## Save tracking

Save tracking for Fallout 4 is **`Done`** and opted into the per-profile save
layer (`with_save_profiles(true)`).

modde locates saves inside the Proton prefix at:

```text
.../compatdata/377160/pfx/drive_c/users/steamuser/Documents/My Games/Fallout4/Saves
```

The Fallout 4 save tracker (`FALLOUT4_SAVE_TRACKER`) scans that directory for
**`.ess`** save files (`.bak` backups are skipped) and, for each one, parses the
binary header to fingerprint it:

- It verifies the **`FO4_SAVEGAME`** magic so a stray Skyrim/Starfield save in the
  same folder is not mislabelled.
- From the header it reads the **save number** (the slot ID that increments each
  save) and the **player name** (a length-prefixed UTF-8 string), producing a
  label like `Sole Survivor — Save 7`.
- Slot type is inferred from the filename: `Autosave…` → `auto`, `Quicksave…` →
  `quick`, everything else → `manual`.
- A save whose header cannot be parsed is still captured, just without a label.

Captures are summarized newest-first and grouped by character, so a capture
message reads like `capture: 3 saves — Sole Survivor (slots 5, 6); …`. See
[Save tracking & profiles](../guides/saves.md) for committing, browsing, and
restoring save snapshots.

## Installer specifics

- **FOMOD** is fully supported. When a downloaded mod contains a
  `fomod/ModuleConfig.xml`, modde detects it and runs the installer — interactively
  via the GUI wizard, or non-interactively from a saved choices file. Generate a
  template with `modde fomod generate`, inspect options with `modde fomod inspect`,
  and replay choices with `--fomod-config`. See the
  [FOMOD installer guide](../guides/fomod.md).
- **BA2 archives** are deployed verbatim (never repacked or extracted), and their
  contents are still indexed for conflict analysis as described above.
- **Loose-file mods** (a bare `Data/` tree or top-level asset folders) are
  recognized by the Bethesda bare-layout policy without any installer.
- Fallout 4 does **not** use REDmod (Cyberpunk), Unreal `.pak`, or SMAPI installers
  — those belong to other engine families. Its installer story is FOMOD + loose
  files + BA2, the standard Creation Engine toolkit.

## Gaming tools & overlays

Fallout 4 launches through modde's [Tools & Overlays](../guides/tools.md) layer
like any other game. The settings that matter most for this title:

- **`proton`** — pin a Proton/GE-Proton runner, set `extra_env`, override the
  Wine prefix, or force DLL overrides. Fallout 4 Script Extender (F4SE) and many
  script-extender-dependent mods need the right runner and, occasionally, forced
  overrides; `dll_override_mode=forced` with `forced_dll_overrides=…` is how you
  apply them through modde's launcher integration.
- **`mangohud`**, **`vkbasalt`**, **`gamemode`**, **`reshade`** — performance
  overlay, post-processing, the Feral GameMode daemon, and the ReShade shader
  framework, enabled and configured per game with `modde tool enable/configure`.
- **Overwrite capture.** Tools that rewrite files in `Data/` (xEdit/FO4Edit,
  BodySlide, Material Editor) can be run through `modde tool run … --game fallout4`
  so their output is captured into an `__overwrite__` mod and survives
  redeployment.

> modde ships **no OptiScaler profile** for Fallout 4 (`optiscaler_profiles` is
> empty for this title). OptiScaler can still be applied as a generic tool, but
> there is no curated Fallout-4-specific upscaling profile the way there is for
> Stellar Blade.

## Linux / Proton notes & gotchas

These notes apply to the game's runtime on Linux and macOS, where Fallout 4 runs
through Proton/Wine. On Windows the game runs natively — there is no Wine prefix,
and `plugins.txt`, the `My Games/Fallout4` tree, and the INI files live at their
native Windows locations. modde itself runs natively on all three platforms.

- **Launch the game once first.** Until Proton has created the `compatdata/377160`
  prefix, `plugins.txt`, the `My Games/Fallout4` save tree, and the per-prefix INI
  files do not exist. Run the game once so the prefix is populated, then let modde
  manage it.
- **INI files live in the prefix.** `Fallout4.ini`, `Fallout4Prefs.ini`, and
  `Fallout4Custom.ini` are under the prefix's `My Games/Fallout4`, not your Linux
  `$HOME`. modde patches them comment- and formatting-preserving rather than
  rewriting them wholesale. Creating `Fallout4Custom.ini` and enabling loose-file
  loading is still your responsibility, exactly as on Windows.
- **Case sensitivity.** Bethesda assets assume Windows' case-insensitive paths.
  The bare-layout recognizer matches the standard asset directories
  case-insensitively, but a poorly packed mod with an unexpected directory name
  can still mis-deploy on a case-sensitive Linux filesystem — check the conflict
  report if assets do not load.
- **Script extender.** F4SE is installed into the game root like on Windows and
  launched through the script-extender executable; configure the runner with the
  `proton` tool and, if a plugin needs it, forced DLL overrides.
- **GOG via Heroic.** The GOG build (Heroic app ID `1998527297`) works, but Steam
  + Proton is the most exercised path; the GOG prefix layout differs, so verify
  the detected `gameDir` and save path if you use it.

## Worked example

### Home-Manager profile

```nix
{ inputs, ... }:
{
  imports = [ inputs.modde.homeManagerModules.modde ];

  programs.modde = {
    enable = true;

    profiles.fo4-vanilla-plus = {
      game = "fallout4";
      # Leave gameDir unset (or installMode = "await-game") until Steam/Heroic
      # has installed Fallout 4 and you have launched it once to build the prefix.
      installMode = "await-game";
    };
  };
}
```

Once Steam has installed Fallout 4 and you have run it once, set `gameDir` to the
install root (the folder containing `Fallout4.exe` and `Data/`) and rebuild;
modde deploys the profile through its activation script.

### CLI

```bash
# Install a Nexus mod into a Fallout 4 profile, applying saved FOMOD choices
modde install mod https://nexusmods.com/fallout4/mods/12345 \
  --profile fo4-vanilla-plus \
  --fomod-config my-choices.toml

# Discover what is already in Data/
modde scan --game fallout4

# Sort and validate the load order against the LOOT masterlist
modde loot sort --game fallout4
modde loot validate --game fallout4

# Deploy, then launch through Proton
modde deploy --profile fo4-vanilla-plus --game fallout4
modde play --game fallout4

# Capture xEdit/FO4Edit edits into an __overwrite__ mod
modde tool run /path/to/FO4Edit --game fallout4 -- -quickautoclean
```

## See also

- [Supported games](supported-games.md) — the status baseline and full game list
- [Skyrim SE/AE](skyrim-se.md), [Starfield](starfield.md) — sibling Creation Engine titles
- [Deployment & VFS](../guides/deployment.md) — the symlink-farm deploy pipeline
- [Conflict detection](../guides/conflicts.md) — severities and winner resolution
- [Scanning installed mods](../guides/scanning.md) — what the scanner reports
- [Save tracking & profiles](../guides/saves.md) — committing and restoring saves
- [FOMOD installer](../guides/fomod.md) — interactive and declarative installs
- [Tools & Overlays](../guides/tools.md) — Proton, OptiScaler, ReShade, overwrite capture
- [Parity audit](../reference/parity.md) — what is `Done` vs `Partial`
