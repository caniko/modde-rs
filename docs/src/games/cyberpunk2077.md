# Cyberpunk 2077

Cyberpunk 2077 is one of the few non-Bethesda titles that modde supports **end to
end**. Its overall status is `Done` in [`docs/capability-matrix.toml`][matrix] and on
the [supported games](supported-games.md) page: the scanner, conflict classifier,
and save tracker are all wired up, and deployment runs a real `REDmod` deploy pass
after staging.

The game id is **`cyberpunk2077`**. Use it verbatim with every CLI command and in
home-manager profiles.

[matrix]: https://github.com/caniko/rs-modde/blob/trunk/docs/capability-matrix.toml

## Engine and status

| Property | Value |
| -------- | ----- |
| Engine family | REDengine 4 (`EngineFamily::CyberpunkRedEngine`) |
| Game id | `cyberpunk2077` |
| Display name | Cyberpunk 2077 |
| Overall status | `Done` |
| Scanner | Yes |
| Conflict detection | Yes |
| Save tracking | `Done` |
| Steam App ID | `1091500` |
| Heroic (GOG) app id | `1423049311` |
| Heroic (Epic) app id | `Ginger` |
| Nexus domain | `cyberpunk2077` |
| Nexus numeric game id | `3333` |
| Wabbajack name | `Cyberpunk2077` |

The numeric Nexus id is present, so Cyberpunk 2077 appears in the UI's *Browse
Nexus* picker (which needs the numeric id), and the Nexus side panel can fetch
mod metadata via REST v1 and GraphQL v2. The mod information dialog is still
`Partial`: only the Nexus-metadata side panel exists — there is no MO2-style
file-tree / INI / conflict dialog yet.

## Install detection

modde locates the install through its launcher ids. For Steam it uses App ID
`1091500` and the `steamapps/common/Cyberpunk 2077` directory; for Heroic it
matches the GOG app id `1423049311` or the Epic app id `Ginger`. The same ids
drive Proton-prefix discovery for save tracking.

Because Cyberpunk runs through Proton/Wine on Linux, the game writes its saves
inside a Wine prefix rather than a native Linux path. modde checks two prefixes,
in order:

1. **Heroic** (GOG / sideload):
   `~/Games/Heroic/Prefixes/default/Cyberpunk 2077/pfx/drive_c/users/steamuser/Saved Games/CD Projekt Red/Cyberpunk 2077`
2. **Steam Proton**: the Steam compat prefix under
   `steamapps/compatdata/1091500/pfx/drive_c/users/steamuser/Saved Games/CD Projekt Red/Cyberpunk 2077`

The first prefix that exists on disk wins. If neither is present (for example,
the game has never been launched under that launcher, so the prefix has not been
created), save tracking reports no save directory.

## Mod directory and deploy strategy

The canonical mod directory is `<install>/mods/` — the REDmod loader's directory.
Deployment is a **two-step** process:

1. **Per-mod symlink.** For every entry in the profile's staging directory, modde
   symlinks the mod directory into `<install>/mods/<name>/`. If a target name
   already exists (a real directory, a file, or a stale symlink), it is removed
   first so the new symlink is clean. This keeps the game directory pointing at
   modde-managed staging rather than copying bytes around.
2. **Post-deploy REDmod deploy hook.** After symlinking, modde runs a REDmod
   deploy pass (see below). This is what turns the symlinked REDmod packages into
   the game's loadable `mod` archives.

Symlink deployment means an uninstall or redeploy is cheap and non-destructive to
your staged sources.

### The REDmod deploy hook

modde looks for the `redmod` binary in two places:

- `<install>/tools/redmod/bin/redmod` (or `redmod.exe` on Windows) — the copy
  CD Projekt Red ships alongside the game, and
- anywhere on `PATH`.

If a `redmod` binary is found and `<install>/mods/` contains at least one
subdirectory, modde invokes:

```bash
redmod deploy -mod <mods/dir-1> -mod <mods/dir-2> ...
```

with the game directory as the working directory, passing one `-mod` flag per mod
subdirectory. If the binary is **not** found, modde logs a warning and skips the
step — it does not fail the deploy. That means loose-file content (redscript,
TweakXL, CET, `.archive` files) is still deployed by the symlink pass; only the
REDmod-packaged content needs the deploy hook to be registered. A non-zero exit
from `redmod deploy` *does* surface as a deploy error, with the tool's stderr
attached.

> **Gotcha:** REDmod is an optional component in the GOG/Steam installers. If you
> intend to use REDmod packages, install the *REDmod* DLC/tool so the
> `tools/redmod/bin/redmod` binary exists, or put a `redmod` binary on `PATH`.

### Bare-layout recognition

When you install an archive that is **not** a REDmod package, modde recognizes a
"bare" Cyberpunk layout if the extraction root contains any of these top-level
directories: `r6`, `archive`, `archives`, `bin`, `engine`, `mods`, `red4ext`.
A bare extract is treated as a single mod and symlinked into `<install>/mods/<name>/`,
preserving the loose-file tree the mod author shipped.

## What scanning finds

`modde scan --game cyberpunk2077` walks five known mod roots under the install
directory and reports a confidence score per discovered mod:

| Scan root | Mod kind | id prefix | Confidence |
| --------- | -------- | --------- | ---------- |
| `bin/x64/plugins/cyber_engine_tweaks/mods/<name>/` | Cyber Engine Tweaks (CET) | `cet/` | 0.70, or 0.95 if `init.lua` present |
| `r6/scripts/<name>/` | redscript | `reds/` | 0.90 |
| `r6/tweaks/<name>/` | TweakXL | `tweak/` | 0.90 |
| `archive/pc/mod/<file>.archive` | loose `.archive` | `archive/` | 0.85 |
| `mods/<name>/` | REDmod package | `redmod/` | 0.95 with `info.json`, else 0.80 |

For REDmod packages, the scanner reads the `info.json` manifest and pulls the
mod's `name` and `version` from it (falling back to the directory name when the
manifest is missing or unparseable). The manifest fields modde parses are `name`,
`version`, `custom_sounds`, and `scripts`.

Each discovered mod's id footprint is stable and lowercased, so modde can detect
stale duplicates across redeploys — for example a CET mod maps to the directory
footprint `bin/x64/plugins/cyber_engine_tweaks/mods/<name>/` and a loose archive
maps to the file footprint `archive/pc/mod/<stem>.archive`.

## Conflict classification

modde classifies conflicts at the **loose-file level**. Cyberpunk's `.archive`
container format is proprietary and not yet reverse-engineered in this codebase,
so the classifier does not index *inside* `.archive` files — two mods that both
ship `archive/pc/mod/foo.archive` collide on the file itself, but modde cannot
peek at the resources packed within.

Conflicts are graded by extension into three severities:

| Severity | Extensions |
| -------- | ---------- |
| **Dangerous** | `reds`, `lua`, `tweak`, `xl`, `yaml`, `yml`, `dll` |
| **Config** | `ini`, `cfg`, `json`, `toml` |
| **Cosmetic** | `archive`, `png`, `jpg`, `dds`, `tga` |

A *Dangerous* collision means two mods are fighting over executable game logic
(scripts, tweaks, or a native DLL) and the result is likely a crash or broken
behavior. *Config* collisions usually need a manual merge. *Cosmetic* collisions
are last-writer-wins texture/material overrides and are generally safe.

See the [conflicts guide](../guides/conflicts.md) for how modde surfaces and
resolves these.

## Save tracking

Save tracking is `Done`. Once the save directory is resolved (see
[Install detection](#install-detection)), modde treats each top-level save folder
as a save and categorizes it by directory-name prefix:

| Prefix | Category |
| ------ | -------- |
| `ManualSave-` | manual |
| `AutoSave-` | auto |
| `QuickSave-` | quick |
| `PointOfNoReturn-` | point-of-no-return |

Anything that does not match a known prefix is bucketed as **manual** (the default
category). The scan is **non-recursive** (each save is one directory), and the
`user.gls` settings file is explicitly excluded so it is never mistaken for a save.

**What is fingerprinted.** For each save, modde tries to extract a human-readable
label. When a save uses CDPR's *NamedSaves*, the directory contains a
`metadata.9.json`; modde reads it and uses the `customName` (or `name`) field as
the label. If that file is missing or has no usable name, it falls back to the
save's directory name. The capture summary is reported **by category**, so a
snapshot tells you how many manual / auto / quick / point-of-no-return saves were
captured.

Cyberpunk 2077 also declares `supports_save_profiles = true`, so its saves
participate in [save profiles](../guides/saves.md).

## Plugin and load-order handling

Cyberpunk 2077 has **no ESP/ESM plugin list** or `loadorder.txt`-style ordering
the way Bethesda titles do. There is therefore no plugin enable/disable or
load-order panel for this title. Ordering is whatever the underlying frameworks
impose:

- **REDmod** order is established by the `redmod deploy` pass over the mods in
  `<install>/mods/`.
- **redscript**, **TweakXL**, and **CET** each resolve their own loading; modde's
  job is to deploy the files into the correct roots, not to sequence them.

If two logic mods genuinely conflict, modde flags it as a *Dangerous* collision
(see above) rather than trying to reorder them.

## Installer specifics

modde inspects an extracted archive and picks an install method:

- **REDmod packages** are detected by a top-level `info.json` *plus* an
  `archives/` (or `archive/`) subdirectory. modde records the install as a REDmod
  install whose manifest is `info.json`, and the post-deploy hook later registers
  it with `redmod deploy`.
- **Bare Cyberpunk extracts** (loose redscript / TweakXL / CET / `.archive`
  trees) are recognized by the top-level directory names listed under
  [Bare-layout recognition](#bare-layout-recognition) and symlinked wholesale
  into `mods/`.
- **FOMOD installers** are handled by modde's general FOMOD engine when a mod
  ships an XML installer; see the [FOMOD guide](../guides/fomod.md). This is the
  same engine used across all supported titles.

There is no SMAPI or `.pak`/`.ucas`/`.utoc` handling for Cyberpunk — those belong
to Stardew Valley and the Unreal titles respectively.

## Gaming tools that matter

modde's tool integrations (`modde tool ...`) apply to Cyberpunk just as they do to
other titles — MangoHud, vkBasalt, GameMode, ReShade, OptiScaler, and per-game
Proton settings. See the [Tools & Overlays guide](../guides/tools.md) for the
full workflow.

Cyberpunk does **not** ship a built-in OptiScaler profile in modde's registry
(its `optiscaler_profiles` list is empty, unlike Stellar Blade). You can still
enable and apply OptiScaler manually with `modde tool enable optiscaler` /
`modde tool apply optiscaler`, but you choose the proxy DLL and release yourself —
modde does not auto-select a known-good proxy for this title. `dxgi.dll` is the
proxy most OptiScaler/upscaling setups use on Cyberpunk.

### Wine DLL overrides

Many Cyberpunk frameworks ship as **proxy DLLs** that hijack a Windows system DLL
name in the executable directory `bin/x64`. Under Wine/Proton these need a
`native,builtin` override so Wine loads the mod's DLL instead of its own built-in
stub. modde scans `bin/x64` and emits overrides for any of these it finds:

| Proxy DLL | Typically used by |
| --------- | ----------------- |
| `version` | CET (Cyber Engine Tweaks), ASI loaders |
| `winmm` | ASI loaders, some mod frameworks |
| `dinput8` | various mod frameworks |
| `d3d11` | ReShade, ENB |
| `dxgi` | OptiScaler, ReShade (often handled by fgmod) |
| `winhttp` | some mod loaders |
| `xinput1_3` | controller-hook mods |

modde can compute these overrides both from the live game directory and from the
staging directory before deploy (it searches nested `mods/.../bin/x64` layouts).
The detected overrides are surfaced through the Proton tool integration so they
end up in your launch environment automatically — no manual `WINEDLLOVERRIDES`
editing required for the proxies modde knows about. If you use a proxy DLL not in
the list above, set it explicitly, e.g.:

```bash
modde tool configure proton --game cyberpunk2077 \
  dll_override_mode=forced forced_dll_overrides=dxgi,version
```

## Linux / Proton notes and known gotchas

These notes cover the game's runtime on Linux and macOS, where Cyberpunk runs
through Proton/Wine. On Windows the game runs natively — there is no Wine prefix,
and saves and proxy DLLs live at their native Windows locations. modde itself runs
natively on all three platforms.

- **Launch the game once first.** Saves live inside the Proton/Heroic prefix.
  Until the game has been run under your launcher, the prefix (and the save
  directory) does not exist, and save tracking has nothing to find.
- **Install REDmod if you use REDmod packages.** Without the
  `tools/redmod/bin/redmod` binary (or a `redmod` on `PATH`), the post-deploy hook
  is skipped with a warning. Loose-file mods still deploy.
- **Proxy DLLs need overrides.** CET (`version.dll`), ReShade (`d3d11`/`dxgi`),
  OptiScaler (`dxgi`), and friends will silently do nothing under Wine without the
  `native,builtin` override. modde detects the common ones in `bin/x64`; verify
  with `modde tool status --game cyberpunk2077` and the [troubleshooting
  guide](../guides/troubleshooting.md) if a framework does not load.
- **`.archive` internals are opaque.** modde detects archive-vs-archive collisions
  by filename but cannot diff their contents, so two large `.archive` mods that
  touch overlapping resources may both report clean while still visually
  conflicting in-game.
- **Game runtime differs by platform.** The Proton/Heroic prefix paths above are
  how Cyberpunk runs on Linux and macOS, where it runs through Proton/Wine. On
  Windows the game runs natively, so there is no Wine prefix — saves and proxy
  DLLs live at their native Windows locations. modde itself runs natively on all
  three platforms; see [installation](../getting-started/installation.md).

## Worked example

### Home-Manager profile

```nix
programs.modde = {
  enable = true;

  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles = {
    cyberpunk-mods = {
      game = "cyberpunk2077";
      installMode = "auto";
      gameDir = "/home/me/.local/share/Steam/steamapps/common/Cyberpunk 2077";

      nexusCollection = {
        slug = "my-cyberpunk-collection";
        version = "2.1.0";
      };

      tools = {
        gamemode.enable = true;
        proton = {
          enable = true;
          settings = {
            # CET ships as version.dll; force the override so Wine loads it.
            dllOverrideMode = "forced";
            forcedDllOverrides = "version,dxgi";
          };
        };
      };
    };
  };
};
```

See the [home-manager module reference](../configuration/hm-module.md) for every
profile and tool option.

### CLI

```bash
# Discover the install and inspect what is already there
modde scan --game cyberpunk2077

# Install a mod from a Nexus URL (REDmod, CET, redscript, TweakXL, or .archive)
modde install --game cyberpunk2077 https://www.nexusmods.com/cyberpunk2077/mods/107

# Deploy the active profile: symlinks into mods/, then runs `redmod deploy`
modde deploy --game cyberpunk2077

# Check for conflicts before playing
modde conflicts --game cyberpunk2077

# Capture and list saves from the resolved Proton/Heroic prefix
modde saves list --game cyberpunk2077

# Force a Wine DLL override if a proxy framework is not loading
modde tool configure proton --game cyberpunk2077 \
  dll_override_mode=forced forced_dll_overrides=version,dxgi
```

## See also

- [Supported games](supported-games.md)
- [The Witcher 3](witcher3.md) — the other REDengine title
- [Deployment](../guides/deployment.md)
- [Scanning](../guides/scanning.md)
- [Conflicts](../guides/conflicts.md)
- [Saves](../guides/saves.md)
- [FOMOD installers](../guides/fomod.md)
- [Tools & Overlays](../guides/tools.md)
- [Nexus integration](../guides/nexus.md)
- [Home-Manager module](../configuration/hm-module.md)
- [Parity reference](../reference/parity.md)
