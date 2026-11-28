# Mount & Blade II: Bannerlord

Mount & Blade II: Bannerlord (`bannerlord`) is TaleWorlds' medieval sandbox, built on its in-house engine. Its flat `Modules/` directory and per-module `SubModule.xml` manifests differ from Bethesda, Unreal, REDengine, and SMAPI layouts, so modde uses `EngineFamily::Bannerlord`.

## Overall status

Bannerlord is **`Partial`** in [the supported-games matrix](supported-games.md):

- **Scanner** — Yes. modde discovers installed modules under `Modules/` and parses each `SubModule.xml` for identity and dependencies.
- **Conflict detection** — Yes, via the generic policy classifier plus Bannerlord-specific save-breaking and cosmetic classification.
- **Save tracking** — `Done`. The git-backed save vault, fingerprinting, and capture/restore all work against the Bannerlord save directory.

The `Partial` rating reflects missing features: modde has no Bannerlord-aware load-order manager; the game's Launcher / Mod Options screen controls order. The mod information dialog has only a Nexus-metadata side panel, not a file tree or dependency graph. Deployment, scanning, conflict classification, save management, and launcher integration work.

| Capability | Status | Notes |
| ---------- | ------ | ----- |
| Install detection (Steam) | Yes | Steam app id `261550`, install dir `Mount & Blade II Bannerlord` |
| Mod directory | Yes | `Modules/` under the install root |
| Filesystem scanner | Yes | Walks `Modules/`, parses `SubModule.xml` |
| Dependency checking | Yes | Reports `<DependedModule>` ids missing from the module set |
| Conflict classification | Yes | Generic classifier + Bannerlord save-breaking / cosmetic policy |
| Save tracking | `Done` | `Documents/Mount and Blade II Bannerlord/Game Saves/Native`, `*.sav` |
| Load-order management | Not shipped | Use the in-game Launcher's Mod Options screen |
| OptiScaler profiles | Not shipped | No bundled upscaling profile (see [tools](#gaming-tools-proton-and-overlays)) |

## How modde detects the install

modde searches Steam libraries for app id **`261550`** first, then falls back to the default directory name **`Mount & Blade II Bannerlord`**. Bannerlord is a Steam title with no registered Heroic (GOG/Epic) ids. For a non-Steam install, pass `--game-dir` (see the [worked example](#worked-example)).

On Linux, Steam runs Bannerlord through Proton. modde reads and writes the host paths directly: `Modules/` in the game install and the save directory under `$HOME/Documents`. Proton maps those paths into Windows, so you usually don't need to enter the prefix to manage mods. modde records `bin/Win64_Shipping_Client/` relative to the install root as the Windows executable directory for tools and launch integrations.

## Mod directory and deploy strategy

The mod directory is **`Modules/`** under the game install root. Each mod is a self-contained subdirectory (for example `Modules/MyAwesomeMod/`) that contains, at minimum, a `SubModule.xml` and usually a `bin/`, `ModuleData/`, `GUI/`, or `AssetPackages/` tree.

modde deploys mods with its standard **symlink-farm VFS**: mods are staged in `~/.local/share/modde/staging/<profile>/` and then linked into `Modules/`, leaving the original game files untouched and the deployment fully reversible. Conflict resolution (later mods in the load order win) happens during the build phase. See [Deployment & VFS](../guides/deployment.md) for the full pipeline.

### Archive analysis (what modde does when you install a mod)

modde chooses the install layout from the extracted archive:

- **Module-root archives** — if the archive extracts to a directory that already contains a `SubModule.xml` at its top level, modde treats the whole archive as a single module. It uses the module's own `Id.value` attribute (from `SubModule.xml`) as the mod id when one is present.
- **`Modules/`-prefixed archives** — if the archive instead contains a top-level `Modules/` directory, modde strips that prefix and deploys its contents into the game's `Modules/` directory, so a mod packaged as `Modules/CoolMod/...` lands at `Modules/CoolMod/...`.

The same two shapes are recognized as a "bare layout" (a loose, already-extracted folder you point modde at): a directory holding a `SubModule.xml`, or a directory whose root contains a `modules/` folder and `.xml` files (the directory match is case-insensitive). This is what lets you adopt manually downloaded mods without a wrapping archive.

## What scanning finds

`modde scan --game bannerlord` walks the `Modules/` directory and reports one discovered mod per immediate subdirectory that contains a `SubModule.xml`. For each such module it:

1. Parses `SubModule.xml` for the module **id** (`<Id value="...">`) and human-readable **name** (`<Name value="...">`). If `<Id>` is missing it falls back to the directory name; if `<Name>` is missing it falls back to the id.
2. Records every file under the module directory, relative to the install root, as the module's footprint.
3. Emits a discovered mod with id `module/<id>`, the parsed display name, and a high confidence score (0.95). Modules with no files are skipped.

The scanner skips directories without `SubModule.xml`. See [Mod Scanning](../guides/scanning.md) for importing results into a profile and matching Wabbajack manifests.

### Dependency checking

modde reads every `<DependedModule Id="...">` entry in `SubModule.xml`, compares declared dependencies with discovered module ids, and reports **missing dependencies** as `(module_id, missing_dependency_id)` pairs. A mod may silently fail to load if a required module such as `Bannerlord.Harmony` or `Bannerlord.UIExtenderEx` is missing. modde reports what's missing; only the in-game Launcher controls runtime order.

## Conflict classification

Bannerlord uses the generic policy collision classifier for file-level overwrite severity, layered on top of a Bannerlord content policy that maps file types to save-breaking, cosmetic, and content categories:

| Extension | Content category | Save-breaking? |
| --------- | ---------------- | -------------- |
| `dll` | Binary | Yes |
| `xml` | Config | Yes |
| `xslt` | Config | Yes |
| `xsl` | Config | Yes |
| `pak` | Archive | Yes |
| `dds`, `png`, `tga`, `jpg` | Texture | No (cosmetic) |

The save-breaking extension set is `dll`, `xml`, `xslt`, `xsl`, `pak`; the cosmetic set is `png`, `jpg`, `dds`, `tga`. In addition, anything under a module's `bin/` directory or a `submodule.xml` file is treated as save-breaking, because those carry the compiled assemblies and the module manifest that actually change how a campaign behaves. Texture overwrites are classified as cosmetic — visible, but they will not invalidate a save.

Run `modde collisions --profile <name>` to see conflicting module pairs with severity and file counts, or add `--all` to include cosmetic overlaps. The conflict model (load-order priority, shadowed mods, redundant files, per-file hiding) is the same one described in [Conflicts & Load Order](../guides/conflicts.md); the LOOT plugin-sorting parts of that guide are Bethesda-only and do not apply to Bannerlord.

## Save tracking

Bannerlord saves are tracked from:

```
~/Documents/Mount and Blade II Bannerlord/Game Saves/Native/
```

The tracker matches **`*.sav`** files (non-recursively) in that directory and files them all under the `campaign` category. Each file's relative name is used as its label, and the capture summary is reported by category.

These saves flow into modde's standard **git-backed save vault**: a per-game repository under `~/.local/share/modde/saves/bannerlord/`, with one branch per profile. Switching profiles captures the outgoing saves and restores the incoming profile's. Every snapshot embeds a **SHA-256 fingerprint** of the profile's enabled save-breaking mods (the `dll`/`xml`/`xslt`/`xsl`/`pak` + `bin/` + `submodule.xml` set above), so that on restore modde can warn you when a snapshot was taken with a different set of save-breaking modules than your current profile:

```
Mod-Fingerprint: a1b2c3d4e5f6
Save-Breaking-Mods: module/Bannerlord.Harmony, module/CoolMod
```

Adopt pre-existing saves with `modde save adopt --game bannerlord --profile <name>`, then capture/restore/history as in [Save Management](../guides/saves.md). Because Bannerlord saves carry the active module list inside the `.sav` itself, the fingerprint warning is a strong signal: loading a campaign save against a mismatched module set is the most common cause of corrupted Bannerlord saves.

## Plugin / load-order handling

Bannerlord has no separate plugin format (no `.esp`/`.esl` equivalent); a module either loads or it does not, and order is resolved by the game's own **Launcher → Mods** screen and the `LauncherData.xml` it writes. modde does **not** ship a Bannerlord load-order manager and does not write `LauncherData.xml`. What modde provides instead is:

- **Dependency visibility** — missing-`<DependedModule>` reporting (above), so you know *which* modules must be present and roughly in what dependency relationship.
- **Deployment ordering** — modde's load-order priority decides which mod wins a file-level conflict during deployment, which is independent from the runtime module activation order TaleWorlds' Launcher controls.

After deploying with modde, enable and order your modules in the in-game Launcher (or a third-party launcher such as the Bannerlord Mod Launcher / BUTR tools) as you normally would.

## Installer specifics

Bannerlord does not use FOMOD, REDmod, `.pak` mod managers, or SMAPI. Its packaging is simpler and modde handles it directly:

- **No FOMOD wizard** — most Bannerlord mods are plain module folders. modde's two archive shapes (a top-level `SubModule.xml`, or a top-level `Modules/` prefix to strip) cover the vast majority of Nexus downloads. The FOMOD installer described in [FOMOD installer](../guides/fomod.md) is used by Bethesda-style mods and generally does not apply here.
- **`.pak` files** are Bannerlord asset packages, classified as save-breaking archives — they are content, not an installer format, and are deployed like any other module file.
- **Harmony / UIExtenderEx / ButterLib / MCM** are themselves ordinary modules: install them like any other mod and let dependency checking confirm they are present.

Install a single mod from Nexus with `modde install mod <url> --profile <name>`; the Nexus domain for Bannerlord is `mountandblade2bannerlord`, which modde uses to resolve Nexus URLs to this game.

## Gaming tools, Proton, and overlays

Bannerlord registers **no OptiScaler profile**, so it lacks the preconfigured DLSS/FSR workflow that titles like Stellar Blade ship. Use `modde tool ...` for MangoHud, vkBasalt, GameMode, ReShade, and per-game Proton settings; see [Tools & Overlays](../guides/tools.md).

The most relevant tool for a Linux Bannerlord install is the **`proton`** integration, which stores per-game launch settings (Proton version mode, prefix override, extra environment variables, and DLL overrides) without installing Steam or the game itself:

```bash
# Keep Steam's default Proton runner for Bannerlord
modde tool configure proton --game bannerlord version_mode=launcher_default

# Force a DLL override if a native-loader mod needs it
modde tool configure proton --game bannerlord dll_override_mode=forced forced_dll_overrides=dxgi
```

modde's **executable management** is fully shipped and is useful for Bannerlord's out-of-game tools: register a named executable (with arguments, working directory, environment, Wine DLL overrides, and an output-capture mod) via `modde tool add-executable ...` and run it with `modde exec ...`, capturing any files the tool writes into an overwrite mod. This is handy for running modding utilities against the deployed `Modules/` tree.

## Linux / Proton notes and known gotchas

Bannerlord runs through Proton/Wine on Linux and macOS. On Windows it runs natively without a Wine prefix and keeps `Modules/` and saves at their native Windows locations. modde runs natively on all three platforms.

- **Run the game once before modding.** Let Steam/Proton create the prefix and let Bannerlord generate `~/Documents/Mount and Blade II Bannerlord/` (Proton maps this from the prefix's `drive_c/users/steamuser/Documents`, but on a typical setup it surfaces at your real `$HOME/Documents`). modde's save tracker reads the `Game Saves/Native/` directory there.
- **Modules live host-side.** Because modde links into the real `Modules/` directory under the install root, you usually do not touch the Proton prefix to add or remove mods — but the game still needs to be launched through the same Proton runner you configured.
- **Enable modules in the Launcher after deploying.** Deployment puts the files in place; it does not toggle modules on. A freshly deployed module that is not enabled in the in-game Launcher will appear "missing" even though its files are present.
- **`.NET` module DLLs are save-breaking.** Adding or removing a `dll`-bearing module changes the save fingerprint. Expect (and heed) modde's mismatch warning when restoring an older campaign save.
- **Case sensitivity.** modde's bare-layout recognition treats the `Modules`/`modules` directory case-insensitively, which matters on Linux's case-sensitive filesystem when a mod ships an oddly-cased folder; the on-disk `Modules/` directory itself is whatever the game created.

## Worked example

### Declarative (home-manager)

Define a Bannerlord profile and wait for the game to be installed before modde deploys into it:

```nix
programs.modde.profiles.my-bannerlord = {
  game = "bannerlord";
  # Point at the real Steam install once it exists; until then modde
  # prints an awaiting message instead of failing activation.
  installMode = "await-game";
  # gameDir = "/home/you/.steam/steam/steamapps/common/Mount & Blade II Bannerlord";

  tools.proton.settings = {
    version_mode = "launcher_default";
  };
};
```

After Steam has installed the game, set `gameDir` to the install path and switch `installMode = "auto"`, then rebuild your home-manager configuration; modde deploys the profile during activation.

### Imperative (CLI)

```bash
# 1. Create a Bannerlord profile
modde profile create my-bannerlord --game bannerlord

# 2. Install a mod from Nexus into it
modde install mod \
  https://www.nexusmods.com/mountandblade2bannerlord/mods/2018 \
  --profile my-bannerlord

# 3. Import any modules already sitting in Modules/
modde scan --game bannerlord --import-to my-bannerlord

# 4. Inspect conflicts (add --all for cosmetic overlaps)
modde collisions --profile my-bannerlord

# 5. Deploy the symlink farm into the game's Modules/ directory
modde deploy --profile my-bannerlord --game bannerlord \
  --game-dir "$HOME/.steam/steam/steamapps/common/Mount & Blade II Bannerlord"

# 6. Adopt existing campaign saves into the git-backed vault
modde save adopt --game bannerlord --profile my-bannerlord
```

Then launch through Steam (Proton), enable your modules in the in-game Launcher, and order them there.

## See also

- [Supported games](supported-games.md)
- [Deployment & VFS](../guides/deployment.md)
- [Mod Scanning](../guides/scanning.md)
- [Conflicts & Load Order](../guides/conflicts.md)
- [Save Management](../guides/saves.md)
- [Tools & Overlays](../guides/tools.md)
- [Feature parity reference](../reference/parity.md)
