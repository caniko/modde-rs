# Executables & external tools

## Overview

Save modding tools such as xEdit, BodySlide, Nemesis, FNIS, the Creation Kit, or LOOT as **named executable launch targets** for a game install. Each target stores a name, executable path, default arguments, working directory, environment variables, optional Wine DLL overrides, and an output mod. modde snapshots the mod directory before running the tool, captures files newly written to the game's mod directory, and moves them into the output mod so they survive future deploys.

Executables are marked **Done** in the [capability matrix](../reference/parity.md): they work in both the CLI and GUI. [Tools & Overlays](tools.md) instead manages a fixed set of six gaming overlays (MangoHud, OptiScaler, Proton, …).
Named executables also differ from [Patcher pipelines](patcher-pipelines.md): executables
are user-invoked tools with overwrite capture, while patcher stages are
profile-scoped deployment steps that run automatically during `modde deploy`.

modde stores targets in the `executable_configs` database table, keyed by `(game_id, name)`. Saving an existing name for the same game overwrites that target, so **add doubles as edit**.

## Two equivalent command surfaces

The same storage is exposed through two CLI namespaces. `modde exec ...` is a thin alias for the executable subset of `modde tool ...`; `modde exec list` and `modde tool list-executables` print the same rows.

| Short form (`modde exec`) | Long form (`modde tool`) | Action |
| --- | --- | --- |
| `modde exec add` | `modde tool add-executable` | Save or update a named target |
| `modde exec list` | `modde tool list-executables` | List configured targets for a game |
| `modde exec remove` | `modde tool remove-executable` | Delete a target |
| `modde exec run` | `modde tool run-executable` | Run a saved target with overwrite capture |

Use `modde tool run <executable> [-- args]` to run any executable with overwrite capture **without** saving a configuration.

## Fields

Every saved target has the following fields. On the CLI they map to positional arguments and flags on `modde exec add` / `modde tool add-executable`:

| Field | CLI | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| Name | positional `<name>` | yes | — | Display name, e.g. `xEdit`. Unique per game. Cannot be blank. |
| Executable | positional `<executable>` | yes | — | Path to the program to run |
| Game | `--game <id>` | yes | — | Must be a supported game id |
| Working directory | `--working-dir <dir>` | no | detected game install dir | The cwd the tool runs in |
| Output mod | `--output-mod <name>` | no | `__overwrite__` | Mod that captures newly written files. Cannot be blank. |
| Wine DLL overrides | `--wine-dll-overrides <str>` | no | none | Exported as `WINEDLLOVERRIDES`, e.g. `dinput8=n,b;winmm=n,b` |
| Environment | `--env KEY=VALUE` (repeatable) | no | none | Each must be `KEY=VALUE` with a non-empty key |
| Default args | `-- ARGS...` | no | none | Everything after `--` is the tool's default argument list |

`add-executable` validates eagerly: an unsupported game id, a blank name, a blank output mod, or a malformed `--env` entry (missing `=` or empty key) is rejected before anything is written.

## Adding a target

```bash
modde tool add-executable xEdit /games/skyrim/SSEEdit.exe \
  --game skyrim-se \
  -- -quickautoclean
```

The equivalent short form:

```bash
modde exec add xEdit /games/skyrim/SSEEdit.exe --game skyrim-se -- -quickautoclean
```

Both store the same row. The arguments after `--` become the tool's defaults. When you run the target, add arguments after another `--` to append them to those defaults.

## Listing and removing

```bash
modde exec list --game skyrim-se
# Executables for skyrim-se:
#   xEdit
#     path: /games/skyrim/SSEEdit.exe
#     args: -quickautoclean
#     output mod: __overwrite__

modde exec remove xEdit --game skyrim-se
```

`remove` reports an error if no target with that name exists for the game.

## Running with overwrite capture

When you run a saved target (or `modde tool run <exe>` directly), modde performs the same four-step capture used for any external tool:

1. **Snapshot** the game's mod directory (the deployed VFS tree) before the tool runs.
2. **Run** the tool with the configured working directory (defaulting to the detected install dir), saved `--env` variables, `WINEDLLOVERRIDES` (if set), and the saved arguments plus any you appended.
3. **Snapshot again** after the tool exits and diff against the before-snapshot.
4. **Move** every newly written file into the output mod under modde's store directory (`~/.local/share/modde/store/<output_mod>/`).

```bash
# Run the saved xEdit target, appending an extra flag this time
modde exec run xEdit --game skyrim-se -- -autoload

# Run against a specific profile's deployed tree
modde exec run xEdit --game skyrim-se --profile my-skyrim
```

If the tool exits non-zero, modde warns but still captures whatever was written. If nothing new appeared, modde prints `No new files written to mod directory.` and stops. Otherwise it lists each captured file and reminds you to add the output mod to your profile:

```
Tool wrote 3 new file(s). Moving to output mod '__overwrite__':
  meshes/actors/character/...
  ...
Output mod: ~/.local/share/modde/store/__overwrite__
Add '__overwrite__' to your profile mod list to include these files in future deploys.
```

### What lands in the output mod

modde captures only files that are **new** since the pre-run snapshot. Existing files modified in place stay in the deployed tree with their owning mod. modde moves new files into the output mod (renaming them or copying then removing them across filesystems). You can enable, disable, and order that mod like any other. The default `__overwrite__` mod works like Mod Organizer 2's "Overwrite" pseudo-mod. Use `--output-mod` to separate BodySlide output from xEdit output, for example.

## The GUI Executables view

The **Executables** view manages the same database rows. Each row shows the target name, executable path, and a summary of its arguments, working directory, Wine DLL overrides, and output mod. Each has **Run**, **Edit**, and **Remove** buttons; Run and Remove are disabled while that target runs. **Refresh** reloads the list. **Add executable** opens a form for name, arguments, output mod, executable path (**Browse**), working directory (**Browse**), `WINEDLLOVERRIDES`, and `KEY=VALUE` env lines. "Save / update" updates an existing name in place.

## Worked examples (Proton)

Windows modding tools run under Proton/Wine on Linux. The patterns below assume the tool's `.exe` lives in or near the game install and that you have a usable Wine/Proton runner. Use `--wine-dll-overrides` when a tool needs native DLLs, `--working-dir` when it must run from a specific folder, and `--output-mod` to keep generated assets out of the catch-all overwrite.

### xEdit (SSEEdit) — quick auto clean

```bash
modde tool add-executable xEdit /games/skyrim/SSEEdit.exe \
  --game skyrim-se \
  --output-mod xedit-output \
  -- -quickautoclean

modde exec run xEdit --game skyrim-se
```

xEdit writes cleaned plugins and backups; capturing them into `xedit-output` lets you review and order them as a dedicated mod.

### BodySlide — built meshes to a dedicated mod

```bash
modde tool add-executable BodySlide "/games/skyrim/Data/CalienteTools/BodySlide/BodySlide x64.exe" \
  --game skyrim-se \
  --working-dir "/games/skyrim/Data/CalienteTools/BodySlide" \
  --output-mod bodyslide-output \
  --wine-dll-overrides "dinput8=n,b"

modde exec run BodySlide --game skyrim-se
```

BodySlide builds meshes into the game's `Data` tree; the new `.nif`/`.tri` files are captured into `bodyslide-output`, which you then place after the body/armor mods in your load order.

### Nemesis — generated behavior files

```bash
modde tool add-executable Nemesis \
  "/games/skyrim/Data/Nemesis_Engine/Nemesis Unlimited Behavior Engine.exe" \
  --game skyrim-se \
  --working-dir "/games/skyrim/Data/Nemesis_Engine" \
  --output-mod nemesis-output \
  --wine-dll-overrides "vcruntime140=n,b"

modde exec run Nemesis --game skyrim-se
```

Nemesis regenerates behavior files under `meshes/`; capturing them into `nemesis-output` keeps the generated animation data as a single, re-buildable mod that wins over the source animation mods.

## See also

- [Tools & Overlays](tools.md) — the separate gaming-overlay manager (MangoHud, OptiScaler, Proton)
- [Deployment & VFS](deployment.md) — how the mod directory you capture from is built
- [Profiles](profiles.md) — enabling and ordering the captured output mod
- [Conflict resolution](conflicts.md) — where the output mod sits in priority
- [CLI reference](../reference/cli.md) — full command and flag listing
