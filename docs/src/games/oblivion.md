# The Elder Scrolls IV: Oblivion

The Elder Scrolls IV: Oblivion (`game_id` **`oblivion`**) is one of modde's two
Gamebryo titles. It shares mod-directory rules, conflict policy, and `plugins.txt`
handling with Fallout: New Vegas through the data-driven `GamebryoGame` plugin
in `crates/modde-games/src/gamebryo/mod.rs`.

## Status at a glance

| Aspect | State | Notes |
| ------ | ----- | ----- |
| Overall | `Partial` | Deployment, scanning, conflicts, and save tracking work; there is no bespoke INI-merge or BSA-aware extraction UX. |
| Scanner | Yes | Reads `Data/` for `.esp`/`.esm` plugins and matching `.bsa` archives. |
| Conflict detection | Yes | Gamebryo collision policy with a `bsa`-aware archive comparison. |
| Save tracking | `Done` | `.ess`/`.fos` saves under the Proton prefix, git-backed vault, fingerprinting. |
| Plugin system | Yes | `plugins.txt`-style load-order read/write. |

The [supported games table](supported-games.md) is the canonical, test-coupled
source for this `Partial` rating.

## Engine and registration

Oblivion is registered in `crates/modde-games/src/registry.rs` with the
`EngineFamily::Gamebryo` engine and the following identifiers:

| Field | Value |
| ----- | ----- |
| `game_id` | `oblivion` |
| Display name | `The Elder Scrolls IV: Oblivion` |
| Steam App ID | `22330` |
| Steam install dir | `Oblivion` |
| Nexus domain | `oblivion` |
| Wabbajack name | `Oblivion` |
| `my_games_dir` | `Oblivion` |
| INI file | `Oblivion.ini` |
| Archive extension | `bsa` |

The plugin reports `has_plugin_system() == true` and
`supports_save_profiles() == true`, and exposes the Nexus domain `oblivion` so a
`nexusmods.com/oblivion/mods/<id>` URL resolves straight to this game.

> **Not to be confused with** the 2025 Unreal Engine remaster. That is a
> separate registration, [Oblivion Remastered](oblivion-remastered.md)
> (`game_id` `oblivion-remastered`, Steam App ID `2623190`), which uses the
> Unreal `pak`/`ucas`/`utoc` pipeline — *not* the Gamebryo path described here.

## Install detection

modde finds launcher-managed installs; it does not install the base game.
Oblivion registers only Steam identifiers (`steam_app_id = "22330"`,
`steam_dir = "Oblivion"`); its Heroic GOG/Epic app IDs are `None`.
`launcher_games()` matches the Steam App ID, and the standard Steam library scan
finds `.../steamapps/common/Oblivion/`.

If you install through a non-default path (or via Heroic/GOG), set the game
directory explicitly — `gameDir` in the home-manager module, or `--game-dir` on
the CLI — and modde will use that instead of probing Steam.

## Mod directory and deploy strategy

The Gamebryo plugin places mods under the game's `Data/` folder:

```text
<install>/Data/
```

`mod_directory(install)` returns `install.join("Data")`. With the standard
symlink-farm VFS, modde stages mods, picks the winner at each relative path by
load order, and symlinks the staging tree into `Data/`. See
[Deployment & VFS](../guides/deployment.md) for the build → materialize →
deploy pipeline and rollback.

### Archive recognition on install

When you install a downloaded archive, the plugin's `analyze_mod_archive`
detects the common "the mod ships its own `Data/` folder" layout and strips it,
mapping the archive's `Data/` contents to the game's `Data/`
(`InstallMethod::StripContentRoot { root: "Data" }`). Archives that instead ship
a *bare* Gamebryo layout — loose `meshes/`, `textures/`, `sound/`, `music/`,
`menus/`, `scripts/`, `shaders/` folders, or top-level `.esp`/`.esm`/`.bsa`
files — are recognized by `recognizes_bare_layout` (case-insensitively) and
deployed directly into `Data/` without a wrapper directory. See the
[FOMOD guide](../guides/fomod.md) for installers that present optional
sub-packages.

## What scanning finds

`modde scan --game oblivion` runs the Gamebryo scanner (`OBLIVION_SCANNER`),
which looks for plugins only in `Data/`:

- It checks only top-level files in `Data/` for `.esp` or `.esm` extensions
  (case-insensitive).
- It groups each plugin with a sibling **`.bsa`** of the same stem, if present
  (e.g. `MyMod.esp` + `MyMod.bsa`), as one mod.
- Each discovered mod is reported with `mod_id = "plugin/<filename>"`, the
  plugin filename as the display name, its `Data/`-relative file list with
  sizes, and a detection confidence of `0.85`.

The scanner's footprint for matching against a Wabbajack manifest is the
lowercased plugin filename, so manifest archives that carry a known `.esp`/`.esm`
line up with on-disk plugins. See [Mod Scanning](../guides/scanning.md) for
`--import-to`, `--manifest`, and threshold options.

> The scanner intentionally does not walk loose-asset trees or open BSA
> contents — it discovers *plugins* and their paired archives. Loose textures
> and meshes are still resolved and conflict-checked at deploy time; they are
> just not enumerated as standalone "mods" by `scan`.

## Conflict classification

Oblivion uses the shared Gamebryo collision classifier with a `bsa`-aware
archive comparison (`gamebryo_collision_classifier`, built over
`BSA_ARCHIVE_EXTENSIONS = ["bsa"]`). When two mods provide the same relative
path, the later mod in the load order wins, and the per-extension **severity**
comes from the default policy:

| Severity | Extensions |
| -------- | ---------- |
| Dangerous | `esp`, `esm`, `dll`, `lua`, `ws` |
| Config | `ini`, `json`, `xml` |
| Cosmetic | `dds`, `png`, `jpg`, `tga`, `nif` |

Run `modde collisions --profile <name>` for the critical/major report, or add
`--all` to include cosmetic overlaps. See
[Conflicts & Load Order](../guides/conflicts.md) for shadowed-mod and
redundant-file reporting and per-file hiding.

### Content categories and save-breaking classification

Separately from collision severity, the Gamebryo **content policy** classifies
file types so save-tracking knows what is risky. Extensions are mapped to
categories — `esp`/`esm` → Plugin, `dds`/`png`/`tga`/`jpg` → Texture, `nif`/`kf`
→ Mesh, `wav`/`mp3`/`ogg` → Sound, `pex`/`obse`/`nvse` → Script, `bsa` →
Archive, `ini`/`xml` → Config, `dll` → Binary — and the following are treated as
**save-breaking**:

- Extensions: `esp`, `esm`, `pex`, `dll`, `obse`, `nvse`
- Any file under a `scripts/` directory

Cosmetic file types (`nif`, `bsa`, `dds`, `png`, `tga`, `jpg`, `kf`, `wav`,
`mp3`, `ogg`, `ini`, `xml`) are *not* save-breaking. This set is what feeds the
save fingerprint described below.

## Plugin / load-order handling

Gamebryo games carry a real plugin system. modde reads and writes
`plugins.txt`-style load-order files via `read_plugin_order_file` /
`write_plugin_order_file`:

- Reading strips blank lines, `#`/`;` comments, and the leading `*`
  enabled-marker, returning a clean ordered list of plugin names.
- Writing emits one plugin per line, each prefixed with `*` to mark it enabled.

This is the same `plugins.txt` convention Bethesda's Creation-Engine titles use,
so modde's plugin-order backup/restore (`modde backup plugins` /
`backup restore-plugins`) applies to Oblivion as well. Note that Oblivion's
classic engine does **not** use light plugins (`.esl`); load order is `.esm`
masters first, then `.esp` plugins. modde's shared LOOT tooling is built for the
Bethesda masterlists; treat any cross-engine LOOT sorting for Oblivion as
best-effort rather than a guaranteed masterlist — fall back to a curated order
captured with `backup plugins`.

## Save tracking

On Linux and macOS Oblivion runs through Proton/Wine, so `save_directory()`
resolves saves inside the **compatibility prefix** rather than at a native path:

```text
<steam>/steamapps/compatdata/22330/pfx/drive_c/users/steamuser/Documents/My Games/Oblivion/Saves/
```

That path is assembled from the Steam common root, the `compatdata/<app_id>`
prefix, and the game's `my_games_dir` (`Oblivion`). On Windows the game runs
natively, so the same saves live at their native Windows location with no prefix
involved. The Gamebryo save tracker
(`GAMEBRYO_SAVE_TRACKER`) then:

- Matches save files by extension — **`.ess`** (Oblivion saves) and **`.fos`**
  (shared Gamebryo extension) — non-recursively in that directory.
- Excludes `*.bak` backups.
- Categorizes every match as `manual` and labels it by its relative filename.

Saves flow into modde's git-backed vault (one repo per game, one branch per
profile). Each snapshot embeds a **SHA-256 fingerprint** computed over the
sorted list of enabled *save-breaking* mods (the `esp`/`esm`/`pex`/`dll`/`obse`/
`nvse` + `scripts/` set above). On restore, modde compares fingerprints and warns
when the save-breaking mod set has changed since the snapshot was taken. See
[Save Management](../guides/saves.md) for capture, history, restore, and
`save adopt` for existing saves.

## Installer specifics

- **FOMOD** — Oblivion mods are frequently packaged as FOMOD installers. modde's
  FOMOD support (interactive wizard plus declarative TOML/JSON/Nix config) works
  for any game; pre-resolve choices declaratively where you can. See the
  [FOMOD guide](../guides/fomod.md).
- **Bare / `Data`-rooted archives** — handled automatically by
  `analyze_mod_archive` / `recognizes_bare_layout` as described above.
- **OBMM/OMOD** — Oblivion's legacy OMOD format is **not** a modde installer
  type. Extract such mods to a plain `Data/` layout (or convert) before
  installing.
- **Script extenders (OBSE)** — the Oblivion Script Extender is an *external
  runtime*, not a mod modde installs. `.obse`/`.dll` plugins that ride along with
  OBSE are scanned/classified (as Script/Binary, and save-breaking), but you
  launch the game through the script-extender loader yourself — see Proton notes.

There is no REDmod, `.pak`, or SMAPI path here; those belong to other engines
([Cyberpunk 2077](cyberpunk2077.md), [Baldur's Gate 3](baldurs-gate3.md),
[Stardew Valley](stardew-valley.md) respectively).

## Tools that matter for Oblivion

Oblivion has **no OptiScaler profiles** registered (`optiscaler_profiles: &[]`),
which is expected for a 2006 DX9 title — DLSS/FSR upscaling frameworks are not a
fit. The tools that do matter are launch-and-config integrations:

- **Proton** — version selection, Wine prefix, environment, and DLL overrides.
  The script extender needs `obse_loader.exe`/`obse_*.dll` to be honored, which
  usually means a forced DLL override and launching via the loader (see below).
- **MangoHud / vkBasalt / GameMode** — overlay, post-processing, and performance
  tuning; all apply to Oblivion as launch/environment integrations.
- **xEdit / BodySlide-style tools** — run external editors with overwrite
  capture via `modde tool run`, which snapshots `Data/` before and after and
  parks new/changed files into an `__overwrite__` mod.

See [Tools & Overlays](../guides/tools.md) for the full tool surface, and
[Playing with modde](../guides/playing.md) for deploy-then-launch.

## Linux / Proton notes and gotchas

The prefix-specific notes below apply on Linux and macOS, where Oblivion runs
through Proton/Wine. On Windows it runs natively, with no Wine prefix: saves and
INI files use native Windows paths. modde runs natively on all three platforms.

- **Saves live in the Proton prefix**, under `compatdata/22330/...` — not in your
  Linux home directory. If you change Proton/compat tools or wipe the prefix,
  the save path moves with it; re-run `modde save adopt` if needed.
- **Script extender launch** — OBSE must be invoked through its loader inside the
  prefix. Configure Proton DLL overrides for the OBSE DLLs (e.g. via
  `modde tool configure proton --game oblivion dll_override_mode=forced
  forced_dll_overrides=...`) and set Steam's launch command to the loader.
- **`Data/` case-sensitivity** — Windows is case-insensitive; Linux is not. modde
  recognizes bare layouts case-insensitively at install time, but mods that
  reference assets with a different case than the file on disk can still fail to
  load. Keep the on-disk casing matching what plugins expect.
- **INI is not auto-merged** — modde detects `Oblivion.ini` as a config file and
  classifies INI collisions as `Config` severity, but it does not perform an
  MO2-style INI tweak/merge. Edit the INI under the prefix's
  `My Games/Oblivion/` directly when a mod requires it.
- **`Partial` rating** — the core path (install → scan → deploy → conflicts →
  saves) is solid, but there is no Oblivion-specific BSA browser, no INI editor,
  and no engine-validated LOOT masterlist. Plan load order with
  `backup plugins`/`restore-plugins` snapshots.

## Worked example

### CLI

```bash
# 1. Confirm modde resolves the install (Steam App ID 22330)
modde scan --game oblivion --dry-run

# 2. Create a profile and import what is already in Data/
modde scan --game oblivion --import-to my-oblivion

# 3. Install a Nexus mod (oblivion domain resolves automatically)
modde install mod "https://www.nexusmods.com/oblivion/mods/12345" \
  --game oblivion --profile my-oblivion

# 4. Inspect conflicts before playing
modde collisions --profile my-oblivion --all

# 5. Back up the current plugin order, then deploy and play
modde backup plugins --profile my-oblivion --game oblivion
modde play --game oblivion --profile my-oblivion

# 6. Adopt pre-existing saves into the vault (one-time)
modde save adopt --game oblivion --profile my-oblivion
```

### Home-Manager profile

```nix
programs.modde.profiles.my-oblivion = {
  game = "oblivion";
  # Set once Steam has installed the game; until then omit it or use await-game.
  gameDir = "/home/me/.local/share/Steam/steamapps/common/Oblivion";
  installMode = "auto";

  tools = {
    gamemode.enable = true;
    mangohud.enable = true;
    proton = {
      enable = true;
      settings = {
        # Honor OBSE DLLs inside the Proton prefix.
        dll_override_mode = "forced";
        forced_dll_overrides = "dxgi";
      };
    };
  };
};
```

> Declare the profile before the game exists by leaving `gameDir` unset (or
> `installMode = "await-game"`); activation prints the next step and continues.
> modde never installs the base game itself.

## See also

- [Supported games](supported-games.md) — the canonical status table
- [Fallout: New Vegas](fallout-new-vegas.md) — the sibling Gamebryo title
- [Oblivion Remastered](oblivion-remastered.md) — the separate Unreal remaster
- [Mod Scanning](../guides/scanning.md) — `scan`, `--import-to`, manifest matching
- [Conflicts & Load Order](../guides/conflicts.md) — severity, hiding, plugin order
- [Deployment & VFS](../guides/deployment.md) — symlink-farm deploy and rollback
- [Save Management](../guides/saves.md) — vault, fingerprinting, restore
- [FOMOD](../guides/fomod.md) — interactive and declarative installers
- [Tools & Overlays](../guides/tools.md) — Proton, MangoHud, overwrite capture
- [MO2 parity & capability audit](../reference/parity.md) — what is `Done` vs `Partial`
