# Playing a Game

## Library landing page

The GUI opens on **Library**. Its default order is favorites first, then games
with a modde profile or manager instance, then other games. Search matches names,
stores and installation paths. The category filter includes installed,
uninstalled, favorites and modde-managed games.

Select an installed game and choose **Play**. **Launch settings** saves an
executable, runner, prefix, arguments, wrappers, environment, working directory,
mod profile, save directory and sandbox permissions for that installation. File
and folder pickers fill the path fields; JSON import/export transfers a launch
configuration. Arguments and wrappers are arrays, so spaces and shell punctuation
are passed literally. Save before Play; a failed save does not apply the draft.

A configured directory is not enough to launch a local game: choose its
executable. Games do not need a mod plugin to appear in Library or run. A mod
plugin is needed for profile deployment and profile-managed saves.

This remains a **Partial, unqualified** capability. The implementation includes
process supervision and store/manager integration, but compilation, regression
execution, live provider compatibility, containment and game-performance
measurements have not yet been completed for this change.

## Owned games and provider coverage

Ownership and installation are separate. An owned, uninstalled game has
**Install via Steam** or **Install via GOG/Epic**, rather than Play. Installation
is handed to Steam or Heroic. Library rescans while idle, or use **Refresh** after
installation completes.

| Provider        | Ownership source                                      | Local installation source                                                |
| --------------- | ----------------------------------------------------- | ------------------------------------------------------------------------ |
| Steam           | Explicit `GetOwnedGames` sync, cached for offline use | All local app manifests, including games outside modde's plugin registry |
| GOG             | Heroic `store_cache/gog_library.json`                 | Heroic GOG installed catalogue                                           |
| Epic            | Heroic `store_cache/legendary_library.json`           | Legendary installed catalogue, plus older Heroic installed catalogue     |
| Heroic sideload | Locally registered applications                       | Heroic sideload installed catalogue                                      |
| Local           | User-configured games                                 | Configured paths                                                         |

For Steam, set `MODDE_STEAM_API_KEY` in the environment used to start modde, enter
your 17-digit SteamID64 in Library, and choose **Sync owned games**. The CLI is:

```sh
modde library sync-steam YOUR_STEAMID64
modde library list
modde library list --json
```

The key is sent only to Steam and is not saved in library preferences. Account
visibility and Steam's API determine which games the response includes. A
private/unavailable or incomplete response does not replace the previous cache.
The account ID and last successful sync time are retained. Refresh rescans disk;
it does not implicitly perform an account sync.

For GOG/Epic, sign in and refresh the library in Heroic, then refresh modde.
Native and Linux Flatpak Heroic config locations are scanned. Provider notices
say when results are cached or ownership is unavailable. An installed game on
disk is not proof of ownership; a local-only list is labelled accordingly.

## Saved direct launches

A direct launch runs a native executable, or invokes a Wine/umu runner with the
executable as its first argument. It does not require Lutris. Configure the
runner's prefix and runtime first, then save their absolute paths. For Proton,
use a configured umu runner and its documented environment; a raw Proton script
does not have Wine's argument convention. Direct launches clear ambient
`WINEPREFIX` and `STEAM_COMPAT_DATA_PATH` before applying saved settings, so a
terminal's previous Proton session cannot choose this game's save destination.

On Linux, an isolated subreaper waits for the command and its descendants,
including orphaned children after a launcher forks and exits. If available in the
same host namespaces, a systemd user service also tracks the session cgroup with
`ExitType=cgroup`. Inside an existing container or a different namespace, the
observer stays in the caller's namespaces.

Use the actual game command. A command that asks a pre-existing service to launch
the game can hand work outside the observed process tree. Wine prefixes must be
idle before launch so an existing Wine server cannot accept work outside that
tree. Modde waits up to ten seconds for prefix services to exit before launch and
again after patchers; a prefix that remains busy blocks launch. Other platforms
require explicit descendant-exit confirmation.

CLI configuration uses the same per-installation record as the GUI:

```sh
modde library configure INSTALLATION_ID
modde library configure INSTALLATION_ID --file launch.json
modde library play INSTALLATION_ID
modde library favorite INSTALLATION_ID
modde library favorite INSTALLATION_ID --remove
```

Example `launch.json` (replace paths with existing files/directories):

```json
{
  "executable": "/games/example/Game.exe",
  "runner": "/home/me/.local/share/modde/runners/wine/wine-ge-9-2/bin/wine",
  "prefix": "/games/prefixes/example",
  "arguments": [],
  "wrappers": [],
  "environment": { "WINEESYNC": "1" },
  "working_directory": "/games/example",
  "profile": "my-profile",
  "use_active_profile": true,
  "store_hook": false,
  "save_directory": "/games/prefixes/example/drive_c/users/steamuser/Documents/Example/Saves",
  "sandbox": {
    "enabled": false,
    "network": true,
    "read_only": [],
    "writable": []
  }
}
```

Omit `runner` and `prefix` for native games. Profile selection has three modes:

- Set `profile` to a name to use that profile.
- Leave `profile` empty and keep `use_active_profile: true` (the default) to use
  the selected installation's active profile.
- Leave `profile` empty and set `use_active_profile: false` to skip profile
  deployment, save switching and capture. This leaves existing deployed mods and
  live saves in place; it does not revert the game to a stock installation.

The GUI exposes profile selection and the active-profile fallback. `wrappers` is
an outer-to-inner list of argument arrays, for example
`[["gamescope", "-f", "--"]]`. Configure Wine's prefix with `prefix`; an
environment `WINEPREFIX` that disagrees is rejected.
For a Proton/UMU compat container, `prefix` identifies the physical `pfx` used
for saves. An explicit `WINEPREFIX` may name the container root only when
`STEAM_COMPAT_DATA_PATH` identifies that same container and its `pfx` resolves
to the configured physical prefix.

The GUI invokes the companion `modde` CLI to share the same preparation and
capture pipeline. Packaged installs include both binaries. `MODDE_BIN` can point
to a companion CLI for a custom GUI build.

`--config-dir` selects modde's base configuration directory, while `--data-dir`
selects its data directory. Generated hooks and GUI launches pass both to the
companion CLI. A modde configuration override does not move native game saves;
use the launch environment's HOME/XDG fields for that. Provider discovery and
desktop audio credentials also use the desktop user's configuration root.

## Profiles, exact installations and saves

Installation IDs are persisted with their known paths and store associations.
Rediscovering a configured local copy through Steam or Heroic retains its ID,
launch settings, sandbox permissions and save bindings. Known symlink aliases
resolve to one copy. Existing IDs are retained during migration; conflicting
saved identities produce an error instead of silently choosing defaults.
Catalogue refresh can persist this identity metadata in `library.json`.

Path aliases record their resolved targets. Retargeting a known symlink is
rejected instead of transferring the old installation's configuration to the
new copy. Restore the original link before refreshing, or register the new
physical path as a separate installation and review its imported launch settings.
Merely refreshing Library does not authorize that transfer.

Two physical copies keep separate commands and save state. Moving a copy to an
unrelated path is not an automatic migration of its settings or vault. A store URI cannot select
between two copies of the same store app; use a direct executable in that case.

The shared CLI/GUI session pipeline:

1. Acquire the mutation lease, check pending sessions and resolve the saved
   installation, profile, save vault and prefix. Acquire resource locks.
2. Validate enabled mod sources, patcher definitions, alternate deployment
   targets and the complete direct/store-boundary command, including tool wrappers.
3. Persist a preparation journal, generate tool configuration and construct the
   launch command. Probe sandbox namespaces and mounts before live deployment.
4. Deploy to the **selected installation and prefix**, including enabled
   patchers. Capture outgoing saves before journaling and performing the save
   replacement and active-profile update.
5. Re-resolve the command and sandbox grants after deployment, so executable or
   wrapper symlinks use the newly deployed version. Persist launch intent and
   start the observer. Direct launches and store hooks
   apply enabled tool configuration, DLL overrides and wrapper commands. Explicit
   environment settings override generated tool environment; required mod DLL
   overrides are merged. The store supplies its authenticated runner command.
6. Capture saves after observed descendant exit, including unsuccessful exits.
   Commit a completion receipt before clearing the launch journal, then write
   performance output and evaluate any bisect result. Capture failures retain
   the launch journal; analysis failures retain the receipt. A separate completion
   worker can finish the exact observed session if the initiating CLI disappears.

The unique existing installation can retain its legacy active slot and complete
save-vault history through a persisted binding. Duplicate installations are not
assigned that history by guessing. They need independent save directories,
resolved from their prefixes or configured explicitly, and their own adoption.
Changing a bound save destination creates a new save scope.
Historical vaults are retained. Adoption that has already established a scoped
vault is not later redirected to a legacy vault.

Save fingerprints classify each enabled mod's stored source contents before
deployment merges files. Cosmetic-only content can be excluded without depending
on a previous staging build. Missing or unknown content stays save-affecting;
Wabbajack archive IDs also stay save-affecting because they do not identify an
individual installed directive output.

For existing saves with no active owner, save the profile and save directory,
then use **Adopt existing saves** in Launch settings, or:

```sh
modde library adopt INSTALLATION_ID --profile my-profile
```

Adoption captures existing files without replacing live saves. Separate
installations must use separate save directories for independent profile swaps.
The Saves view, CLI save commands, profile forks and experiments use the same
installation scope as Play. Choose **Manage Mods** on the desired Library copy
to select it for these game-oriented actions. They use the configured game path,
or the sole installed copy; ambiguity is an error. Restore requires that the
target profile be active for that installation. Installation vaults are stored
under the data directory's `saves/install-.../` directories.

Save discovery follows the selected Steam library or explicit prefix for
supported game layouts. UE4 configuration overlays and BG3 deployment hooks also
receive the selected prefix. For an unknown Wine-user layout, configure an
explicit save directory. Save roots for different installations must not be
equal or nested inside one another, including through symlink aliases. Switching
a parent directory would otherwise replace the child's live saves too.

`modde play my-profile --game skyrim-se` uses this same pipeline when exactly one
installation matches. Ambiguous game-only requests report the available Library
workflow instead of picking the first installation. With no profile argument,
the saved profile selection is used; without a selected profile, existing game
files and saves are left in place.

| Flag on `modde play` | Effect                                                                |
| -------------------- | --------------------------------------------------------------------- |
| `--no-switch`        | Require the chosen profile to already be active for this installation |
| `--no-deploy`        | Skip rebuilding/deploying mods                                        |
| `--no-capture`       | Skip post-session save capture                                        |

## Steam and Heroic command hooks

A store hook puts modde at the actual game-command boundary. Set it up once from
**Launch settings → Set up store wrapper**, or:

```sh
modde library hook INSTALLATION_ID
```

- **Steam:** paste the generated wrapper followed by `%command%` into that game's
  Launch Options. The GUI copies the instruction to the clipboard. Keep any
  `fgmod` preparation before modde's wrapper, and remove the old
  `modde-launch-wrapper.sh`/`.cmd` entry.
- **Heroic:** the matching installation's configuration receives a wrapper after
  `fgmod`. The previous configuration is backed up as
  `*.json.modde-before-hook`. Restart Heroic to load the edited configuration.
  Ambiguous native/Flatpak configurations are rejected; when no matching config
  exists, modde prints the manual wrapper instruction.

For **profile-managed Heroic saves**, disable automatic cloud-save synchronization
for this game and restart Heroic before Play or save experiments. The matching
game configuration must explicitly contain `"autoSyncSaves": false`; a missing
value is not accepted as proof that sync is disabled. Hook setup reports this
requirement but does not change the cloud-save setting. Modde checks it before
dispatch and when the hook is invoked directly. Heroic downloads saves before the
wrapper and uploads after it returns, outside modde's save transition and bisect
source restoration. The disk check cannot verify Heroic's in-memory settings,
which is why the restart matters.

The store command's environment supplies the effective Wine/Proton prefix and
runtime tunings. A native command without a Wine environment does not reuse an
old Steam compatdata directory. For Heroic Proton, raw Proton uses
`winePrefix/pfx`; UMU can alias `pfx` to the configured root or retain an existing
raw-Proton directory. Modde resolves the existing layout and rejects a runtime
prefix inconsistent with the selected installation, while preserving the
runner-facing container root in its environment. Initialize the chosen runner
and prefix in Heroic before adopting saves or starting performance/bisect work;
a changed runtime save destination stops an experiment rather than moving its
vault silently. Use an explicit prefix for a layout that cannot be inferred.
An environment `WINEPREFIX` override must match the store's runner-facing prefix;
use the `prefix` field for physical save discovery rather than overriding the
store's container path with its `pfx` child.

The generated hook is installation-specific and preserves the supplied argument
vector. It pins the CLI executable and modde configuration/data paths; regenerate
it after relocating or removing that CLI installation. These paths are recorded
as absolute paths even when configuration was supplied relative to the caller.
When present, Steam/Heroic game and store identifiers are checked against the
hook's installation. Set `store_hook: true`
only when the hook has actually been installed. The setup action records this
setting, but it cannot verify that Steam's Launch Options were pasted correctly.

Play first journals **Waiting for store wrapper**, then releases its mutation
lease before opening the store URI. The wrapper claims that installation's
request, resolves the store prefix/environment, prepares the selected profile,
and launches through the observer. A launch directly from the store also uses
the saved modde settings when the hook is installed. Cancel an unclaimed request
with **Cancel pending request** or `modde library recover`.

Managed store launches require this hook. An unmanaged URI launch without a hook
reports **Unobserved store session**. The URI handler's exit is never treated as
game exit, and sandboxing such a handoff is rejected.

## Session status and recovery

An unfinished session survives GUI restarts and blocks profile, deployment and
save mutations. Inspect the journal and process evidence with:

```sh
modde library status
```

After an observed exit, interrupted completion can be retried with
`modde library finish`. For an unobserved handoff, another platform, or lost
process evidence, close all game/Wine processes first, then use **Confirm exited
& capture** or:

```sh
modde library finish --confirm-exited
```

Confirmation cannot override a live observer lease or an active session cgroup.
`UMU_CONTAINER_NSENTER=1` is rejected because reconnecting to an existing UMU
service can run the game outside this session's descendant tree. Direct launches
clear an inherited value unless it is explicitly supplied in launch settings.
If a save directory has disappeared, restore it before retrying capture; modde
does not treat a missing directory as a new empty save set.

Once saves commit, **Completing analysis** displays **Retry analysis** and
**Skip automatic analysis**. Retry with `modde library finish` after repairing the
missing CSV or analysis output. The receipt prevents retry from recapturing later
live saves. To release automatic continuation and handle results manually:

```sh
modde library finish --skip-analysis
```

This also captures an exited session if capture has not finished yet; an
unobserved exit still needs `--confirm-exited`. A captured receipt can be skipped
even if its exit-status JSON is missing or corrupt, while observer leases and
session-cgroup checks remain enforced. Skip does not grade or advance a pending
bisect or undo an already committed result. Inspect `modde bisect status` and
mark, retry or abort the bisect as appropriate after releasing the receipt.

Bisect source restoration can itself leave an interrupted save journal. In that
case, run `modde library recover` first, then retry `modde library finish` to
resume the original completion receipt. A confirmed-exit Finish can report an
analysis-evidence error after capture; **Completing analysis** means those saves
have already committed and can be retained with `--skip-analysis`.

Interrupted setup or save changes display **Recover operation**, also available as:

```sh
modde library recover
```

The journal distinguishes preparation, save replacement, readiness, launch intent,
store handoff, running, observed exit and capture. Recovery before launch restores the prior active slot
and captured saves if replacement began. It never captures a partially replaced
directory. Recovery is retryable if restoration fails. Deployment and patcher
side effects are replayed by the next full Play deployment; after an interrupted
deployment, `--no-deploy` and profile-free Play are rejected until that succeeds.

CLI profile Switch/Try/Rollback, GUI experiments, and CLI/GUI save restoration
use the same journalled save transition as Play. Before replacing live files,
the journal records the outgoing snapshot's commit ID, active profile, and full
experiment stack. Recovery uses that immutable snapshot even if a restore has
already moved the profile's vault branch. A failed recovery retains its journal
for retry. Once a save-only operation has a durable completion marker, recovery
only clears that marker and preserves the completed change.

The mutation lease and new pending journal live under
`<modde_config>/sessions/`, shared across data directories. The journal records
its owning data directory; finish/recovery must use that same `--data-dir`.
Old `<modde_data>/pending-session.json` markers are recognized when that data
directory is opened and conservatively treated as possibly launched sessions.
Each observed launch records `run-*/evidence.json`, `observer.lock` and
`completion.log` beside the journal. Its inherited-environment request is removed
as soon as the observer consumes it. Post-capture continuation uses
`pending-completion.json` in that same sessions directory and also blocks new
launches and mutations until completed or explicitly skipped. GUI launch output is stored under
`<modde_data>/logs/`; the UI reads only a bounded tail.

## Manager instances

Manager entries use the validated `modde-manager onboard launch` path and offer
vanilla/HD actions. The validated game command then enters the shared Library
observer with its selected runner, prefix, tunings and DLL overrides. Launch
settings can add arguments, wrappers, environment and sandbox permissions;
manager retains ownership of executable, runner, prefix and game profile.

Runner discovery includes
`$XDG_DATA_HOME/modde/runners/wine/wine-*/bin/wine`, legacy Lutris directories,
and explicit `system` Wine selection. A `.modde-library-session.json` marker in
the instance root blocks manager mutation after an interrupted Library bridge.
Finish or recover the Library session to clear it. Manager save/addon policies
remain manager-owned; these entries do not acquire a modde game-plugin save vault.

## Opt-in Linux bubblewrap sandbox

Sandboxing is **off by default** and saved per installation. Enable it in Launch
settings for a direct executable, a store command hook or a manager game launch.
The native Linux Nix package supplies `bwrap` and systemd command-line tools;
other packages require bubblewrap installed on PATH and working user namespaces.
An enabled sandbox fails with an actionable error if setup fails. It never
silently retries without isolation.

The sandbox mounts the game, prefix and save directories writable, mod-store and
staging targets read-only, plus explicit user-granted paths. It supplies system
libraries, Nix store paths, GPU/driver resources, display/audio sockets and input
devices. Resolved command and wrapper executables are granted individually;
dedicated Wine/Proton/UMU runtime trees and store-declared runtime mounts are also
included. An arbitrary wrapper's parent directory is not granted automatically.
The complete discovered mount set is probed before deployment or save switching.
The logical HOME is preserved with private per-installation contents,
with granted save paths mounted at their original locations. Non-default XDG
configuration, data, and cache locations retain their logical paths with private
backing directories. Explicit HOME/XDG launch settings also feed native save
discovery; explicit save directories and Wine-prefix paths retain their configured
targets. Its cache persists between sessions. Host session D-Bus is not exposed
automatically; tools requiring it may need a different supported configuration. Display/input access, especially
X11, is not a security boundary against a hostile desktop client.

Network access defaults to enabled; disabling it requests a separate network
namespace. Permission arrays use existing absolute paths. Additional symlink
targets and external assets must be granted explicitly.

Wrapping a Steam/Heroic URI would sandbox only the request, not the game. Use its
command hook instead. The observer does not escape an existing Flatpak or Steam
runtime namespace through a host systemd service. This does not establish nested
container compatibility: the outer container must expose the CLI, runner,
assets and user-namespace facilities needed by the inner sandbox. The current
Flatpak manifest does not package bubblewrap or provide a host-launch bridge.
Steam-runtime/Proton nesting, Flatpak hosting, anti-cheat and individual games
still need runtime qualification.

The implementation uses bind mounts and native GPU access, with no asset copying
or rendering proxy. **Zero performance overhead is not established.** Release
qualification needs process-tree/mount checks and repeated sandbox-on/off
measurements with the same runner, prefix, settings, warmed caches and scene.
Record startup time, frame-time distributions and run-to-run variation; do not
infer parity from a successful launch alone.

## Performance capture and paired sandbox runs

`modde perf run` and bisect candidates use the same installation-scoped
preparation, supervision and save capture as Play. Configure MangoHud for the
renderer/runner first. Vulkan injection uses `MANGOHUD=1`; an OpenGL workload may
also require a saved MangoHud wrapper.

```sh
modde perf run my-profile --game skyrim-se --duration 300 --label baseline
modde perf sandbox INSTALLATION_ID --profile my-profile --pairs 3 --duration 300 --warmup-seconds 30
```

Paired automation requires a direct executable and accepts 2–30 pairs. It
alternates off/on and on/off order without changing saved sandbox preferences.
Replay the same scene and exit the game after each run. `--duration` controls
MangoHud logging, not forced game termination. A successful exit and at least
100 usable post-warmup samples in both the FPS and frame-time series are required
for each paired result. Missing, nonfinite or nonpositive frame times cannot
qualify a trace merely because it contains enough FPS rows. Capture and grading
use measured frame times; missing values or columns are not filled with
FPS-derived estimates. An all-warmup trace is rejected rather than reused as a
measurement.

Positive warmup requires finite elapsed timestamps; samples with missing or
nonfinite timestamps are excluded. A CSV without elapsed times requires an
explicit `--warmup-seconds 0` for a pre-trimmed trace. Performance bisects use the
default warmup and refuse to grade insufficient usable post-warmup FPS or
frame-time samples, or missing summary metrics, as a good candidate.
MangoHud's `elapsed` column is converted from nanoseconds to seconds before
applying warmup. Generic `time`, `time_s` and `elapsed_seconds` columns use seconds;
an explicitly named `elapsed_ns` column uses nanoseconds too.

Per-run files record requested/effective settings, installation/save scope,
platform details and session evidence. The paired report includes individual
metrics, paired p99 frame-time changes and their between-pair standard deviation.
Positive frame-time changes are worse. Startup timing is **launch to first
parseable CSV sample**, polled every 100 ms; it is not first displayed frame and
includes MangoHud's logging delay.

Store-hook captures finish when their wrapper observes exit. Their saved bisect
request also evaluates the selected crash/performance oracle then; a delayed
completion cannot grade a newer candidate. Performance bisects require a
successful, installation-scoped baseline with matching launch settings and the
default warmup. Its source profile and enabled mod order/versions must also match
the baseline; changing them requires a new baseline. Older bisects without an
installation pin must be restarted.
Abort/completion restores the source profile before candidate cleanup, and the
next Play redeploys it.

If a captured run needs manual CSV ingestion, use its recorded run ID and warmup:

```sh
modde perf ingest --run RUN_ID --csv PATH --warmup-seconds 30
modde library finish
```

While a matching completion receipt is pending, ingestion must use its recorded
warmup and carries its observed exit status. Finish recognizes an already
ingested run and resumes the remaining bisect work. Captured runs with a
`configuration.json` keep the recorded warmup even after their receipt is cleared;
manual ingestion cannot change that boundary while retaining the original
benchmark provenance. If exit evidence cannot be
read, skip automatic analysis first and ingest manually; that ingestion cannot
establish a successful observed exit for a new bisect baseline.
Re-ingesting a completed run preserves its recorded exit status, including a
failed exit or an unknown status.

No overhead measurements have been collected for this implementation. The
commands and reports provide the measurement workflow, not a performance result.

## Provider references

- [Steam GetOwnedGames](https://partner.steamgames.com/doc/webapi/IPlayerService#GetOwnedGames)
- [Heroic ownership cache](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/cache.ts)
- [Heroic protocol handling](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/protocol.ts)
- [Heroic launch, prefix initialization and cloud-sync boundaries (reviewed revision)](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/3934a83a0707baad23cd2c06bc94bd23f51e6622/src/backend/launcher.ts)
- [UMU prefix layout (reviewed revision)](https://github.com/Open-Wine-Components/umu-launcher/blob/e2b203a1fdd2af9f35166f5713cb3f85d72587e2/umu/umu_run.py)
- [MangoHud CSV columns and elapsed-time units (reviewed revision)](https://github.com/flightlessmango/MangoHud/blob/73931de948402caf3ea80f26d5bbb6b3f1d2c1d9/src/logging.cpp#L200-L230)
- [Bubblewrap sandbox model](https://github.com/containers/bubblewrap/blob/main/README.md)

See also [Save Management](saves.md), [Deployment](deployment.md),
[Executables](executables.md), [Tools](tools.md) and [Profiles](profiles.md).
