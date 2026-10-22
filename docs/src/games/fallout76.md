# Fallout 76

Fallout 76 (`game_id` = `fallout76`) is one of the five Bethesda Creation
Engine titles modde ships with. It shares the Bethesda deploy strategy, archive
format, and INI machinery with Skyrim SE/AE, Fallout 4, and Starfield, but it is
the odd one out: it is an **always-online** game whose saves live on Bethesda's
servers, and whose loose-archive mods are activated through an INI line rather
than a local `plugins.txt`. Because of that, modde marks the title **`Partial`**
overall, with **`Partial` save tracking** in particular.

> Honest status: deployment, BA2 scanning, conflict classification, and launcher
> integration are real and working. What is *not* fully solved is save tracking
> (saves are server-side; only a local cache is visible) and load-order parity
> with single-player Bethesda games (Fallout 76 has no LOOT masterlist
> equivalent). See [Supported games](supported-games.md) and the
> [parity reference](../reference/parity.md) for the canonical status baseline.

## Engine and overall status

| Property | Value |
| -------- | ----- |
| Engine | Bethesda Creation Engine (`EngineFamily::Bethesda`) |
| modde `game_id` | `fallout76` |
| Steam App ID | `1151340` |
| Nexus domain | `fallout76` (numeric game id `2590`) |
| Wabbajack name | `Fallout76` |
| Overall status | `Partial` |
| Scanner | Yes (loose-archive scanner) |
| Conflict detection | Yes (shared Bethesda classifier) |
| Save tracking | `Partial` — server-side; local cache only |
| Plugin system | Reported as present, but no local load-order file |

The game is registered in `crates/modde-games/src/registry.rs` with the
`Bethesda` engine family, the shared Bethesda collision classifier, a
Fallout-76-specific archive scanner, and a Bethesda save tracker tagged as
"FO76". The plugin metadata lives in
`crates/modde-games/src/bethesda/mod.rs` as the `FALLOUT76` `BethesdaGame`
record.

## How modde detects the install

modde locates Fallout 76 the same way it finds every other Bethesda title: by
its launcher IDs. The registration carries:

- `steam_app_id = "1151340"`
- `steam_dir = "Fallout76"` (the folder under `steamapps/common/`)

There are **no Heroic GOG or Epic IDs** for Fallout 76 — it ships on Steam (and
the Microsoft Store / Bethesda launcher, which modde does not detect). On Linux
the Steam copy runs under Proton, so modde resolves both the install root and
the per-game Proton prefix from the Steam App ID:

```text
~/.local/share/Steam/steamapps/common/Fallout76/          # install root
~/.local/share/Steam/steamapps/compatdata/1151340/pfx/    # Proton prefix
```

The Proton prefix matters for two things modde reads under
`drive_c/.../AppData/Local/Fallout76/`: the (largely unused for FO76)
`plugins.txt` location and the `My Games` documents tree where the local save
cache lands.

## Mod directory and deploy strategy

Like all Bethesda games, the mod directory is **`Data/`** under the install
root:

```text
~/.local/share/Steam/steamapps/common/Fallout76/Data/
```

`mod_directory()` for the Bethesda plugin returns `install.join("Data")`, so
modde deploys mod files into `Data/` through its virtual-filesystem symlink
farm — the same overlay mechanism used for every game. Your real `Data/`
directory stays clean; modde links the active profile's files in and removes
them on profile switch or undeploy. See the
[Deployment & VFS guide](../guides/deployment.md) for how the overlay is built
and torn down.

When modde extracts an archive whose contents look like a bare Bethesda layout
(top-level `meshes/`, `textures/`, `scripts/`, `interface/`, `sound/`,
`strings/`, … or loose `.esp`/`.esm`/`.esl`/`.bsa`/`.ba2` files), it recognizes
it as a Data-rooted mod and lays it down accordingly. This is the
`BareLayoutPolicy` shared across Bethesda titles and is case-insensitive.

## What scanning finds

Fallout 76 uses a **dedicated scanner** that differs from the Skyrim/Fallout 4
scanner. The single-player Bethesda scanner walks `plugins.txt` for an
authoritative load order; Fallout 76 has no meaningful local load order, so it
uses `BethesdaArchiveScanner` instead.

The Fallout 76 scanner (`FALLOUT76_SCANNER`):

- scans the `Data/` directory only;
- discovers loose **`.ba2`** archive files (the Fallout 76 archive extension);
- **skips game-shipped archives** by ignoring any file whose name starts with
  the `SeventySix` prefix (those are vanilla content, not mods);
- assigns each discovered archive a mod id of the form `archive/<stem>` and a
  confidence of `0.8` (a heuristic match, since there is no load-order file to
  corroborate it);
- maps a mod id back to its on-disk footprint as `data/<stem>.ba2`, which lets
  modde detect stale duplicates and reconcile a scanned archive against a
  managed mod.

In short: scanning Fallout 76 returns the loose `.ba2` mods you have dropped in
`Data/`, excluding Bethesda's own `SeventySix*.ba2` archives. Unlike Skyrim or
Fallout 4, it does **not** pair plugins with companion archives or read enabled
state from a load-order file, because Fallout 76 mods are activated through the
INI rather than a plugin list. See the
[Scanning guide](../guides/scanning.md) for the general scan workflow.

## Conflict classification

Fallout 76 uses the shared **Bethesda collision classifier**, the same one used
by Skyrim and Fallout 4. modde indexes the contents of `.bsa`/`.ba2` archives
(via the archive index reader) and classifies a file collision by its
extension:

| Severity | Extensions (representative) |
| -------- | --------------------------- |
| Dangerous | `esp`, `esm`, `esl`, `pex`, `dll`, `psc` |
| Config | `ini`, `cfg`, `json`, `toml`, `xml` |
| Cosmetic | `dds`, `png`, `tga`, `jpg`, `nif`, `hkx`, `fuz`, `wav`, `xwm`, `swf`, `btr`, `bto`, `btt`, `bsa`, `ba2` |

So two mods that both overwrite a texture inside their `.ba2` archives produce a
`Cosmetic` collision (last-deployed wins, cosmetically), whereas two mods that
ship the same script or plugin produce a `Dangerous` collision worth your
attention. The classifier reads *inside* archives, so it can flag conflicts
between the contents of two `.ba2` files, not just same-named archives.

> Caveat for Fallout 76 specifically: Bethesda actively detects and bans
> certain client modifications in the online game. modde's conflict severity is
> about *file overwrites*, not about whether a given mod is permitted online.
> Treat `Dangerous`-class mods (loose scripts, DLLs, plugins) with extra care on
> an online title.

See the [Conflicts & load order guide](../guides/conflicts.md) for how
severities surface and how overwrite order is resolved.

## Save tracking

This is where Fallout 76 differs most from its single-player siblings, and why
its save tracking is **`Partial`**.

Fallout 76 is an online game: **the authoritative save is on Bethesda's
servers.** The local `.sav`/`.ess`-style files in the Proton prefix are at best a
partial cache, not your real character. modde represents this honestly:

- The Fallout 76 save tracker uses the magic header `FO76_SAVEGAME` to identify
  local Bethesda save files (the same binary header family as Skyrim's
  `TESV_SAVEGAME` and Fallout 4's `FO4_SAVEGAME`).
- When a save header parses, modde fingerprints the **save number** (a unique,
  incrementing slot id) and the **player name** read from the header.
- The tracker is flagged `is_fo76 = true`, so every capture is labelled with an
  explicit warning. Instead of the normal `capture: …` prefix, Fallout 76
  captures read:

  ```text
  capture (FO76 cache — server saves not tracked): …
  ```

That warning is the whole point: modde will fingerprint and snapshot what is on
disk, but it tells you plainly that this is a local cache and that your real
progress lives server-side and is **not** under modde's control. The save
directory modde looks in is resolved from the Proton prefix:

```text
.../compatdata/1151340/pfx/drive_c/users/steamuser/Documents/My Games/Fallout 76/Saves
```

(`my_games_dir = "Fallout 76"` for this title.) Do not rely on modde save
profiles to roll a Fallout 76 character back and forth the way you would for
Skyrim — the server state will not follow. For the general save-vault model
(git-backed vaults, fingerprints, auto-capture), see the
[Save management guide](../guides/saves.md).

## Plugins and load order

`has_plugin_system()` returns `true` for every Bethesda game, and modde knows the
Fallout 76 INI files it manages per profile:

- `Fallout76.ini`
- `Fallout76Prefs.ini`
- `Fallout76Custom.ini`

But in practice Fallout 76 has **no `plugins.txt`-driven load order parity** with
single-player Bethesda titles. There is no LOOT masterlist for Fallout 76, and
the game does not consume a `plugins.txt` the way Skyrim and Fallout 4 do.
Instead, loose `.ba2` archives are activated by listing them in the
`sResourceArchive2List` / custom archive lines of `Fallout76Custom.ini`. That is
why the scanner is archive-based rather than plugin-based, and why you should
think in terms of "which archives are listed in my custom INI" rather than
"what is my load order". modde manages the INI files as part of a profile; you
edit the archive list there to enable a loose `.ba2` mod.

## Installers (FOMOD, BA2, and friends)

Fallout 76 mods come in two common shapes, and modde handles both:

- **Loose / archived files (`.ba2` or bare `Data/` layouts).** Drop-in mods are
  recognized by the Bethesda bare-layout policy and deployed into `Data/`. A
  single `.ba2` is the canonical Fallout 76 mod unit.
- **FOMOD installers.** modde includes a FOMOD engine (re-exported from
  `fomod-oxide`) shared by all Bethesda titles, so a Fallout 76 mod packaged as
  a FOMOD with `ModuleConfig.xml` is installed through the same interactive or
  declarative flow as a Skyrim FOMOD. See the
  [FOMOD installer guide](../guides/fomod.md).

There is **no** REDmod, `pak`, or SMAPI handling here — those belong to
Cyberpunk 2077, Unreal titles, and Stardew Valley respectively. Fallout 76 is
archive-and-INI, plus FOMOD packaging.

## Gaming tools for this title

Fallout 76 carries **no built-in OptiScaler profile** in its registration
(`optiscaler_profiles` is empty) — unlike Stellar Blade, which ships a
`community-dxgi` preset. You can still attach the generic tools modde supports to
a Fallout 76 profile through the home-manager module or `modde tool` commands:

| Tool | Use on Fallout 76 |
| ---- | ----------------- |
| `proton` | Select the Proton runtime and set Wine/Proton DLL overrides for the prefix |
| `mangohud` | Performance HUD overlay |
| `gamemode` | System performance tuning at launch |
| `vkbasalt` | Vulkan post-processing (sharpening, CAS) |
| `reshade` | D3D post-processing for the Wine-backed game |
| `optiscaler` | DLSS/FSR/XeSS upscaling — usable, but no curated FO76 preset ships |

The most relevant of these for an online Proton title is `proton` itself, for
runtime selection and DLL overrides. See the [Tools & overlays
guide](../guides/tools.md) for how tools attach to a profile and the
[Playing a game guide](../guides/playing.md) for the deploy-launch-capture flow.

## Linux and Proton notes / known gotchas

The prefix-specific points here describe the game's runtime on Linux and macOS,
where Fallout 76 runs through Proton/Wine. On Windows the game runs natively, so
there is no Wine prefix and the local cache and INI files live at their native
Windows locations. modde itself runs natively on all three platforms.

- **Always-online; saves are server-side.** This is the single biggest gotcha.
  modde can snapshot the local cache but cannot version your real character.
  Every Fallout 76 capture is labelled `FO76 cache — server saves not tracked`.
- **Anti-cheat / bannable mods.** Bethesda enforces what is allowed in the live
  game. modde's `Dangerous` severity flags *file* risk, not online-policy risk;
  loose scripts/DLLs/plugins can get you actioned regardless of how modde
  classifies the overwrite. Mod conservatively on a live online account.
- **Activation is via `Fallout76Custom.ini`, not a load order.** If a `.ba2`
  mod does nothing after deploy, confirm its archive name is listed in the
  custom INI's archive list — there is no `plugins.txt` to enable it.
- **Vanilla archives are skipped on scan.** Anything named `SeventySix*` is
  treated as base-game content and excluded from discovery, so it will not show
  up as a "mod".
- **No Heroic path.** Detection is Steam-only for this title; there are no GOG or
  Epic launcher IDs.

## Worked example

### Home-Manager profile

A minimal Fallout 76 profile. Because saves are server-side, there is little
point in elaborate save automation here — keep the profile focused on deploying
loose-archive mods and selecting a Proton runtime:

```nix
programs.modde = {
  enable = true;
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.fo76 = {
    game = "fallout76";
    installMode = "auto";
    gameDir = "/home/me/.local/share/Steam/steamapps/common/Fallout76";

    tools = {
      proton.enable = true;
      gamemode.enable = true;
      mangohud.enable = true;
    };
  };
};
```

### CLI

```bash
# Confirm modde sees the install (Steam App ID 1151340).
modde game list

# Scan Data/ for loose .ba2 mods (skips SeventySix* vanilla archives).
modde scan --game fallout76

# Inspect conflicts between deployed archives' contents.
modde conflicts --game fallout76

# Deploy the active profile's mods into Data/ and launch under Proton.
modde play --game fallout76

# Snapshot the local save cache. Note the FO76 server-side warning in output.
modde save auto-capture --game fallout76
```

`modde save auto-capture --game fallout76` will print a capture line prefixed
with `capture (FO76 cache — server saves not tracked)`, reflecting that the real
character lives on Bethesda's servers.

## See also

- [Supported games](supported-games.md) — the canonical status table
- [Fallout 4](fallout4.md) and [Starfield](starfield.md) — sibling Creation
  Engine titles with full single-player save tracking
- [Deployment & VFS](../guides/deployment.md)
- [Mod scanning](../guides/scanning.md)
- [Conflicts & load order](../guides/conflicts.md)
- [Save management](../guides/saves.md)
- [FOMOD installer](../guides/fomod.md)
- [Tools & overlays](../guides/tools.md)
- [Playing a game](../guides/playing.md)
- [Parity reference](../reference/parity.md)
