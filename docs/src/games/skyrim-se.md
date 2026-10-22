# Skyrim Special Edition / Anniversary Edition

Skyrim Special Edition (`skyrim-se`) and Anniversary Edition (`skyrim-ae`) are
modde's most complete, end-to-end-tested titles. They share this guide because
they are the same game build: identical Steam App ID, install directory, INI
files, archive formats, and Nexus domain. The Anniversary Edition is the Special
Edition plus Creation Club content, so everything below applies to both ids
unless explicitly noted.

Both are marked **`Done`** in the
[supported-games matrix](supported-games.md): the scanner, conflict detection,
and save tracking are all shipped and user-reachable, and the Wabbajack +
home-manager install path described at the end is modde's strongest worked
example.

## Engine and overall status

| Property | Value |
| -------- | ----- |
| Engine | Bethesda Creation Engine |
| Game ids | `skyrim-se`, `skyrim-ae` (share this guide) |
| Steam App ID | `489830` (both editions) |
| Steam install dir | `Skyrim Special Edition` |
| Mod directory | `Data/` |
| Archive formats | `.bsa`, `.ba2` |
| INI files (per-profile) | `Skyrim.ini`, `SkyrimPrefs.ini`, `SkyrimCustom.ini` |
| Nexus domain | `skyrimspecialedition` |
| Nexus numeric game id | `1704` |
| Plugin system | Yes (`plugins.txt` + LOOT) |
| Scanner | `Done` |
| Conflict detection | `Done` (loose files + BSA/BA2 contents) |
| Save tracking | `Done` |

`skyrim-ae` is registered as a distinct game so you can target it explicitly, but
launcher detection reports `skyrim-se` by default because AE reuses the SE Steam
app and install directory. When in doubt, use `skyrim-se`; pick `skyrim-ae` only
if a modlist or collection demands the AE id specifically.

## How modde detects the install

modde locates Skyrim through its launcher ids, then derives the per-user data
paths from the Steam App ID. On Linux and macOS Skyrim runs through Proton/Wine,
so those paths live inside the compatibility prefix; on Windows the game runs
natively and the same paths are their native Windows locations.

- **Steam.** modde resolves `489830` and the `Skyrim Special Edition` directory
  under your Steam libraries. Heroic GOG/Epic ids are not set for Skyrim SE/AE
  (it is a Steam title), so detection is Steam-based.
- **Proton prefix (Linux/macOS).** Save and `plugins.txt` paths are read from the
  Proton compatibility prefix keyed by App ID:

  ```text
  <steam>/steamapps/compatdata/489830/pfx/drive_c/users/steamuser/...
  ```

For Wabbajack lists and any modlist that references the installed base game, you
should still point modde at the game directory explicitly with `--game-dir`
(CLI) or `gameDir` (home-manager) — see [Linux/Proton notes and known
gotchas](#linux-proton-notes-and-known-gotchas).

## Mod directory and deploy strategy

Every Bethesda title deploys into the game's `Data/` directory. modde computes
the mod directory as `<install>/Data` and stages mod content there. The deploy
itself is symlink-based, consistent with all Bethesda games, so the game's own
`Data/` stays a virtual composite of the active profile rather than a
destructively merged folder.

Two file classes get special treatment:

- **Loose files** (meshes, textures, scripts, INIs) are linked into `Data/`
  under their relative paths.
- **Archives** (`.bsa`, `.ba2`) are deployed **as-is, not extracted**, because
  the Creation Engine loads them natively. They are placed directly into `Data/`.

modde recognizes a "bare" extracted layout — a folder that already looks like a
`Data/` tree — by these root markers, matched case-insensitively:

- Root directories: `data`, `meshes`, `textures`, `scripts`, `interface`,
  `sound`, `music`, `materials`, `seq`, `shadersfx`, `strings`
- Root file extensions: `esp`, `esm`, `esl`, `bsa`, `ba2`

This lets modde correctly stage both `Data`-rooted archives and FOMOD output
without you having to flatten them by hand.

See [Deployment](../guides/deployment.md) for the general symlink model and how
overwrite/overrides priority works.

## What scanning finds

The Skyrim scanner walks `Data/` and reconstructs your installed mods, using
`plugins.txt` as the authoritative source of load order and enabled state.

It works in two passes:

1. **Authoritative pass.** Every plugin listed in `plugins.txt` that actually
   exists on disk is emitted as a discovered mod (`plugin/<filename>`,
   confidence `0.95`). For each plugin stem, the scanner also pulls in companion
   archives that share the stem — both `<stem>.bsa`/`<stem>.ba2` and the
   `<stem> - Textures.bsa`/`.ba2` texture-archive convention.
2. **Unmanaged pass.** Any `.esp`/`.esm`/`.esl` in `Data/` *not* already seen
   via `plugins.txt` is emitted with lower confidence (`0.8`) so disabled or
   externally-dropped plugins still surface.

Plugin extensions recognized: `.esp`, `.esm`, `.esl`. Archive extensions paired
with a plugin: `.bsa`, `.ba2`. Each discovered file records its `Data/`-relative
path and size.

See [Scanning](../guides/scanning.md) for how discovered mods feed adoption and
stale-duplicate detection.

## Conflict classification specifics

Skyrim uses the Bethesda collision classifier, which assigns a severity to every
overlapping file by extension — and, crucially, **indexes the contents of
`.bsa`/`.ba2` archives** so two mods that ship conflicting files *inside*
archives are still flagged.

| Severity | Extensions |
| -------- | ---------- |
| Dangerous | `esp`, `esm`, `esl`, `pex`, `dll`, `psc` |
| Config | `ini`, `cfg`, `json`, `toml`, `xml` |
| Cosmetic | `dds`, `png`, `tga`, `jpg`, `nif`, `hkx`, `fuz`, `wav`, `xwm`, `swf`, `btr`, `bto`, `btt`, `bsa`, `ba2` |

So two mods overwriting the same `.dds` are a cosmetic (last-wins, usually safe)
conflict, while two mods providing the same plugin record or compiled script
(`.pex`/`.psc`) or native DLL are flagged as dangerous and surfaced
prominently. Archive contents (`.bsa`/`.ba2`) are read and merged into the
collision map, so the analysis is not limited to loose files.

See [Conflicts](../guides/conflicts.md) for how severities drive the conflict
report and resolution order.

## Save tracking

Save tracking is `Done` for Skyrim SE/AE. modde reads saves from the Proton
prefix:

```text
<steam>/steamapps/compatdata/489830/pfx/drive_c/users/steamuser/Documents/My Games/Skyrim Special Edition/Saves
```

What it captures and fingerprints:

- It tracks `*.ess` save files (and `*.bak` are recognized but skipped as backup
  copies). The Skyrim **`.skse` co-save** written alongside each `.ess` is
  explicitly *excluded* from tracking — it is a SKSE side file, not a save of
  record.
- For each `.ess`, modde parses the binary header (magic `TESV_SAVEGAME`) and
  extracts the **save number** and **character name**, producing labels like
  `Lydia — Save 14`. If the header is unreadable or has the wrong magic (e.g. a
  save from another game dropped in the folder), the file is still captured but
  without a label.
- Slot category is inferred from the filename: `Autosave*` → `auto`,
  `Quicksave*` → `quick`, everything else → `manual`. A multi-save capture is
  summarized grouped by character (e.g. `Lydia (slots 14, 15); Orc Mage (slot 7)`).

Save profiles are enabled for Skyrim, so saves are captured per modde profile —
letting you swap modlists without cross-contaminating character saves.

This save fingerprinting matters because Skyrim saves bake in your active
plugins: removing or changing a save-breaking mod can corrupt or invalidate an
existing character. modde treats the following extensions as **save-breaking**,
which is what triggers a save fingerprint before a destructive change: `.esp`,
`.esm`, `.esl`, `.pex`, `.dll`, `.psc`. Cosmetic-only changes (`.nif`, `.dds`,
`.bsa`/`.ba2`, audio, etc.) do not.

See [Saves](../guides/saves.md) for the general capture/restore workflow.

## Plugin and load-order handling

Skyrim is one of the few engines with full plugin/load-order support in modde.

### plugins.txt

modde reads and writes the standard Bethesda `plugins.txt` format inside the
Proton prefix:

```text
~/.local/share/Steam/steamapps/compatdata/489830/pfx/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition/plugins.txt
```

Format rules: a leading `*` marks a plugin **enabled**, no prefix means
**disabled**, and `#` lines are comments. modde-written files start with a
generated header comment (`# This file is generated by modde. Do not edit
manually.`).

modde keeps plugin load order separate from mod install priority — "which mod's
files win on disk" is independent from "which `.esp` loads first" — mirroring
MO2's dual-pane model.

### LOOT sorting

modde ships a pure-Rust LOOT masterlist parser (no `libloot` FFI). It downloads
the public LOOT masterlist for Skyrim SE/AE from
`loot/skyrimse` and converts its rules to modde's internal load-order rules:

- `after` → load-after rule
- `requires` → load-after rule (and a missing-master signal if absent)
- `incompatible` → incompatible-pair rule

Rules are generated only for plugins that are actually in your active set, and
matching is case-insensitive. `skyrim-se` and `skyrim-ae` share the same
masterlist URL.

```bash
modde loot sort --game skyrim-se
```

### Plugin diagnostics

modde reads the binary TES4 header of each active plugin (only the first ~1 KB)
to run two Skyrim-specific checks plus a shared one:

| Diagnostic | Severity | What it means | Suggested fix |
| ---------- | -------- | ------------- | ------------- |
| Missing master | Error | A plugin lists a master file that is not in the active load order; the game crashes on load | Install and enable the mod providing that master |
| Form 43 plugin | Warning | A plugin uses the Oldrim/LE format (Form 43) instead of SSE's Form 44; can cause crashes in SE/AE | Open it in the SSE Creation Kit and re-save to convert to Form 44 |
| Orphaned overrides | Info | The profile's `overrides/` directory contains files (highest priority, wins over all mods) | Review the files to confirm they are intentional |

The Form 43 check is gated to `skyrim-se`/`skyrim-ae` specifically, since the
Oldrim-vs-SSE plugin format distinction is unique to Skyrim. ESM/ESL flags are
read from the same header.

```bash
modde loot validate --game skyrim-se
```

## Installer specifics

- **FOMOD.** Scripted and basic FOMOD installers are supported; FOMOD output is
  recognized as a `Data/`-rooted layout via the bare-layout markers above, so the
  selected options stage correctly into `Data/`. See the
  [FOMOD installer guide](../guides/fomod.md).
- **BSA/BA2.** Archives are deployed as-is into `Data/` and indexed for conflict
  analysis — they are never unpacked.
- **Wabbajack.** Skyrim SE is modde's best-supported Wabbajack target. modde
  parses Wabbajack manifests, including `GameFileSourceDownloader` entries (local
  vanilla game files referenced by the list), Wabbajack CDN authored-files
  archives, Nexus downloads, and ModDB-style mirror pages. See
  [Wabbajack](../guides/wabbajack.md) and the worked example below.
- **REDmod / pak / SMAPI** do **not** apply to Skyrim — those are Cyberpunk,
  UE4/5, and Stardew constructs respectively. Skyrim's content model is loose
  files plus `.bsa`/`.ba2` archives plus `.esp`/`.esm`/`.esl` plugins.

## Gaming tools that matter for this title

- **Proton.** Skyrim SE/AE run under Proton; modde derives its save and
  `plugins.txt` paths from the Proton prefix automatically once the game is
  installed.
- **SKSE (Skyrim Script Extender).** Many mods depend on SKSE. modde does not
  install SKSE for you, but you can register the SKSE loader as a named
  executable and launch it through modde — see the executable-management
  workflow (`modde tool add-executable` / `modde exec`) in the
  [tools guide](../guides/tools.md).
- **OptiScaler.** modde ships no OptiScaler profiles for Skyrim
  (`optiscaler_profiles` is empty for both ids); OptiScaler/upscaler tooling is
  relevant to the UE4/5 titles, not Creation Engine Skyrim.

## Linux/Proton notes and known gotchas

These notes concern the game's runtime on Linux and macOS, where Skyrim runs
through Proton/Wine. On Windows the game runs natively, so the prefix-specific
points below do not apply — saves, `plugins.txt`, and INI files live at their
native Windows locations. modde itself runs natively on all three platforms.

- **`--game-dir` / `gameDir` is required for vanilla-referencing modlists.** Many
  Wabbajack lists — Legends of the Frost is the canonical example — reference
  *vanilla* Skyrim `Data` files as install inputs (`GameFileSourceDownloader`).
  modde must read and hash-verify those local game files before staging the list,
  so you have to point it at the installed game directory. Without it, the install
  fails with a clear missing-`--game-dir` error rather than silently producing a
  broken profile.
- **`installMode = "await-game"` (home-manager).** You can declare a Wabbajack
  profile *before* Skyrim is installed. In `await-game` mode, activation prints
  the next step and exits successfully instead of failing; once Skyrim is
  installed through Steam/Heroic, set `gameDir` and switch `installMode` to
  `"auto"` (or remove it).
- **Authored-files availability.** Wabbajack lists can break when their upstream
  authored-files archives are taken down (HTTP 404). modde runs an availability
  preflight and fails fast, reporting exactly which authored-file ids are
  missing, before starting bulk downloads. If you have exact local copies, import
  them by hash with `modde wabbajack import-archive` (name-only matches are
  refused).
- **Form 43 plugins.** Oldrim-era mods ported carelessly to SE/AE can carry the
  Form 43 header and crash; run `modde loot validate --game skyrim-se` to catch
  them.

## Worked example

### CLI: install a Wabbajack list and deploy

```bash
# Install a Wabbajack modlist, verifying vanilla game files under --game-dir.
modde install wabbajack /path/to/lotf.wabbajack \
  --profile skyrim \
  --game-dir "/path/to/Skyrim Special Edition"

# Sort the load order against the LOOT masterlist and check for problems.
modde loot sort --game skyrim-se
modde loot validate --game skyrim-se

# Deploy the profile into the game's Data/ directory.
modde deploy --profile skyrim --game skyrim-se
```

If you have local copies of authored archives that were pulled upstream:

```bash
modde wabbajack import-archive /path/to/list.wabbajack \
  /path/to/Archive-A.7z \
  /path/to/Archive-B.7z
```

### Home Manager: declarative Wabbajack profile

```nix
programs.modde = {
  enable = true;
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.skyrim = {
    game = "skyrim-se";
    installMode = "auto";
    gameDir = "/path/to/Skyrim Special Edition";
    wabbajackList = {
      url = "https://authored-files.wabbajack.org/...";
      hash = "sha256-...";
    };
  };
};
```

`gameDir` is required for lists (like Legends of the Frost) whose manifest
references local vanilla game files; modde verifies them before staging.

If Skyrim is not installed yet, declare the profile in awaiting mode and fill in
`gameDir` after installing the game through Steam or Heroic:

```nix
programs.modde.profiles.skyrim = {
  game = "skyrim-se";
  installMode = "await-game";
  wabbajackList = {
    url = "https://authored-files.wabbajack.org/...";
    hash = "sha256-...";
  };
};
```

Activation prints the next step and continues; once Skyrim exists, set `gameDir`
and switch `installMode` to `"auto"`.

> Note: a `wabbajackList` profile cannot also set a `nexusCollection` — the two
> install sources are mutually exclusive.

## See also

- [Supported games](supported-games.md)
- [Installation](../getting-started/installation.md)
- [Wabbajack modlists](../guides/wabbajack.md)
- [Deployment](../guides/deployment.md)
- [Conflicts](../guides/conflicts.md)
- [Scanning](../guides/scanning.md)
- [Saves](../guides/saves.md)
- [FOMOD installers](../guides/fomod.md)
- [Executables & tools](../guides/tools.md)
- [Troubleshooting](../guides/troubleshooting.md)
- [Parity reference](../reference/parity.md)
