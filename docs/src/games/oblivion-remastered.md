# The Elder Scrolls IV: Oblivion Remastered

Oblivion Remastered pairs an Unreal Engine 5 presentation layer with the
original Gamebryo simulation. The modde plugin for game id `oblivion-remastered`
recognises both the UE pak `~mods` layout (`.pak` / `.ucas` / `.utoc`) and
Bethesda-style `.esp` / `.esm` plugins. It scans the UE pak directory and the
`Data/` plugin tree.

> **Overall status: `Partial`.** Deployment, scanning, conflict classification,
> Wine DLL-override detection, and save tracking work. The install pipeline
> integrates Steam/Heroic detection, Nexus, and Wabbajack. Like other UE titles,
> it falls short of `Done`: no dedicated ESP/ESM load-order editor, no
> pak-internal asset conflict resolution (modde compares filenames, not archive
> contents), and less complete install-method coverage than mature Creation Engine games.
> See [Supported games](supported-games.md) for the
> status baseline and [Parity](../reference/parity.md) for the
> MO2/Vortex/Wabbajack comparison.

## Engine and registration

| Field | Value |
| ----- | ----- |
| Display name | The Elder Scrolls IV: Oblivion Remastered |
| Game id | `oblivion-remastered` |
| Engine family | `Unreal4` (UE5 layout; shares modde's UE pak handling) |
| Project folder | `OblivionRemastered/` (under the install root) |
| Steam App ID | `2623190` |
| Steam install dir | `Oblivion Remastered` |
| Heroic (GOG/Epic) | not registered |
| Nexus domain | `oblivionremastered` |
| Wabbajack name | `OblivionRemastered` → `oblivion-remastered` |
| Save profiles | enabled |
| Plugin system | yes (ESP/ESM are recognised alongside paks) |

Instead of the shared UE4 game type, a dedicated `GamePlugin` supports
Bethesda plugin extensions, Gamebryo-style saves, and the blended content policy.

## Install detection

modde finds the install through the launcher integration, keyed on the Steam
App ID `2623190` (Steam install dir `Oblivion Remastered`). There is no
registered Heroic/GOG/Epic mapping, so detection is Steam-first; a manually
specified install path also works for non-Steam copies.

On Linux and macOS the game runs through Proton/Wine, so modde resolves the
prefix-rooted paths (saves, and the Wine DLL overrides described below) under the
Steam compat data tree (on Windows these are native paths, no prefix involved):

```
<steam>/steamapps/compatdata/2623190/pfx/drive_c/...
```

Before the first launch, the prefix does not exist and prefix-dependent
lookups return "not found". Launch once through Steam/Proton to create the
prefix, then re-run.

## Mod directory and deploy strategy

The deploy root for pak-style mods is the UE `~mods` directory under the
project's `Content/Paks`:

```
<install>/OblivionRemastered/Content/Paks/~mods
```

The leading tilde matters: UE sorts `~mods` **after** the shipping paks, so mod
paks override base content. As with other games, modde links the active
profile's enabled mods into the live game tree and re-links them on profile
switch. Plugin content (`.esp` / `.esm`) belongs in the game's `Data/` tree,
which modde also scans.

## What scanning finds

The scanner inspects two locations:

```
Content/Paks/~mods     # UE pak mods
Data                   # Bethesda-style plugins
```

- **Pak mods** are discovered by grouping files that share a stem across the
  `.pak` / `.ucas` / `.utoc` extensions into a single mod (so a `.pak` and its
  sidecar `.ucas`/`.utoc` count as one mod, not three). These come from the
  `paks-mods` source location at confidence `0.9`, with mod ids prefixed `pak/`.
- **Plugins** are discovered as one mod per top-level `.esp` file directly in
  `Data/` (source location `Data`, confidence `0.8`, mod ids prefixed
  `plugin/`). No prefixes are ignored, so master files are listed too.

Each discovered mod also has a footprint used for matching against a known mod:

| Mod id form | Footprint path |
| ----------- | -------------- |
| `pak/<stem>` | `oblivionremastered/content/paks/~mods/<stem>.pak` |
| `plugin/<stem>` | `data/<stem>.esp` |

## Conflict classification

Oblivion Remastered uses modde's **pak collision classifier** (archive
extensions `pak` / `ucas` / `utoc`, default severity table). When two enabled
mods write the same relative path, the severity is decided by file type:

| Extension(s) | Severity |
| ------------ | -------- |
| `esp`, `esm`, `dll`, `lua`, `ws` | Dangerous |
| `pak`, `ucas`, `utoc` (as archive members) | Dangerous |
| `ini`, `json`, `xml` | Config |
| `dds`, `png`, `jpg`, `tga`, `nif` | Cosmetic |

Conflicts are detected by overlapping deployed paths, not by cracking open pak
archives — two paks that touch different in-game assets but never collide on a
file path are not flagged. Texture/mesh collisions are reported but treated as
cosmetic (last-writer-wins is usually fine); plugin and pak collisions are
flagged as dangerous because they can change game logic or load behaviour.
See [Conflict detection](../guides/conflicts.md) for how severities drive the
UI and resolution.

### Save-breaking classification

Separately from path collisions, modde classifies each mod's *content* to
decide whether enabling or disabling it should invalidate save compatibility.
For Oblivion Remastered:

- **Save-breaking extensions**: `esp`, `esm`, `pak`, `ucas`, `utoc`, `dll`,
  `lua`
- **Save-breaking directories**: `plugins`, `logicmods`
- **Cosmetic extensions**: `dds`, `png`, `jpg`, `tga`, `nif`

This classification feeds the save-fingerprint logic below: only save-breaking
mods contribute to the fingerprint, so swapping a texture pack will not trigger
a false "incompatible save" warning, while toggling a plugin or a logic pak
will.

## Save tracking

Saves live under the Proton prefix, in the remastered Documents tree:

```
<steam>/steamapps/compatdata/2623190/pfx/drive_c/users/steamuser/Documents/My Games/Oblivion Remastered/Saves
```

The tracker matches `.ess` and `.sav` in that directory without recursing and
skips `*.bak`. It records each save's path, `manual` category, relative filename
as its label, and last-modified time. It does **not** hash individual saves.

Save *fingerprinting* happens at the vault layer, and it fingerprints your
**mod set**, not the save bytes: modde computes a SHA-256 over the sorted list
of enabled save-breaking mods (per the classification above) and embeds it as a
git commit trailer in each snapshot:

```
Mod-Fingerprint: a1b2c3d4e5f6
Save-Breaking-Mods: pak/some-overhaul, plugin/somequest
```

On restore, modde compares the snapshot's fingerprint to your current profile
and warns if save-breaking mods were added or removed since that save was
taken. Oblivion Remastered opts into per-profile save layering, so each
profile gets its own branch in the git-backed vault and profile switches
capture/restore automatically. See [Save management](../guides/saves.md) for
the full vault, fingerprint, and restore workflow.

## Plugins and load order

modde recognises the ESP/ESM plugin system (it is reported as a plugin-capable
game) and scans `Data/` for plugins, so plugin mods are discovered, classified
as save-breaking, and conflict-checked by path. There is **no** dedicated
load-order editor for the Bethesda side of this title in modde today — load
order for plugins is managed by the game/community tooling you already use, and
pak ordering is governed by UE's `~mods` mounting. Treat plugin ordering as
out-of-band for now.

## Installer specifics

When you add a downloaded archive, modde's analyzer picks the layout:

| Archive shape | Detected method |
| ------------- | --------------- |
| `.pak` / `.ucas` / `.utoc` files at the archive root | `SingleFileSet` — files stage straight into the resolved mod root |
| A top-level `Data/` directory | `StripContentRoot { root: "Data" }` — the `Data/` wrapper is stripped before staging |

modde also recognises a **bare** archive layout for this game: an archive whose
root contains any of the directories `data`, `mods`, `content`, `paks`, or
`~mods` (matched case-insensitively), or root files with the extensions `esp`,
`esm`, `pak`, `ucas`, or `utoc`. That recognition lets it accept the loosely
packed archives common on Nexus for this title without a wrapper directory.

FOMOD installers are handled by modde's generic FOMOD pipeline when an archive
ships a `fomod/ModuleConfig.xml`; that path is engine-agnostic and not specific
to this game. There is no REDmod or SMAPI path here — those belong to Cyberpunk
2077 and Stardew Valley respectively. See [FOMOD installers](../guides/fomod.md)
and the [installer pipeline](../guides/deployment.md) for the staging and
uninstall model.

## Gaming tools that matter here

Mod loaders, ReShade/ENB-style injectors, and upscalers for this title ship as
**proxy DLLs** dropped next to the shipping executable in:

```
<install>/OblivionRemastered/Binaries/Win64
```

modde detects which proxy DLLs are present from this set and surfaces them as
Wine DLL overrides (see below):

```
dwmapi   xinput1_3   d3d11   dxgi   version   winmm
```

That covers the usual suspects — generic ASI/loader proxies (`version`,
`winmm`), UE-loader proxies (`dwmapi`, `xinput1_3`), and the graphics-swapper
slots (`d3d11`, `dxgi`) used by ReShade and DLSS/FSR swappers such as
**OptiScaler**.

This game does not ship a curated OptiScaler profile in modde (unlike Stellar
Blade), so OptiScaler is installed and tuned manually as a `dxgi`/`d3d11` proxy.
The named-executable and proxy-DLL machinery is fully supported regardless — see
[Tools and executables](../guides/tools.md) for adding a launcher executable
with arguments, working dir, env, and DLL overrides, and capturing its output.

## Linux and Proton notes

Oblivion Remastered runs through Proton/Wine on Linux and macOS. On Windows it
runs natively: no Wine prefix or `WINEDLLOVERRIDES`, saves use the native
Windows path, and proxy DLLs load directly. modde runs natively on all three
platforms.

- **Run the game once first.** The save directory and the proxy-DLL overrides
  are resolved relative to the Proton prefix at `compatdata/2623190/pfx`. If you
  have not launched the game, that prefix does not exist and prefix-rooted
  lookups will report "not found".
- **DLL overrides are automatic from what is present.** modde builds
  `WINEDLLOVERRIDES` from the proxy DLLs it finds in `Binaries/Win64` (or in a
  mod's staging dir), so Wine loads the native (mod) DLL instead of its built-in
  stub. You do not hand-write the override string.
- **`~mods` ordering is a UE convention, not a modde trick.** If a pak mod is
  not taking effect, confirm it landed in `Content/Paks/~mods` and not loose in
  `Content/Paks`.
- **Texture/mesh collisions are last-writer-wins.** They are reported as
  cosmetic; if a visual mod loses to another, adjust which one deploys, since
  modde does not merge pak contents.

## Worked example

### CLI

```bash
# Detect installs and confirm the game is found
modde game list

# Install a Nexus pak mod by URL (domain resolves to oblivion-remastered)
modde mod install "https://www.nexusmods.com/oblivionremastered/mods/1234"

# Scan the live install for already-present mods (paks + Data plugins)
modde mod scan --game oblivion-remastered

# Inspect conflicts for the active profile
modde conflicts --game oblivion-remastered

# Adopt existing saves into a profile, then capture
modde save adopt --game oblivion-remastered --profile main
modde save capture --game oblivion-remastered --profile main -m "fresh start"

# Add OptiScaler (or any injector) as a dxgi proxy executable launcher
modde tool add-executable \
  --game oblivion-remastered \
  --name "Oblivion Remastered (modded)" \
  --working-dir "OblivionRemastered/Binaries/Win64"
modde exec run "Oblivion Remastered (modded)" --game oblivion-remastered
```

### home-manager profile snippet

A minimal modde profile for this title via the home-manager module:

```nix
{
  programs.modde = {
    enable = true;

    profiles.oblivion-remastered = {
      game = "oblivion-remastered";
      mods = [
        # Nexus URLs or local archive paths; paks land in
        # OblivionRemastered/Content/Paks/~mods, plugins in Data/.
        "https://www.nexusmods.com/oblivionremastered/mods/1234"
      ];
    };
  };
}
```

modde installs the same way for every game — through your platform's native
package manager, a direct download, Cargo, or, if you use Nix, the flake and its
home-manager module. The home-manager module additionally lets you declare this
profile as code. See [Installation](../getting-started/installation.md) for the
full set of install channels.

## See also

- [Supported games](supported-games.md)
- [Oblivion](oblivion.md) (the Gamebryo original) and the sibling UE titles in
  the per-game list
- [Conflict detection](../guides/conflicts.md)
- [Save management](../guides/saves.md)
- [Deployment and the installer pipeline](../guides/deployment.md)
- [FOMOD installers](../guides/fomod.md)
- [Tools and executables](../guides/tools.md)
- [Scanning](../guides/scanning.md)
- [Parity reference](../reference/parity.md)
