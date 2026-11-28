# The Witcher 3: Wild Hunt

The Witcher 3: Wild Hunt is modde's second REDengine title, alongside
[Cyberpunk 2077](cyberpunk2077.md). Its built-in plugin uses the `witcher3` game id.

## Engine & overall status

| Property | Value |
| -------- | ----- |
| Engine family | REDengine (`EngineFamily::Witcher`) |
| game id | `witcher3` |
| Display name | The Witcher 3: Wild Hunt |
| Overall status | `Partial` |
| Scanner | Yes |
| Conflict detection | Yes |
| Save tracking | `Done` |

The status is **`Partial`**, matching the [supported games table](supported-games.md).
modde deploys mods, scans the filesystem, classifies `.ws` script conflicts,
tracks saves, and detects Wine/Proton DLL overrides. Unlike some Bethesda titles,
The Witcher 3 has no LOOT-style sorter or plugin validation: REDengine has no
`.esp`/`.esm` plugin model. Support is solid for loose-folder and script mods;
heavily scripted modlists still benefit from manual review.

## Install detection

modde locates the game through its launcher registration:

| Field | Value |
| ----- | ----- |
| Steam App ID | `292030` |
| Steam install dir | `The Witcher 3` |
| Heroic (GOG) | not wired |
| Heroic (Epic) | not wired |
| Nexus domain | `witcher3` |

On a typical Linux Steam install the game directory resolves to something like
`~/.local/share/Steam/steamapps/common/The Witcher 3`. Run `modde detect` to see
what modde found, and confirm the resolved path before deploying:

```bash
modde detect
```

Without registered Heroic GOG/Epic app ids, modde cannot auto-detect GOG or Epic
copies. Auto-detection is optional for deployment: you can point a profile
manually at any known install directory (see the worked example below).

### Proton prefix

The Witcher 3 runs through Proton on Linux. modde does not install Proton or the
game; the `proton` tool stores per-game launch settings (runner version, prefix
path override, extra environment, DLL-override mode). The DLL proxies modde
detects for script-extender-style mods (below) are surfaced as Wine DLL
overrides so they take effect inside the Proton prefix. See
[Tools & Overlays](../guides/tools.md) for the `proton` tool options.

## Mod directory & deploy strategy

The mod directory is `mods/` directly under the game install:

```
<install>/mods/
```

modde chooses the deployment layout based on REDengine's top-level
REDkit/REDmod-style roots: `mods/`, `dlc/`, `bin/`, and `content/`.

- **Multi-root overlay** — If the staged content already contains any of
  `mods/`, `dlc/`, `bin/`, or `content/` at its top level, modde symlinks those
  roots straight into the install (a symlink farm, leaving the original game
  files untouched). This is what most archive mods that bundle their own
  `mods/modFoo/` directory will hit.
- **Single mod-root deploy** — Otherwise modde treats the staged tree as the
  contents of one mod and deploys it into the resolved mod root under `mods/`.

Either way deployment is non-destructive and reversible — see
[Deployment & VFS](../guides/deployment.md) for the build → materialize → deploy
pipeline and `modde rollback`.

### Archive analysis

When modde unpacks a downloaded archive it picks an install method:

- If the extracted tree contains any of `mods`, `dlc`, `bin`, or `content`, it is
  installed as a **multi-root overlay** preserving those roots.
- Otherwise, if there is a single top-level directory whose name begins with
  `mod` (case-insensitive, e.g. `modFoo`), it is installed as a **directory
  mod** under `mods/`.

The same `mod*`-prefixed-directory heuristic is also used to recognise a "bare"
mod layout (a folder that *is* a single mod, with no wrapper directory).

## What scanning finds

`modde scan --game witcher3` walks the install and discovers already-present
mods in two directories:

| Directory | mod id prefix | Confidence |
| --------- | ------------- | ---------- |
| `mods/`   | `mod`         | 0.90       |
| `dlc/`    | `dlc`         | 0.75       |

Each immediate sub-directory of `mods/` becomes a discovered mod (`mod/<name>`),
and each sub-directory of `dlc/` becomes a discovered DLC-style mod (`dlc/<name>`).
The on-disk footprint modde records for a discovered mod is the directory itself,
lowercased — `mods/<name>/` or `dlc/<name>/` — so it can be matched and removed
cleanly later. Import the results into a profile with:

```bash
modde scan --game witcher3 --import-to my-witcher3
```

See [Mod Scanning](../guides/scanning.md) for `--dry-run`, threshold tuning, and
manifest matching.

## Conflict classification

The Witcher 3 collision classifier treats `.bundle` and `.cache` as archive
extensions and otherwise uses modde's default per-extension severities. The most
important entries for this title:

| Extension(s) | Category / severity |
| ------------ | ------------------- |
| `ws` | Script — **Dangerous** (overrides game scripts) |
| `dll` | Binary — **Dangerous** |
| `xml`, `csv` | Config |
| `bundle`, `cache` | Archive |
| `dds`, `png`, `jpg`, `xbm` | Texture / Cosmetic |

Run the standard conflict report:

```bash
modde collisions --profile my-witcher3
```

### `.ws` script conflicts

Many mods ship gameplay scripts under `content/scripts/`, but REDengine compiles
only one copy of each script file. If two mods ship `r4Player.ws`, one silently
overrides the other unless you merge their changes.

modde detects this specifically:

- `has_script_conflict(mod_dir)` returns `true` if a mod directory contains *any*
  `.ws` file anywhere in its tree (case-insensitive).
- `script_conflict_paths(mods_root)` walks every installed mod under `mods/`,
  records the relative `.ws` paths each one provides (lowercased,
  forward-slashed), and returns exactly the script paths provided by **more than
  one** mod — i.e. the scripts that actually collide.

Because `.ws` is classified **Dangerous** and `content/scripts` is flagged as a
save-breaking directory, script-shipping mods also contribute to the save
fingerprint (below). When two mods collide on the same `.ws` file you generally
need a manual or tool-assisted script merge — modde tells you *which* files
collide, but it does not merge REDengine scripts for you. See
[Conflicts & Load Order](../guides/conflicts.md) for the general workflow and
per-file hiding.

## Save tracking

| Property | Value |
| -------- | ----- |
| Save directory | `~/Documents/The Witcher 3/gamesaves` |
| Tracked extension | `.sav` |
| Excluded | `*.png` (save thumbnails) |
| Recursive | No — top-level of the save dir only |
| Save profiles | Supported |

modde stores saves in the git-backed vault described in
[Save Management](../guides/saves.md), with one branch per profile. It tracks
top-level `*.sav` files and ignores the accompanying `*.png` thumbnails.

### What is fingerprinted

modde computes a SHA-256 **mod fingerprint** from the sorted list of enabled
mods classified as *save-breaking*, and stores it as a commit trailer on each
save snapshot. For The Witcher 3 the save-breaking surface is:

- Save-breaking extensions: `ws`, `xml`, `bundle`, `cache`, `csv`, `dll`
- Save-breaking directory: `content/scripts`

Cosmetic-only mods (textures such as `dds`, `png`, `jpg`, `xbm`) do not move the
fingerprint, so swapping a texture pack will not trigger a save-compatibility
warning. Adding or removing a script/archive mod will. On restore, modde compares
fingerprints and warns when the snapshot's save-breaking set differs from your
current profile.

## Plugin / load-order handling

REDengine has no Bethesda-style plugin system, so there is **no** `.esp`/`.esm`
load order, no LOOT sorting, and no plugin validation for this title. Mod
precedence is the ordinary modde load-order priority (later mods win file
conflicts), resolved during the VFS build phase. The one ordering-sensitive area
is `.ws` script conflicts, which modde surfaces (see above) but does not
auto-resolve. There is no built-in `mods.settings`/`modList` priority writer.

## Installer specifics

The Witcher 3 modding ecosystem does not use FOMOD, REDmod CLI packaging, UE
`.pak` chunks, or SMAPI. In practice mods are distributed as archives that either:

1. contain the REDengine root layout (`mods/`, `dlc/`, `bin/`, `content/`), or
2. contain a single `mod*`-prefixed directory.

modde handles both via the archive-analysis heuristics described above — there is
no separate scripted-installer step for this title. (For games that *do* use
guided installers, see [FOMOD Installers](../guides/fomod.md).)

`bundle` and `cache` are treated as packed archive formats for conflict
reporting; modde does not repack them.

## Gaming tools that matter

The Witcher 3's OptiScaler profile list is empty. Use the generic `optiscaler`
tool or your own pin for upscaling. Other useful tools:

- **`proton`** — runner version, prefix override, `extra_env`, and DLL-override
  mode. This is where you control how the game launches under Proton.
- **`mangohud` / `vkbasalt` / `gamemode`** — overlay, post-processing, and the
  Feral GameMode daemon, all wired through modde's launcher integration.
- **Script-extender-style DLLs** — see the Wine DLL proxies below.

See [Tools & Overlays](../guides/tools.md) for enabling and configuring each.

### Wine DLL proxies

Several Witcher 3 mods inject through a proxy DLL placed next to the game
executable (in `bin/x64`). modde knows the proxy names this title uses and emits
the corresponding Wine DLL overrides so they load inside the Proton prefix:

```
dxgi, d3d11, version, winmm
```

When a staged mod (or the live executable directory) contains one of these DLLs,
modde adds it to the game's Wine DLL overrides automatically, so the proxy is
picked up under Proton without hand-editing the prefix.

## Linux / Proton notes & gotchas

The Witcher 3 runs through Proton/Wine on Linux and macOS. On Windows it runs
natively without a Wine prefix: proxy DLLs load directly and saves use the
native Windows location. modde runs natively on all three platforms.

- **Documents path.** The save directory is the Wine/Proton-mapped
  `~/Documents/The Witcher 3/gamesaves`. If your distro or home setup relocates
  `Documents`, confirm `modde detect`/save scanning is looking where the Proton
  prefix actually writes saves.
- **Script merging is on you.** modde flags colliding `.ws` files but does not
  merge REDengine scripts. For modlists that touch the same scripts, plan to do a
  manual or tool-assisted merge — and re-check `modde collisions` afterward.
- **Proxy DLLs need the override.** A mod that relies on a `dxgi`/`d3d11`/
  `version`/`winmm` proxy will silently do nothing under Proton if the override
  is missing. modde sets these automatically when it sees the DLL; if you install
  such a mod outside modde, you may need to set the override yourself via the
  `proton` tool.
- **No Heroic auto-detect.** GOG/Epic copies are not auto-discovered; point the
  profile's `gameDir` at the install directory explicitly.

## Worked example

### Home-Manager profile

```nix
programs.modde = {
  enable = true;

  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.my-witcher3 = {
    game = "witcher3";
    installMode = "auto";
    gameDir = "/home/me/.local/share/Steam/steamapps/common/The Witcher 3";

    tools = {
      gamemode.enable = true;
      vkbasalt = {
        enable = true;
        settings = {
          enableOnLaunch = true;
          casSharpness = 0.4;
        };
      };
      proton = {
        enable = true;
        settings = {
          version_mode = "launcher_default";
        };
      };
    };
  };
};
```

Declare the profile before the game is installed by omitting `gameDir` or setting
`installMode = "await-game"`; activation prints the next step instead of failing.
After Steam finishes installing, set `gameDir` and rebuild. See the
[Home-Manager Module](../configuration/hm-module.md) reference for every option.

### CLI workflow

```bash
# 1. Confirm modde found the install
modde detect

# 2. Import any mods already sitting in mods/ and dlc/
modde scan --game witcher3 --import-to my-witcher3

# 3. Check for colliding files (especially .ws scripts)
modde collisions --profile my-witcher3

# 4. Adopt your existing saves into the vault
modde save adopt --game witcher3 --profile my-witcher3

# 5. Deploy and launch (captures saves on exit)
modde play my-witcher3 --game witcher3
```

## See also

- [Supported Games](supported-games.md)
- [Cyberpunk 2077](cyberpunk2077.md) — the other REDengine title
- [Deployment & VFS](../guides/deployment.md)
- [Mod Scanning](../guides/scanning.md)
- [Conflicts & Load Order](../guides/conflicts.md)
- [Save Management](../guides/saves.md)
- [Tools & Overlays](../guides/tools.md)
- [Playing a Game](../guides/playing.md)
- [Home-Manager Module](../configuration/hm-module.md)
- [Parity reference](../reference/parity.md)
