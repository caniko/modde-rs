# Generic & user-defined games

modde ships [15 built-in games](supported-games.md) with game-specific scanners,
save trackers, and conflict logic. For other titles, write or generate a TOML
*GameSpec* in modde's data directory. The **generic game** plugin provides
deployment, conflict detection, launcher integration, and executable management
without a hand-coded profile.

> **Generic game support is `Partial`.** A spec describes a game with paths and
> IDs. That enables virtual-filesystem deployment, conflict classification,
> launcher detection, and executable management, but not a game-specific
> filesystem scanner or a save tracker. See
> [the supported-games matrix](supported-games.md) for the status vocabulary
> and `docs/capability-matrix.toml` for the baseline.

## What a user-defined game can and cannot do

modde registers user-defined games as `EngineFamily::Generic` and skips steps
that need game-specific logic.

| Capability | Generic game | Notes |
| ---------- | ------------ | ----- |
| Mod deployment (virtual filesystem / overlay) | **Yes** | Mods deploy into the configured `mod_dir` (defaults to the install root). |
| Conflict detection & classification | **Yes** | Uses the built-in `generic_collision_classifier`. |
| Profiles | **Yes** | Standard per-game profiles work. |
| Install detection | **Yes** | Via `install_path_override`, Steam `install_dir_name`, or Steam/Heroic launcher data. |
| Launcher integration | **Yes** | `steam_app_id` lets modde resolve the Steam launch target. |
| Executable management (`modde exec` / `modde tool add-executable`) | **Yes** | Named launch targets with args, working dir, env, Wine DLL overrides, and overwrite capture work the same as for built-in games. See the [CLI reference](../reference/cli.md). |
| Wine / Proxy DLL overrides | **Yes** | `proxy_dlls` are emitted as Wine DLL overrides, but only for DLLs that actually exist in the executable directory. |
| Nexus metadata side panel | **Partial** | Set `nexus_domain` to enable Nexus browse/search and the metadata side panel; the richer mod-information dialog is not implemented for any game. |
| **Bespoke filesystem scanner** | **No** | `scanner: None`. `modde scan` has no game-specific heuristics to discover already-installed mods for a generic game. |
| **Save tracking / save profiles** | **No** | `save_tracker: None` and `supports_save_profiles: false`. `modde save` save-profile features are unavailable. |
| Wabbajack list matching | **No** | Generic games carry no `wabbajack_names`. |
| OptiScaler profiles | **Opt-in** | None ship by default, but you can import them — see [import-profile](#modde-game-import-profile). |

The lack of a game-specific scanner and save tracker defines `Partial` here.

## Where specs live on disk

User-defined game specs are plain TOML files in modde's data directory:

```
<data dir>/modde/games/<id>.toml
```

`<data dir>` resolves per-platform:

- **Linux**: `$XDG_DATA_HOME/modde/games/` or `~/.local/share/modde/games/`
- Overridable by the active [instance](../reference/cli.md) data directory.

Each game is one file named after its `id` (for example `factorio.toml`). The
loader reads this directory at startup:

- only files ending in `.toml` are considered;
- files ending in `.optiscaler.toml` are skipped (those are OptiScaler profile
  sidecars, not game specs);
- specs are loaded in sorted filename order;
- each spec is validated, and any spec whose `id` collides with a built-in game
  or a previously-loaded user game is skipped with a warning;
- invalid or unparseable specs are skipped (with a warning) rather than aborting
  the load.

You can edit specs by hand, but prefer `modde game`: it validates before writing.

## The GameSpec TOML schema

A GameSpec is a flat TOML table. Only three fields are required.

```toml
# games/<id>.toml
id = "factorio"                 # required
display_name = "Factorio"       # required
executable_dir = "."            # required (relative to install root)

# All of the following are optional:
steam_app_id = "427520"         # Steam AppID, as a string
install_dir_name = "Factorio"   # steamapps/common/<name>
install_path_override = "/games/factorio"  # absolute path; bypasses detection
mod_dir = "mods"                # where mods deploy, relative to install root
nexus_domain = "factorio"       # Nexus game domain for browse/search
proxy_dlls = ["dxgi", "winmm"]  # candidate Wine DLL overrides
```

| Field | Type | Required | Meaning |
| ----- | ---- | -------- | ------- |
| `id` | string | **yes** | Stable game ID. Must match `^[a-z0-9][a-z0-9-]*$` and not collide with a built-in game ID. This is also the filename stem. |
| `display_name` | string | **yes** | Human-readable name. Must be non-empty (after trimming). |
| `executable_dir` | path | **yes** | Directory containing the game executable(s), **relative to the install root**. Use `"."` if the executable sits at the root. Must be relative and must not contain `..` or escape the root. |
| `steam_app_id` | string | no | Steam AppID (stored as a string; parsed to `u32` when a launch target is resolved). Enables Steam launcher integration. |
| `install_dir_name` | string | no | Folder name under `steamapps/common/`. modde scans every known Steam library for this folder when detecting the install. |
| `install_path_override` | path | no | Absolute path to the install. When set and the directory exists, it short-circuits all other detection. |
| `mod_dir` | path | no | Directory mods deploy into, relative to the install root. Must be relative and must not escape the root. **Defaults to the install root itself** when omitted. |
| `nexus_domain` | string | no | Nexus game domain (the slug in a Nexus URL). Enables Nexus browse/search and the metadata side panel for this game. |
| `proxy_dlls` | array of strings | no | DLL base names (without `.dll`) that modde *may* register as Wine DLL overrides. At launch, modde only emits an override for a name if `<executable_dir>/<name>.dll` actually exists, so listing a DLL that is not present is harmless. Defaults to empty. |

### Validation rules

The CLI and loader validate specs against the same rules:

- `id` must match `^[a-z0-9][a-z0-9-]*$` (lowercase ASCII letters/digits and
  hyphens, starting with a letter or digit).
- `id` must not collide with a built-in game ID.
- `executable_dir` and `mod_dir` (if present) must be **relative** and must not
  contain `..`, a root component, or a Windows drive prefix — they cannot escape
  the install root.
- `display_name` must not be empty after trimming whitespace.

`modde game add` / `modde game import` reject invalid specs. The loader warns
and skips them.

### Install detection order

modde looks for the install directory in this order:

1. `install_path_override`, if set and the directory exists.
2. `install_dir_name` under any detected `steamapps/common/` library.
3. modde's generic launcher detection (Steam/Heroic) keyed on the game ID.

The first hit wins.

## The `modde game` CLI surface

Manage user-defined games with `modde game`:

| Subcommand | Purpose |
| ---------- | ------- |
| `add` | Create or overwrite a user-defined game spec. |
| `list` | List configured user-defined games. |
| `remove` | Delete a user-defined game spec (with confirmation). |
| `show` | Show a resolved game registration (user-defined *or* built-in). |
| `detect` | Find executable-bearing directories under an install path. |
| `export` | Serialize a game registration (any game) to a GameSpec TOML. |
| `import` | Install a GameSpec TOML from a file into the games directory. |
| `import-profile` | Install OptiScaler profiles from a TOML file for a game. |

### `modde game add`

```bash
modde game add <id> \
  --display-name <name> \
  --executable-dir <relative-path> \
  [--steam-app-id <id>] \
  [--install-dir-name <name>] \
  [--mod-dir <relative-path>] \
  [--nexus-domain <domain>] \
  [--proxy-dll <name>]... \
  [--force]
```

| Flag / arg | Required | Maps to GameSpec field |
| ---------- | -------- | ---------------------- |
| `<id>` (positional) | yes | `id` |
| `--display-name <name>` | yes | `display_name` |
| `--executable-dir <path>` | yes | `executable_dir` |
| `--steam-app-id <id>` | no | `steam_app_id` |
| `--install-dir-name <name>` | no | `install_dir_name` |
| `--mod-dir <path>` | no | `mod_dir` |
| `--nexus-domain <domain>` | no | `nexus_domain` |
| `--proxy-dll <name>` | no | one entry in `proxy_dlls`; repeat the flag to add more |
| `--force` | no | overwrite an existing `<id>.toml` instead of erroring |

`add` writes (or overwrites, with `--force`) `<data dir>/modde/games/<id>.toml`.
`install_path_override` is **not** settable from `add` — it is always written as
unset; use `install_dir_name`/`steam_app_id` for detection, or edit the TOML by
hand (or `import` a spec that contains it) if you need an absolute override.
Re-running `add` for an existing id without `--force` fails and tells you to
re-run with `--force`. On success it prints whether it `Saved` or `Overwrote`
the spec and the path it wrote.

### `modde game list`

```bash
modde game list
```

Prints the user-defined games as a table: `id | display name | executable_dir |
source`, where *source* is the on-disk path of each spec. Prints
`No user-defined games configured.` when the games directory is empty or absent.
Built-in games are **not** listed here.

### `modde game show`

```bash
modde game show <id>
```

Shows a resolved registration. If `<id>` is a user-defined game it prints the
full spec (display name, executable_dir, mod_dir, steam_app_id,
install_dir_name, nexus_domain, proxy_dlls, and the source path). If `<id>` is a
**built-in** game it prints the built-in summary (display name, steam_app_id,
nexus_domain, and `source: built-in registry`). Unknown IDs error with the list
of supported game IDs.

### `modde game remove`

```bash
modde game remove <id> [--yes]
```

Deletes `<data dir>/modde/games/<id>.toml`. By default it prompts
`Remove user-defined game '<id>'? [y/N]`; pass `--yes` to skip the prompt.
Attempting to remove a built-in game errors with
`game '<id>' is built in and cannot be removed`; removing an unknown id errors
with `no user-defined game named '<id>' exists`.

### `modde game detect`

```bash
modde game detect <install-path>
```

Searches `<install-path>` for directories containing `.exe` files, so you can
confirm the install layout and choose an `executable_dir`. For each match, it
reports the relative path, sorted `.exe` names, and total executable size in
bytes.

- the search recurses **up to 4 directory levels deep** below the install root;
- `.exe` matching is case-insensitive;
- results are sorted by total executable size **descending**, then by relative
  path — so the largest-executable directory (usually the real game binary) is
  listed first;
- a non-existent install path errors;
- when nothing is found it prints
  `No executable-bearing directories found under <path>.`

`detect` never writes a spec. Confirm which candidate directory contains the
real game binary before passing it as `--executable-dir`.

### `modde game export`

```bash
modde game export <id> [--with-optiscaler] [--output <path>]
```

Serializes *any* resolvable game (built-in or user-defined) to a GameSpec TOML.
Exporting a built-in game gives you a starting point you can rename and tweak: it
carries the built-in `display_name`, `steam_app_id`, `install_dir_name`, and
`nexus_domain`, with `executable_dir = "."` and no `mod_dir`/`proxy_dlls`. With
`--with-optiscaler` the exported TOML also includes the game's resolved
OptiScaler profiles (appended after the spec). With `--output <path>` it writes
to that file; otherwise it prints to stdout.

### `modde game import`

```bash
modde game import <path> [--force]
```

Reads a GameSpec TOML from `<path>`, parses and validates it, then copies it to
`<data dir>/modde/games/<spec-id>.toml`. The destination filename comes from the
spec's `id`, **not** from the source filename. Without `--force`, an existing
destination errors; with `--force` it overwrites. This is how you install a spec
someone shared with you.

### `modde game import-profile`

```bash
modde game import-profile <path> --for <game-id> [--force]
```

Imports OptiScaler profiles (not a game spec) from `<path>` and writes them to
the sidecar `<data dir>/modde/games/<game-id>.optiscaler.toml`. The `--for`
flag names the game the profiles belong to. Importing for an unknown game id
warns but proceeds. Without `--force`, an existing sidecar errors; with `--force`
it overwrites. On success it reports how many profiles were imported and where.

## Detecting executable-bearing directories

Some games put a tiny launcher at the root and the game binary several folders
deep. Before registering the game, use `modde game detect` to list candidate
executable directories and confirm which contains the game binary:

```bash
modde game detect "/home/you/Games/factorio"
```

```text
Executable-bearing directories under /home/you/Games/factorio:
1. bin/x64
   executables: factorio.exe
   total exe size: 41203712 bytes
2. .
   executables: launcher.exe
   total exe size: 524288 bytes
```

The first entry has the largest executables, so `bin/x64` is almost certainly
the `executable_dir` you want. Feed it straight into `modde game add
--executable-dir bin/x64`.

## Sharing a spec

GameSpecs are designed to be portable text files:

```bash
# On the authoring machine: write the spec to a file you can commit/share.
modde game export factorio --output factorio.toml

# Or hand-author games/factorio.toml and copy it out of the games directory.
```

```bash
# On another machine: install it (filename is taken from the spec id).
modde game import factorio.toml
```

Because the destination filename is derived from the spec's `id`, the source file
can be named anything. `import` re-validates the spec before installing it, so a
malformed or id-colliding spec is rejected up front rather than being silently
dropped at load time. Keep specs in version control or share them in issues — they
are small and contain no secrets.

## Worked example: registering a new title

Suppose you want to manage mods for a Steam game called *Voidrunner* that modde
does not ship a profile for. Its Steam AppID is `998877`, it installs to
`steamapps/common/Voidrunner`, the real binary lives in `Binaries/Win64`, and
mods drop into a top-level `Mods/` folder. It uses a `dxgi.dll` proxy for an
upscaler, and there is a Nexus page under the `voidrunner` domain.

**1. Find the executable directory.**

```bash
modde game detect "$HOME/.local/share/Steam/steamapps/common/Voidrunner"
```

```text
Executable-bearing directories under .../Voidrunner:
1. Binaries/Win64
   executables: Voidrunner-Win64-Shipping.exe
   total exe size: 78905344 bytes
2. .
   executables: VoidrunnerLauncher.exe
   total exe size: 1048576 bytes
```

`Binaries/Win64` is the winner.

**2. Register the game.**

```bash
modde game add voidrunner \
  --display-name "Voidrunner" \
  --executable-dir "Binaries/Win64" \
  --steam-app-id 998877 \
  --install-dir-name "Voidrunner" \
  --mod-dir "Mods" \
  --nexus-domain voidrunner \
  --proxy-dll dxgi
```

```text
Saved user-defined game 'voidrunner' at /home/you/.local/share/modde/games/voidrunner.toml
```

This writes:

```toml
# /home/you/.local/share/modde/games/voidrunner.toml
id = "voidrunner"
display_name = "Voidrunner"
steam_app_id = "998877"
install_dir_name = "Voidrunner"
mod_dir = "Mods"
executable_dir = "Binaries/Win64"
nexus_domain = "voidrunner"
proxy_dlls = ["dxgi"]
```

**3. Confirm it.**

```bash
modde game list
modde game show voidrunner
```

**4. Use it like any other game.** From here, `--game voidrunner` works across
the CLI: create a profile, install mods, deploy, and register a launch target.

```bash
modde profile create main --game voidrunner
# install mods into the profile (nexus / mediafire / local files) ...
modde deploy --game voidrunner

# Register a named executable launch target (see the CLI reference):
modde exec add voidrunner-game "Binaries/Win64/Voidrunner-Win64-Shipping.exe" \
  --game voidrunner
modde play main --game voidrunner
```

modde will detect the install (via the override-free detection chain:
`install_dir_name` under Steam, then launcher data), deploy mods into
`Mods/`, classify conflicts, and — because `dxgi.dll` exists in
`Binaries/Win64` after you deploy the upscaler — register the `dxgi` Wine DLL
override at launch.

**What you do *not* get:** `modde scan --game voidrunner` has no Voidrunner-aware
heuristics to discover pre-existing mods, and `modde save` save-profile features
are unavailable, because generic games ship neither a scanner nor a save tracker.
That is the boundary of the `Partial` status.

## See also

- [Supported games](supported-games.md)
- [CLI reference](../reference/cli.md)
- [Architecture](../reference/architecture.md)
