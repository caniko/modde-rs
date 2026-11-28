# Fallout: New Vegas

modde supports Fallout: New Vegas (`game_id` `fallout-new-vegas`) and
[Oblivion](oblivion.md) through the same built-in, data-driven **Gamebryo**
plugin. A `GamebryoGame` record sets each game's Steam app id, save folder name,
INI set, and Nexus domain.

> **Overall status: `Partial`.** Deployment, filesystem scanning, BSA-aware
> conflict classification, save tracking, and `plugins.txt` load-order
> read/write all ship. New Vegas falls short of `Done`: it lacks a dedicated
> end-to-end script-extender (NVSE) setup and the deeper plugin-management UX
> (automatic master/ESM ordering, in-app conflict resolution) of Bethesda
> Creation-Engine titles. Treat New Vegas as
> a solid deploy-and-scan target, not a full Mod-Organizer replacement. The
> canonical status baseline is [`docs/capability-matrix.toml`](../reference/parity.md)
> and the [supported-games table](supported-games.md).

## Engine and overall status

| Property | Value |
| -------- | ----- |
| Engine family | Gamebryo (`EngineFamily::Gamebryo`) |
| modde `game_id` | `fallout-new-vegas` |
| Display name | Fallout: New Vegas |
| Mod directory | `<install>/Data` |
| Archive format | `.bsa` (Bethesda archives) |
| Plugin system | Yes — `.esp` / `.esm`, ordered via `plugins.txt` |
| Script extender | NVSE (`.nvse`) — treated as save-breaking binary content |
| INI files | `Fallout.ini`, `FalloutPrefs.ini`, `FalloutCustom.ini` |
| Save format | `.ess` (save), `.fos` (co-save) |
| Nexus domain | `newvegas` |
| OptiScaler profiles | None shipped for this title |
| Overall status | `Partial` |

The plugin advertises `has_plugin_system() == true` and
`supports_save_profiles() == true`, so New Vegas participates in modde's
load-order and save-profile machinery.

## How modde detects the install

New Vegas is registered with a Steam launcher binding and **no** Heroic
GOG/Epic ids:

| Launcher key | Value |
| ------------ | ----- |
| `steam_app_id` | `22380` |
| `steam_dir` | `Fallout New Vegas` |
| `heroic_gog_app_id` | _(none)_ |
| `heroic_epic_app_id` | _(none)_ |

modde searches your Steam libraries for app `22380` at
`steamapps/common/Fallout New Vegas`. Without a Heroic binding, modde cannot
auto-detect GOG/Epic copies. For a non-Steam copy, set the directory with
`--game-dir` on the CLI or `gameDir` in the home-manager module.

The numeric Nexus game id is intentionally `None` for New Vegas: the
`newvegas` **domain** is enough for URL-based installs and update tracking, but
the in-app "Browse Nexus" picker (which needs the numeric id) will not list
this title until the id is confirmed. URL installs from
`nexusmods.com/newvegas/mods/<id>` resolve correctly via the domain.

## Mod directory and deploy strategy

The mod directory is `<install>/Data` — the same `Data/` folder New Vegas
itself reads plugins, meshes, textures, sound, and BSAs from.

The symlink farm stages mod files, picks the winner at each relative path by
load order, and links the winning paths into `Data/` rather than copying mod
contents there. This standard **symlink-farm** virtual filesystem keeps the base
install clean and each deploy reversible. See [Deployment & VFS](../guides/deployment.md).

When modde unpacks a downloaded archive, the Gamebryo plugin's
`analyze_mod_archive` looks for a top-level `Data/` directory inside the
extracted mod. If present, it applies `StripContentRoot { root: "Data" }`, so a
mod packaged as `Data/MyMod.esp` deploys its contents directly into the game's
`Data/` rather than nesting a redundant `Data/Data/`. For archives that are not
wrapped in `Data/`, the plugin's **bare-layout** recogniser accepts the mod
when it sees the expected loose roots — `data`, `meshes`, `textures`, `sound`,
`music`, `menus`, `scripts`, `shaders` (matched case-insensitively) — or
top-level `.esp` / `.esm` / `.bsa` files. This is what lets older "drop into
Data" mods install without manual repackaging.

## What scanning finds

`modde scan --game fallout-new-vegas` runs the Gamebryo scanner over the single
`Data` directory. The scanner:

- reads the top level of `<install>/Data` (non-recursive);
- treats every `.esp` and `.esm` file as a discovered mod;
- pairs each plugin with a same-stem `.bsa` archive when one exists (e.g.
  `MyMod.esp` + `MyMod.bsa` are reported together as one mod's files);
- assigns each mod the id `plugin/<filename>` with a fixed match confidence of
  `0.85`;
- records each file's install-relative path and on-disk size.

The scanner does **not** report loose meshes/textures in `Data/` as distinct
mods without a plugin. Use `--manifest` to match an existing install against a
Wabbajack manifest (including New Vegas lists, see below). Matching uses the
scanner's per-mod footprint: the lowercased plugin filename.
See [Mod Scanning](../guides/scanning.md) for
threshold, dry-run, and import flags.

## Conflict classification

Two layers cooperate when New Vegas mods overlap.

**Per-file collision severity** (used when two deployed mods write the same
relative path) comes from the BSA-aware collision policy:

| Severity | Extensions |
| -------- | ---------- |
| `Dangerous` | `.esp`, `.esm`, `.dll` (also `.lua`, `.ws` from the shared default set) |
| `Config` | `.ini`, `.json`, `.xml` |
| `Cosmetic` | `.dds`, `.png`, `.jpg`, `.tga`, `.nif` |

The New Vegas classifier registers `.bsa` as the archive extension and compares
overlaps as packed content rather than loose files.

**Per-mod safety classification** (`classify_mod`, used to flag whether
installing/toggling a mod can invalidate saves) uses the Gamebryo content
policy:

- **Save-breaking** extensions: `.esp`, `.esm`, `.pex`, `.dll`, `.obse`,
  `.nvse`, plus anything under a `scripts/` directory. Adding or removing these
  changes the form-id / script footprint a save depends on.
- **Cosmetic** extensions: `.nif`, `.bsa`, `.dds`, `.png`, `.tga`, `.jpg`,
  `.kf`, `.wav`, `.mp3`, `.ogg`, `.ini`, `.xml`.

The same policy also classifies content into categories used across the UI and
reports — `Plugin` (`esp`/`esm`), `Texture`, `Mesh`, `Sound`, `Script`
(including `.obse`/`.nvse`), `Archive` (`bsa`), `Config`, and `Binary` (`dll`).
See [Conflicts](../guides/conflicts.md) for how severities surface in the
conflict report.

## Save tracking

New Vegas save tracking is **shipped (`Done`)**. On Linux and macOS the game runs
through Proton/Wine, so the Gamebryo save tracker points at the compatibility
prefix:

```
<Steam>/steamapps/compatdata/22380/pfx/drive_c/users/steamuser/Documents/My Games/FalloutNV/Saves
```

i.e. modde derives the save folder from the Steam compatdata directory for app
`22380`, then descends into the Wine prefix's
`Documents/My Games/FalloutNV/Saves`. (Note the on-disk folder name is
`FalloutNV`, not the modde id `fallout-new-vegas`.) On Windows the game runs
natively, so the same saves live at their native Windows location with no prefix
involved.

What is fingerprinted in that directory:

- files with the `.ess` (save) and `.fos` (co-save) extensions;
- `*.bak` backups are **excluded**;
- detection is non-recursive (the top level of `Saves` only);
- every match is filed under the `manual` category, with the relative filename
  as its label, sorted newest-first by modification time.

Capture summaries use the by-category form, e.g. `capture: 3 saves (3 manual)`,
so save-profile snapshots tell you how many saves were rolled into each
profile. Save profiles are enabled for this title. See
[Save Management](../guides/saves.md) for snapshot, restore, and per-profile
isolation.

## Plugin and load-order handling

New Vegas orders plugins through a `plugins.txt`-style file. modde ships
read/write helpers for that format:

- **Reading** strips comment lines (`#` or `;`), ignores blanks, and removes a
  leading `*` enabled-marker from each plugin name, yielding the ordered list of
  active plugins.
- **Writing** emits one plugin per line, each prefixed with `*` to mark it
  enabled (creating the parent directory if needed).

This gives modde a faithful round-trip of the active-plugin list. What is **not**
yet shipped for New Vegas is the higher-level plugin UX the Creation-Engine
titles have — automatic master/ESM sorting, LOOT-style ordering, or in-app
conflict resolution. Curate complex load orders with an external sorter and let
modde persist the result; this is part of why New Vegas is `Partial`.

## Installer specifics

- **FOMOD** is the primary scripted-installer path. New Vegas mods that ship a
  `fomod/ModuleConfig.xml` are driven through modde's FOMOD installer just like
  the Bethesda titles — see the [FOMOD Installers](../guides/fomod.md) guide
  for the option/flag selection flow.
- **`Data/`-rooted and bare archives** install without FOMOD via the
  `StripContentRoot` / bare-layout recognition described above.
- **BSA archives** (`.bsa`) deploy as opaque packed content; modde does not
  unpack them, and same-stem BSAs are tracked alongside their plugin.
- **NVSE / OBSE / script plugins** (`.nvse`, `.obse`, `.dll`) deploy as files
  into `Data/` and are flagged save-breaking, but modde does not currently
  bootstrap the NVSE loader executable for you — install the script extender
  itself manually into the game root, then manage its plugins through modde.
- REDmod, `.pak`, and SMAPI installers are **not applicable** to this engine.

## Gaming tools and Proton

New Vegas ships **no OptiScaler profiles** (`optiscaler_profiles` is empty for
this title), so the upscaler presets that exist for Unreal titles like
[Stellar Blade](stellar-blade.md) do not apply here. The engine-agnostic
overlays and the Proton tool still work: MangoHud, vkBasalt, ReShade, GameMode,
and the per-game `proton` tool (Proton build selection, Wine prefix, environment
variables, and DLL overrides) are all available through
[Tools & Overlays](../guides/tools.md). DLL overrides matter for script
extenders and ENB-style injectors that ship a `d3d9.dll`/`dxgi.dll` — set those
through the `proton` tool's override settings so the Wine prefix loads them.

## Linux / Proton notes and gotchas

New Vegas runs through Proton/Wine on Linux and macOS. On Windows it runs
natively, with no Wine prefix: saves and INI files use native Windows paths.
modde runs natively on all three platforms.

- **Saves live in the Proton prefix, not your home directory.** They are under
  `steamapps/compatdata/22380/pfx/...`, which is why a fresh prefix (deleting
  compatdata, switching Proton builds) appears to "lose" saves. modde reads them
  from exactly that path, so launch the game once under Proton before expecting
  save detection to find anything.
- **No Heroic detection ships for New Vegas.** GOG/Epic copies need an explicit
  game directory.
- **Run the game once before deploying** so Proton creates the prefix and the
  `My Games/FalloutNV` tree exists; INI files (`Fallout.ini`,
  `FalloutPrefs.ini`, `FalloutCustom.ini`) are generated on first launch.
- **Script extenders are DLL injection.** NVSE relies on Proton honouring the
  loader; verify the override and that the extender's own files sit in the game
  root, not in `Data/`.
- **The numeric Nexus id is unset**, so use direct `nexusmods.com/newvegas/...`
  URLs rather than the in-app browse picker.

## Worked example

### Home-Manager profile

```nix
programs.modde = {
  enable = true;

  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.viva-new-vegas = {
    game = "fallout-new-vegas";
    installMode = "auto";
    gameDir = "/home/me/.local/share/Steam/steamapps/common/Fallout New Vegas";

    wabbajackList = {
      url = "https://example.com/viva-new-vegas.wabbajack";
      hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    };

    tools = {
      mangohud.enable = true;
      gamemode.enable = true;
      proton = {
        enable = true;
        # Honour a script-extender / injector DLL inside the prefix.
        settings.dllOverrides = "d3d9=n,b";
      };
    };
  };
};
```

If New Vegas is not installed yet, omit `gameDir` or set
`installMode = "await-game"`; activation prints the next step and continues
without failing, then deploys once you set `gameDir` and rebuild. See the
[Home-Manager Module](../configuration/hm-module.md) reference for every option.

### CLI

```bash
# Discover what is already in Data/ and import it into a profile
modde scan --game fallout-new-vegas --import-to viva-new-vegas

# Match an existing install against a Wabbajack New Vegas list
modde scan --game fallout-new-vegas \
  --manifest /path/to/viva-new-vegas.wabbajack \
  --import-to viva-new-vegas \
  --prune-duplicates

# Deploy the symlink farm into <install>/Data
modde deploy --profile viva-new-vegas --game fallout-new-vegas

# Capture saves from the Proton prefix into the profile
modde save capture --game fallout-new-vegas --profile viva-new-vegas

# Deploy then launch through Proton
modde play --game fallout-new-vegas
```

Wabbajack manifests that name the game **`FalloutNewVegas`** or **`FalloutNV`**
both map to the `fallout-new-vegas` id, so either spelling in a modlist resolves
to this title. See [Wabbajack](../guides/wabbajack.md) for the full modlist
workflow.

## See also

- [Supported Games](supported-games.md) — status table and Wabbajack mapping
- [Oblivion](oblivion.md) — the other Gamebryo title sharing this plugin
- [Deployment & VFS](../guides/deployment.md)
- [Mod Scanning](../guides/scanning.md)
- [Conflicts](../guides/conflicts.md)
- [Save Management](../guides/saves.md)
- [FOMOD Installers](../guides/fomod.md)
- [Tools & Overlays](../guides/tools.md)
- [Wabbajack](../guides/wabbajack.md)
- [Home-Manager Module](../configuration/hm-module.md)
- [Parity & capability matrix](../reference/parity.md)
