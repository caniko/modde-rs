# Subnautica 2

Subnautica 2 (`game_id = subnautica2`) shares modde's data-driven UE4 plugin with
Stellar Blade and Oblivion Remastered. The plugin configures `Ue4Game` per title
and reuses the trait implementation across Unreal games. Status is `Partial`:
modde detects the install, deploys mods, classifies conflicts, tracks saves, and
handles Wine/Proton DLL overrides. It uses the generic pak layout without a
Subnautica-specific scanner, save parser, or mod-loader integration.

> **Status baseline.** See [Supported games](supported-games.md) and
> `docs/capability-matrix.toml` for the authoritative per-game status.

## Engine and overall status

| Aspect | Value |
| ------ | ----- |
| `game_id` | `subnautica2` |
| Engine family | `Unreal4` (covers UE4 and UE5 pak/IoStore titles) |
| UE project name | `Subnautica2` |
| Steam App ID | `1962700` |
| Nexus domain | `subnautica2` |
| Overall status | `Partial` |
| Scanner | Yes (shared UE4 pak scanner) |
| Conflict detection | Yes (shared UE4 collision policy) |
| Save tracking | `Done` (Proton-prefix `.sav` capture) |
| Save profiles | Enabled (`with_save_profiles(true)`) |
| OptiScaler profile | None shipped |

The `Unreal4` engine family is modde's umbrella for Unreal pak/IoStore games
regardless of whether the title is technically UE4 or UE5 — the on-disk layout
(`<Project>/Content/Paks/`, `<Project>/Binaries/Win64/`, `~mods` mount ordering,
`pak`/`ucas`/`utoc` chunk triples) is the same, so the same code path drives both.

## How modde detects the install

Subnautica 2 is registered with Steam launcher IDs only:

| Launcher field | Value |
| -------------- | ----- |
| `steam_app_id` | `1962700` |
| `steam_dir` | `Subnautica2` |
| `heroic_gog_app_id` | _none_ |
| `heroic_epic_app_id` | _none_ |

modde reads `libraryfolders.vdf` and looks for `steamapps/common/Subnautica2` in
each Steam library, including those on extra drives. Without a Heroic (GOG/Epic)
mapping, non-Steam installs require `--game-dir` (CLI) or `gameDir` (home-manager).
Run `modde detect` to check the resolved path before using it in a profile.

The install root is the directory that contains the UE project folder
`Subnautica2/`. modde derives everything else from there:

- **Paks root:** `<install>/Subnautica2/Content/Paks`
- **Executable / proxy-DLL dir:** `<install>/Subnautica2/Binaries/Win64`

### Proton prefix

Saves and per-user config live inside the Proton prefix Steam creates for App ID
`1962700`, not in the install directory. modde resolves the prefix as
`<steam_library>/steamapps/compatdata/1962700/pfx`. If you have never launched the
game, that prefix does not exist yet and modde's save/config resolution returns
nothing — the fix is to **launch Subnautica 2 once through Steam (Proton)** so the
prefix is created, then re-run modde.

## Mod directory and deploy strategy

modde deploys mods into the tilde-prefixed `~mods` folder under the paks root:

```
<install>/Subnautica2/Content/Paks/~mods/
```

Unreal mounts `~mods` **after** the shipping paks, so mods override base content.
modde creates the directory if missing.

Deployment uses modde's standard staging-and-link model (see
[Deployment](../guides/deployment.md)): mods live in per-mod staging directories,
and the active profile is materialised into `~mods` by linking the enabled mods'
files. A `pak`/`ucas`/`utoc` triple that shares a file stem is treated as one
logical mod — the IoStore container (`.ucas`/`.utoc`) and its `.pak` header travel
together.

During analysis, a root-level `.pak`, `.ucas`, or `.utoc` selects
`InstallMethod::SingleFileSet` for installation into `~mods`. If none is present
at the root, analysis falls through to the generic installer.

## What scanning finds

`modde scan --game subnautica2` walks two directories under the paks root and
groups `.pak`/`.ucas`/`.utoc` files by file stem into discovered mods:

| Scanned subdirectory | Source location label |
| -------------------- | --------------------- |
| `Subnautica2/Content/Paks/~mods` | `paks-mods` |
| `Subnautica2/Content/Paks/LogicMods` | `logic-mods` |

`LogicMods` is included because UE4SS-style Blueprint/logic mods conventionally
ship there; modde reports anything it finds in that folder even though it deploys
its own managed mods into `~mods`. Each discovered mod gets an ID of the form
`pak/<stem>` and is recorded at a confidence of `0.9`. Sibling `.ucas`/`.utoc`
files collapse into the same row as their `.pak` (the footprint is represented by
`subnautica2/content/paks/~mods/<stem>.pak`), so a multi-chunk mod shows up once
rather than three times.

Scanning finds pak sets on disk without parsing pak contents, reading UE asset
trees, or resolving load order. See [Scanning](../guides/scanning.md) for how
discovered mods reconcile with manifest-driven installs.

## Conflict classification

Conflicts are classified by the shared UE4 collision policy. Two mods "collide"
when they ship a file at the same relative path; the **severity** of that overlap
is decided by file extension:

| Extension | Severity | Meaning |
| --------- | -------- | ------- |
| `pak`, `ucas`, `utoc` | `Dangerous` | Overlapping game-content chunks — last-writer wins and the loser's assets are masked |
| `dll`, `lua` | `Dangerous` | Code/script overlap (proxy DLLs, UE4SS Lua) |
| `ini`, `cfg`, `json`, `toml`, `xml`, `yaml` | `Config` | Configuration overlap — usually mergeable/tunable by hand |
| `dds`, `png`, `jpg`, `tga` | `Cosmetic` | Texture overlap — visual only |

The archive extensions that participate in pak-aware collision handling are
`pak`, `ucas`, and `utoc`. Because Subnautica 2 mods are predominantly pak sets,
most real conflicts you will see are `Dangerous` same-path pak overlaps: two mods
both shipping, say, `~mods/zMyOverhaul.pak` cannot coexist without one shadowing
the other. modde surfaces these so you can choose a winner via load order /
filename prefixing rather than discovering it in-game. See
[Conflicts](../guides/conflicts.md) for the full classification model and how to
resolve overlaps.

## Save tracking

Save tracking is shipped (`Done`) and Subnautica 2 opts into modde's per-profile
save layer (`supports_save_profiles = true`).

### Location

modde resolves the save directory inside the Proton prefix:

```
<steam_library>/steamapps/compatdata/1962700/pfx/drive_c/users/steamuser/
  AppData/Local/Subnautica2/Saved/SaveGames
```

If the prefix does not exist yet (game never launched), save resolution returns
nothing — launch once through Proton first.

### What is captured and fingerprinted

The UE4 save tracker is a pattern tracker that recursively matches `*.sav` files
under the save directory, labels each by its file stem, and files them under the
`manual` category. "Recursive" matters for Subnautica 2 because UE titles often
nest per-slot save folders; modde descends into them.

Captured saves are committed into a **per-game git vault** (one repository per
`game_id`, one branch per profile), which gives history, branching, and rollback
for free. Each capture commit also records a **mod fingerprint**:

- The fingerprint is a **SHA-256 over the sorted, de-duplicated list of enabled,
  save-breaking mod IDs** in the active profile.
- A mod is "save-breaking" if it ships any of the save-affecting extensions
  `pak`, `ucas`, `utoc`, `dll`, or `lua` — i.e. exactly the content types that can
  change game logic. Purely cosmetic mods (`png`, `jpg`, `dds`, `tga`, `ini`) do
  not move the fingerprint, so two profiles that differ only in textures stay
  compatible.
- The fingerprint and a human-readable mod list are stored as
  `Mod-Fingerprint:` / `Save-Breaking-Mods:` git trailers on the capture commit.

When you later restore a snapshot, modde compares the stored fingerprint against
the current profile and warns if the save-breaking mod set has changed (which mods
were added/removed), so you do not silently load a save into an incompatible mod
configuration. See [Saves](../guides/saves.md) for the capture/restore/rollback
workflow.

> Note: the file content itself is not hashed — the fingerprint identifies the
> **mod configuration** that produced the save, not the bytes of the save file.

## Per-user config deploy target

Beyond saves, the UE4 plugin exposes one deploy target for user-editable config:

| Target ID | Label | Resolves to |
| --------- | ----- | ----------- |
| `ue4-saved-config` | UE4 Saved/Config | `compatdata/1962700/pfx/drive_c/users/steamuser/AppData/Local/Subnautica2/Saved/Config/Windows` |

This lets modde place INI tweaks into the prefix's `Saved/Config/Windows`
directory. As with saves, the target resolves to nothing until the Proton prefix
exists.

## Plugin and load-order handling

Unreal pak games have no `plugins.txt`-style master/plugin list the way Bethesda
titles do. Load order is **filename/mount order** within `~mods`: paks mount in a
deterministic order, and `~mods` mounts after the base paks. There is no Subnautica
2 load-order editor in modde; if two pak sets conflict, you resolve it by choosing
which mod wins (renaming with a sorting prefix, or disabling one). modde's job here
is to make the conflict visible and the deployment reproducible — not to arbitrate
UE pak precedence automatically.

## Installer specifics

Subnautica 2 mods are pak-based, so there is no FOMOD/REDmod/SMAPI flow specific to
this title:

- **Pak sets** are the native format. modde's archive analysis detects a root-level
  `.pak`/`.ucas`/`.utoc` and deploys the set into `~mods`.
- **FOMOD** installers are still supported generically (the FOMOD engine is
  engine-agnostic), but most Subnautica 2 mods are plain pak archives that need no
  scripted installer. See [FOMOD installers](../guides/fomod.md) if you do hit one.
- **No REDmod / no SMAPI / no Bethesda plugin tooling** applies here — those belong
  to Cyberpunk and Stardew/Creation-Engine titles respectively.

## Gaming tools that matter

modde manages external gaming tools per game (see [Tools](../guides/tools.md)). For
Subnautica 2:

- **Proton** is the runtime. modde's `proton` tool selects the Proton build and can
  apply DLL overrides; everything below the executable runs inside the App ID
  `1962700` prefix.
- **Wine DLL overrides for proxy DLLs.** UE mod loaders (UE4SS) and overlays ship as
  proxy DLLs that must be loaded as native instead of Wine's built-in stub. modde
  scans `Subnautica2/Binaries/Win64/` (or a mod's staging dir) for known proxies and
  emits the right `WINEDLLOVERRIDES`. The recognised proxy DLLs are:

  | Proxy DLL | Typical use |
  | --------- | ----------- |
  | `dwmapi` | UE4SS default proxy |
  | `xinput1_3` | UE4SS alternate / some trainers |
  | `d3d11` | ReShade / ENB-style |
  | `dxgi` | ReShade / DLSS swappers |
  | `version` | Generic ASI loader |
  | `winmm` | Generic ASI loader |
  | `dinput8` | Generic hook |

- **No OptiScaler profile ships for Subnautica 2.** Unlike Stellar Blade — which has
  a community-tested `community-dxgi` OptiScaler preset — `subnautica2` registers an
  empty OptiScaler profile list. You can still enable the generic `optiscaler` tool
  if you know what you are doing, but modde provides no curated, game-specific preset
  for this title, so there is no `profile = "..."` value to select. Overlays like
  MangoHud, VkBasalt, and GameMode are engine-agnostic and work as for any Proton
  title.

## Linux / Proton notes and gotchas

The prefix-specific notes below apply on Linux and macOS, where Subnautica 2
runs through Proton/Wine. On Windows it runs natively without a Wine prefix and
keeps saves and per-user config at their native Windows locations. modde runs
natively on all three platforms.

- **Launch once before modding state resolves.** Saves, the `ue4-saved-config`
  target, and prefix-based config all require the Proton prefix to exist. A fresh
  install has no `compatdata/1962700` until you run the game once.
- **`~mods` lives in the install, saves live in the prefix.** These are two different
  trees on disk. Deploy/scan operate on `<install>/Subnautica2/Content/Paks`; save
  capture operates on the prefix. Do not look for saves under the install directory.
- **IoStore triples must stay together.** Deploying or removing a `.pak` without its
  `.ucas`/`.utoc` (or vice versa) produces a broken mount. modde groups them by stem,
  so let modde manage the set rather than hand-copying single files.
- **Browse Nexus picker.** Subnautica 2 has a Nexus domain (`subnautica2`) but no
  numeric Nexus game ID recorded, so URL-based installs and update tracking work
  while the UI's "Browse Nexus" game picker may hide the title until the numeric ID
  is confirmed. You can still install from a `nexusmods.com/subnautica2/mods/<id>`
  URL or `nxm://` link.
- **Verify your runtime against the game's current state.** Subnautica 2 is an
  evolving title; pak mods can break across game patches. Capture a save before a big
  mod change so the fingerprint lets you roll back cleanly.

## Worked example

### Home-Manager profile

A minimal declarative Subnautica 2 profile. Point `gameDir` at the Steam install
(the directory containing `Subnautica2/`), and let activation manage the rest.

```nix
programs.modde = {
  enable = true;

  profiles.subnautica2-modded = {
    game = "subnautica2";
    installMode = "auto";
    gameDir =
      "/home/me/.local/share/Steam/steamapps/common/Subnautica2";

    tools = {
      # Engine-agnostic overlays work fine on this Proton title.
      mangohud.enable = true;
      gamemode.enable = true;
      # No OptiScaler preset ships for subnautica2, so there is no
      # `profile = "..."` to set here.
    };
  };
};
```

If the game is not installed yet, omit `gameDir` or set
`installMode = "await-game"`; activation prints the next step instead of failing.
See the [Home-Manager module reference](../configuration/hm-module.md) for every
option.

### CLI

```bash
# See what modde resolves for the install and Proton prefix.
modde detect

# Create a profile and scan the pak directories for existing mods.
modde profile create modded --game subnautica2
modde scan --game subnautica2 --import-to modded

# Install a pak mod from a Nexus URL into the profile.
modde install mod "https://www.nexusmods.com/subnautica2/mods/123" \
  --profile modded

# Deploy the active profile's mods into Content/Paks/~mods.
modde deploy --game subnautica2 --profile modded

# Switch profile, deploy, capture the outgoing profile's saves, and launch.
modde play modded --game subnautica2
```

If your copy is not a default Steam install, add `--game-dir <path>` to the
commands that take it (`scan`, `deploy`), pointing at the directory that contains
`Subnautica2/`.

### Named executables (modding tools)

modde can manage named launch targets — UE4SS-driven tools, trainers, or any helper
executable — with overwrite capture, custom working dir, environment, and Wine DLL
overrides. The `exec` commands are a thin alias for the executable subset of `tool`;
the two share storage.

```bash
# Register a tool as a named launch target for subnautica2.
modde exec add my-tool "/path/to/Tool.exe" --game subnautica2 \
  --wine-dll-overrides "dinput8=n,b" \
  --env "SOME_VAR=1" \
  -- --tool-arg value

# Equivalent via the tool surface:
modde tool add-executable my-tool "/path/to/Tool.exe" --game subnautica2

# Run it, capturing any files it writes into the overwrite mod.
modde exec run my-tool --game subnautica2 --profile modded
```

Captured output defaults to the `__overwrite__` mod; pass `--output-mod <name>` to
route writes into a named mod instead. See [Tools](../guides/tools.md) and
[Playing](../guides/playing.md) for the full launch model.

## See also

- [Supported games](supported-games.md) — the canonical status table and per-game guide index
- [Stellar Blade](stellar-blade.md) — sibling Unreal title (has an OptiScaler preset)
- [Oblivion Remastered](oblivion-remastered.md) — another `Unreal4` pak title
- [Deployment](../guides/deployment.md) — staging-and-link deployment model
- [Scanning](../guides/scanning.md) — filesystem discovery and manifest reconciliation
- [Conflicts](../guides/conflicts.md) — collision classification and resolution
- [Saves](../guides/saves.md) — capture, fingerprints, restore, and rollback
- [Tools](../guides/tools.md) — Proton, overlays, and named executables
- [Playing](../guides/playing.md) — switch-deploy-capture-launch flow
- [Home-Manager module](../configuration/hm-module.md) — declarative profile options
- [Parity reference](../reference/parity.md) — how modde's coverage maps to MO2/Vortex
