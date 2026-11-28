# Stellar Blade

Stellar Blade (`game_id` **`stellar-blade`**, Steam App ID `3489700`) is one of
modde's two data-driven Unreal Engine titles, sharing the `Ue4Game` plugin with
Subnautica 2. Its overall status is **`Partial`**: scanning, conflict
classification, save tracking, deployment, OptiScaler integration, and Wine
DLL-override detection ship and are user-accessible. Use modde for pak-mod
management; it does not provide a fully guided install, and there is no
game-specific installer wizard or curated load order beyond UE4's `~mods` mount
order. The UI's "Browse Nexus" picker omits the title (see the
[Nexus Mods guide](../guides/nexus.md)).

For the authoritative status row see
[Supported games](supported-games.md); the capability vocabulary (`Done` /
`Partial` / `Not shipped`) is defined there and in `docs/capability-matrix.toml`.

## Engine and project layout

| Property | Value |
| -------- | ----- |
| Engine family | `Unreal4` (the shared UE4/UE5 pak plugin) |
| Steam App ID | `3489700` |
| Steam library folder | `Stellar Blade` |
| UE project short name | `SB` |
| Nexus domain | `stellarblade` |
| Wabbajack name | `StellarBlade` |
| Save profiles | Enabled |
| OptiScaler profiles | `community-dxgi` |

Unreal titles use similar project layouts. Stellar Blade's `SB/` folder holds
`SB/Content/Paks/` for pak archives and `SB/Binaries/Win64/` for the shipping
executable and proxy DLLs. For Stellar Blade, the plugin resolves paths under
`<install>/SB/...` from the project short name.

## How modde detects the install

modde auto-detects only Steam installs:

- **Steam** — App ID `3489700`, library subfolder `Stellar Blade`. modde walks your
  Steam library folders (including extra libraries from `libraryfolders.vdf`) to find
  the install root.
- **Heroic (GOG/Epic)** — not configured for this title; there is no Heroic GOG or
  Epic app ID in the registry, so Heroic auto-detection does not apply.

You can always bypass detection by passing `--game-dir <path>` to commands that take
it (for example `modde scan` and `modde install wabbajack`), or by setting `gameDir`
in a home-manager profile.

### Proton prefix

On Linux and macOS Stellar Blade runs through Proton/Wine, so both save tracking
and the UE `Saved/Config` deploy target read from the compatibility prefix that
Steam creates at (on Windows these are native paths, no prefix involved):

```
<steam_root>/steamapps/compatdata/3489700/pfx/
```

modde derives this from the Steam `common` directory's parent, then `compatdata/3489700/pfx`.
On Proton, if the prefix does not exist yet, the save directory and the
`ue4-saved-config` deploy target both resolve to `None`. **Launch the game
once** to create it, then retry.

## Mod directory and deploy strategy

The deploy target is the standard UE pak drop folder:

```
<install>/SB/Content/Paks/~mods
```

UE orders pak mount points lexically. Keep the leading tilde: `~mods` sorts
after the shipping paks, so its mods override base content. modde creates `~mods`
and deploys directly to `Content/Paks/~mods` on disk, without a custom VFS for
UE titles.

A mod archive is recognised as a single-file-set install when its extracted root
contains at least one `.pak`, `.ucas`, or `.utoc` file (`InstallMethod::SingleFileSet`).
That is the common case for Nexus pak mods, which extract a loose `Name.pak`
(optionally with `Name.ucas` / `Name.utoc` siblings) you drop straight into `~mods`.

In addition to `~mods`, the plugin exposes one named deploy target for user config:

| Target ID | Label | Kind | Resolves to |
| --------- | ----- | ---- | ----------- |
| `ue4-saved-config` | UE4 Saved/Config | `UserConfig` | `<prefix>/drive_c/users/steamuser/AppData/Local/SB/Saved/Config/Windows` |

This second target is how `Engine.ini` / `GameUserSettings.ini` style tweaks land in
the right place inside the Proton prefix. It resolves to `None` until the prefix exists.

## What scanning finds

`modde scan --game stellar-blade` runs the UE4 scanner, which walks two
subdirectories of `SB/Content/Paks/`:

| Subdirectory | Source location tag |
| ------------ | ------------------- |
| `~mods` | `paks-mods` |
| `LogicMods` | `logic-mods` |

Within each, files are grouped by **file stem** across the `.pak` / `.ucas` /
`.utoc` extensions. A `Name.pak` plus its `Name.ucas` and `Name.utoc` siblings collapse
into a single discovered mod with `mod_id = "pak/Name"` and a fixed scan confidence of
`0.9`. The scanner reads pak archives by directory and by their grouped stems only — it
does **not** crack open pak contents, parse asset trees, or read mod-author metadata
from inside the archives.

For change-tracking, a discovered mod's footprint is its `.pak` file, recorded as
`sb/content/paks/~mods/<stem>.pak` (lower-cased). The `.ucas`/`.utoc` sidecars collapse
into that same row, which keeps the mod list one-entry-per-mod even though several files
back it.

```bash
# Discover already-installed Stellar Blade paks and import them into a profile
modde scan --game stellar-blade --import-to default
```

## Conflict classification

Conflicts use the shared UE collision policy. Stellar Blade declares `pak`, `ucas`, and
`utoc` as **archive extensions**, so two mods shipping the same-named pak family are a
real overwrite conflict. Severities are assigned per file extension:

| Severity | Extensions |
| -------- | ---------- |
| `Dangerous` | `pak`, `ucas`, `utoc`, `dll`, `lua` |
| `Config` | `ini`, `cfg`, `json`, `toml`, `xml`, `yaml` |
| `Cosmetic` | `dds`, `png`, `jpg`, `tga` |

The same extension lists also drive mod *safety* classification (whether a mod is
considered save-affecting versus purely cosmetic): `pak`, `ucas`, `utoc`, `dll`, and
`lua` are treated as save-breaking/logic-altering, while `png`, `jpg`, `dds`, `tga`,
and `ini` are treated as cosmetic. So a texture-only retexture conflicting with another
texture is flagged `Cosmetic`, while two paks clobbering one another are `Dangerous`.

See [Conflicts & load order](../guides/conflicts.md) for how modde presents and resolves
these.

## Save tracking

Save tracking is **enabled** for Stellar Blade (`with_save_profiles(true)`), and it is
wired into modde's per-profile save layer.

**Location.** Saves are read from the Proton prefix:

```
<steam_root>/steamapps/compatdata/3489700/pfx/drive_c/users/steamuser/AppData/Local/SB/Saved/SaveGames
```

modde returns this directory only if it already exists on disk.

**What is fingerprinted.** The tracker recursively captures every **`.sav`**
under the save directory as `manual`, with no prefix rules or exclusions. It
uses the file stem as the label, falling back to the relative path, and
summarises captures by category. modde tracks the `.sav` files without parsing
their binary format or extracting playtime, character, or chapter metadata.

See [Save management](../guides/saves.md) for how captures attach to profiles.

## Plugin and load-order handling

Stellar Blade has **no plugin file (`.esp`/`.esm`) load order** — it is not a Bethesda
title. Ordering is purely the UE pak mount convention: paks in `~mods` mount after base
game paks, and within `~mods` UE orders by name. If two mods need a deterministic order
relative to each other, the lever is the **pak file name** (the conventional `zzz_`-style
prefixes that sort late). modde does not synthesize or rewrite a load-order file for this
game; there is no curated load order beyond what the file names express.

A separate `LogicMods` folder is also scanned (tagged `logic-mods`) for setups that keep
Blueprint/logic paks distinct from content paks, but modde treats both folders uniformly
as grouped pak mods.

## Installer specifics

- **Pak / ucas / utoc** — the native format. Archives whose root contains a pak-family
  file install as a single file set straight into `~mods`. This is the path most Nexus
  Stellar Blade mods take.
- **FOMOD** — modde's [FOMOD installer](../guides/fomod.md) is engine-agnostic, so a mod
  that ships a `ModuleConfig.xml` can be driven through `modde fomod inspect` /
  `modde fomod apply` (or a declarative config). FOMOD is not Stellar Blade-specific and
  is uncommon for this title, but it is available when an author uses it.
- **REDmod / SMAPI / BSA** — not applicable. Those are Cyberpunk, Stardew, and Bethesda
  installer paths respectively; Stellar Blade uses none of them.

There is no bespoke Stellar Blade installer wizard — this is part of why the title is
`Partial`. Most installs are "extract pak, deploy to `~mods`".

## Gaming tools that matter

### OptiScaler — the `community-dxgi` profile

Stellar Blade ships one curated OptiScaler game profile, **`community-dxgi`**.
GPU-specific FSR4 choices are handled by OptiScaler's global `hardware_tuning`
layer, not by separate per-game GPU profiles.

| Field | Value |
| ----- | ----- |
| Proxy DLL | `dxgi.dll` |
| Source mode | Any OptiScaler source with `FSR4_INT8/` and `FSR4_LATEST/` payloads; GOverlay builds are recommended for FSR4 variant switching |
| Tested OptiScaler version | `0.9` |
| FSR4 variant | selected by `hardware_tuning=auto` |
| `emulate_fp8` | selected by `hardware_tuning=auto` |
| OptiPatcher | enabled |
| `spoof_dlss` | `false` |
| Companion files | copied |

The profile uses **OptiPatcher to unlock DLSS/DLSS-FG inputs without
spoofing** a DLSS-capable GPU. The unlocked code paths run through
OptiScaler/FSR instead of faking vendor detection.

### RDNA3 requirements

FSR4 on AMD RX 7000 series requires:

- **Proton 11+** or **Proton GE 10-34+** (for FSR 4.1 / FFX SDK 2.2 support)
- **Mesa 25.2+** (for FSR4 frame generation)
- **`PROTON_FSR4_UPGRADE=1`** environment variable (auto-emitted by modde's env vars)
- **Proton prefix set to Windows 11** (via `winecfg`)
- For RDNA3 on Linux: the `DXIL_SPIRV_CONFIG=wmma_rdna3_workaround` env var is no longer
  needed as of FSR 4.1.1+.

### GPU auto-detection

modde reads `/sys/class/drm/` to detect your GPU at apply time and finalizes
FSR4 settings when `hardware_tuning=auto`:

- **RDNA3 (RX 7000)** → `fsr4_variant=int8_402`, `emulate_fp8=false`
- **RDNA4 (RX 9000)** → `fsr4_variant=latest_fp8`, `emulate_fp8=false`
- **AMD legacy, NVIDIA, Intel, unknown** → preserve the configured/default FSR4 setting

You can override the GPU tuning only by switching to manual mode:

```bash
modde tool configure optiscaler --game stellar-blade hardware_tuning=manual
modde tool configure optiscaler --game stellar-blade fsr4_variant=int8_402
```

### Known gotchas

- The game **may crash on first boot** with the proxy in place but work on the next launch
  — if the first launch dies, just relaunch before assuming a bad install.
- If DLSSG/frame-gen HUD elements look wrong (interpolation artifacts on the HUD),
  **set the in-game sharpness slider to 0** as a workaround.

### Quick start

```bash
# Inspect the curated profile, cached sources, and effective settings.
modde tool profiles optiscaler --game stellar-blade
modde tool sources optiscaler --game stellar-blade
modde tool settings optiscaler --game stellar-blade

# Configure and apply OptiScaler. Does not upgrade/download unless --upgrade is passed.
modde tool setup optiscaler --game stellar-blade --source auto --apply

# If something looks wrong, print actionable findings and next commands.
modde tool doctor --game stellar-blade optiscaler
```

`modde tool setup ... --source auto` reuses the current source or an already cached
GOverlay build with `FSR4_INT8/` and `FSR4_LATEST/` payloads. It does not select a
newer build by default; pass `--upgrade` when you intentionally want the newest matching
GOverlay release. `setup --apply` writes the `dxgi.dll` proxy and companion files into
the game's `Binaries/Win64`. See [Tools & overlays](../guides/tools.md) for diagnostics,
preview, and exact release pinning commands.

### Proton, overlays, and Wine DLL overrides

The other tool IDs apply normally for this Proton game: `proton` (version mode, extra env,
DLL-override mode), `mangohud`, `vkbasalt`, `gamemode`, and `reshade`. Because Stellar
Blade runs through Wine/Proton, any native proxy DLLs you stage must be loaded *native
first*. modde scans `SB/Binaries/Win64` (and mod staging) for the common UE proxy DLL
names and surfaces them as `WINEDLLOVERRIDES`:

```
dwmapi, xinput1_3, d3d11, dxgi, version, winmm, dinput8
```

So a UE4SS install (`dwmapi`), a ReShade/DLSS swapper (`dxgi`/`d3d11`), or a generic ASI
loader (`version`/`winmm`) is detected and overridden automatically — the OptiScaler
profile's own `dxgi.dll` is exactly one of these.

## Linux / Proton notes and gotchas

Stellar Blade runs through Proton/Wine on Linux and macOS. On Windows it runs
natively, without a Wine prefix or `WINEDLLOVERRIDES`, and keeps saves and config
at their native Windows locations. modde runs natively on all three platforms.

- **Launch the game once before first deploy.** The save directory and the
  `ue4-saved-config` target both live inside the Proton prefix and resolve to `None`
  until Proton has created `compatdata/3489700/pfx`.
- **OptiScaler first-boot crash.** Expected with this profile; relaunch. (See the
  OptiScaler section above.)
- **HUD interpolation under DLSSG.** Set the in-game sharpness slider to 0.
- **DLL override ordering.** If a proxy DLL mod is staged but not taking effect, confirm
  the matching `WINEDLLOVERRIDES` entry is present — modde derives it from the proxy name
  in `Binaries/Win64`, so the file must be the recognised name.
- **Pak mount order is name-driven.** There is no load-order editor; rename paks (e.g.
  a late-sorting prefix) when one mod must win over another.

## Worked example

### CLI

```bash
# 1. Discover the install (Steam App ID 3489700 / "Stellar Blade") and scan it,
#    importing what is already in ~mods into the default profile.
modde scan --game stellar-blade --import-to default

# 2. Install a pak mod from a Nexus URL into a profile.
modde install mod https://www.nexusmods.com/stellarblade/mods/<id> --profile default

# 3. Deploy the active profile into SB/Content/Paks/~mods.
modde deploy --game stellar-blade --profile default

# 4. Add upscaling/frame-gen via the curated OptiScaler profile.
modde tool enable optiscaler --game stellar-blade
modde tool configure optiscaler --game stellar-blade optiscaler_profile=community-dxgi
modde tool apply optiscaler --game stellar-blade

# 5. (Optional) Register a named external tool/executable for this game so it runs
#    with overwrite capture and the right Wine DLL overrides.
modde tool add-executable --game stellar-blade --name my-tool /path/to/tool.exe
modde exec run my-tool --game stellar-blade
```

### Home-Manager profile

The home-manager module exposes Stellar Blade as a declarative profile. The
`tools.optiscaler.profile` option is type-checked against the registered profile list,
so `community-dxgi` is the valid value for this game.

```nix
{
  programs.modde = {
    enable = true;
    profiles.stellar-blade-main = {
      game = "stellar-blade";

      # Auto-detected from Steam; set explicitly if your library is non-standard.
      # gameDir = "/path/to/SteamLibrary/steamapps/common/Stellar Blade";

      installMode = "auto";

      tools.optiscaler = {
        enable = true;
        profile = "community-dxgi";
      };
    };
  };
}
```

On activation modde enables, configures, and applies the OptiScaler profile for the
`stellar-blade` game, writing the `dxgi.dll` proxy into `SB/Binaries/Win64`. Remember to
launch the game once first so the Proton prefix exists.

## See also

- [Supported games](supported-games.md)
- [Subnautica 2](subnautica2.md) — the other data-driven UE title
- [Oblivion Remastered](oblivion-remastered.md) — another UE pak-based game
- [Mod scanning](../guides/scanning.md)
- [Conflicts & load order](../guides/conflicts.md)
- [Deployment & VFS](../guides/deployment.md)
- [Save management](../guides/saves.md)
- [Tools & overlays](../guides/tools.md)
- [FOMOD installer](../guides/fomod.md)
- [MO2 parity & capability audit](../reference/parity.md)
