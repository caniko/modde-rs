# Troubleshooting

This page is organized as **symptom → cause → fix**. Start from the index below,
jump to the matching section, and follow the commands. Every problem links to the
guide that covers the underlying workflow in depth.

If a command itself is misbehaving (wrong flags, missing subcommand), check the
[CLI reference](../reference/cli.md) first — this page covers *runtime* and
*environment* problems, not command syntax. For install-time and build-host
problems (Nix flake errors, `openssl-sys`, Home-Manager wiring), see the
[Installation guide's Troubleshooting section](../getting-started/installation.md#troubleshooting);
the most common one is reproduced under [Build / `openssl-sys` failures](#build-openssl-sys-failures-build-host-issue)
below.

## By symptom

| Symptom | Section | Related guide |
| ------- | ------- | ------------- |
| `modde detect` doesn't list my game | [Game not detected](#game-not-detected) | [Playing](playing.md) |
| Generic game registered but install not found | [Generic game not detected](#generic-game-not-detected) | [Generic games](../games/generic-games.md) |
| `modde game detect` finds nothing / wrong dir | [Generic game not detected](#generic-game-not-detected) | [Generic games](../games/generic-games.md) |
| Wabbajack list wants local game files | [Wabbajack install issues](#wabbajack-install-issues) | [Wabbajack](wabbajack.md) |
| Home Manager says profile is "awaiting game install" | [Wabbajack install issues](#wabbajack-install-issues) | [Wabbajack](wabbajack.md) |
| Wabbajack URL/hash mismatch | [Wabbajack install issues](#wabbajack-install-issues) | [Wabbajack](wabbajack.md) |
| modde reports unavailable authored files | [Wabbajack install issues](#wabbajack-install-issues) | [Wabbajack](wabbajack.md) |
| Nexus auth fails / CDN download refused | [Nexus API authentication fails](#nexus-api-authentication-fails) | [Nexus](nexus.md) |
| Deployment fails with symlink errors | [Deployment fails](#deployment-fails) | [Deployment](deployment.md) |
| Game left in a broken state after deploy | [Deployment fails](#deployment-fails) | [Deployment](deployment.md) |
| `modde tool apply` says "could not detect install dir" | [Tool apply / revert failures](#tool-apply-revert-failures) | [Tools](tools.md) |
| `modde tool apply` says "No files to apply" | [Tool apply / revert failures](#tool-apply-revert-failures) | [Tools](tools.md) |
| `modde tool revert` says "No applied files to revert" | [Tool apply / revert failures](#tool-apply-revert-failures) | [Tools](tools.md) |
| OptiScaler: game crashes on first boot | [OptiScaler first-boot crash](#optiscaler-first-boot-crash) | [Tools](tools.md) |
| Crash Logger SSE / Trainwreck log needs install-state context | [Crash-log correlation](#crash-log-correlation) | [Conflicts](conflicts.md) |
| Crashes or perf regression, unknown which mod is at fault | [Isolating a bad mod (bisect)](#isolating-a-bad-mod-bisect) | [CLI → `modde bisect`](../reference/cli.md#modde-bisect) |
| Save vault wants "adoption" | [Save vault issues](#save-vault-issues) | [Saves](saves.md) |
| `save restore` warns about fingerprint mismatch | [Save vault issues](#save-vault-issues) | [Saves](saves.md) |
| Can't reorder mods (profile locked) | [Profile is locked](#profile-is-locked) | [Profiles](profiles.md) |
| Mod stuck on "pending user input" | [FOMOD install stuck](#fomod-install-stuck-on-pending-user-input) | [FOMOD](fomod.md) |
| "Unknown install type" / dossier written | [Unknown install type](#unknown-install-type-dossier-written) | [Mod installation](mod-installation.md) |
| Extraction fails (`7z` / `unrar` missing) | [Missing extractors](#missing-extractors) | [Mod installation](mod-installation.md) |
| Database looks corrupted | [Database issues](#database-issues) | [Data management](data-management.md) |
| `cargo build`/`test` fails in `openssl-sys` | [Build / `openssl-sys` failures](#build-openssl-sys-failures-build-host-issue) | [Installation](../getting-started/installation.md#cargo) |

## Game not detected

**Symptom:** `modde detect` does not list a game you have installed.

**Cause:** modde auto-detects games via Steam and Heroic (GOG, Epic, Sideload).
A game only appears when the launcher's own metadata can be read and matched to a
built-in or user-defined game registration. A game installed outside a supported
launcher, or in a non-default library, may not be found automatically.

**Fix:**

```bash
modde detect
```

If your game doesn't appear:

- Verify the game is installed via a supported launcher.
- For Steam, ensure the game's `appmanifest_*.acf` file exists in your Steam
  library (including extra libraries listed in `libraryfolders.vdf`).
- For Heroic, check that the game appears in `~/.config/heroic/GamesConfig/`.
- Use `--game-dir` flags to specify the path manually on commands that accept it
  (for example `modde scan --game <id> --game-dir <path>`), or set
  `gameDir` in a Home-Manager profile.

See [Playing a game](playing.md) for how detection feeds into `modde play`, and
[Deployment & VFS](deployment.md) for what happens once a game is detected.

## Generic game not detected

**Symptom:** You registered a user-defined game with `modde game add`, but modde
cannot find its install directory, or `modde game detect` reports the wrong
directory (or none at all).

**Cause:** Generic (user-defined) game support is **`Partial`** — it gives you
deployment, conflict classification, launcher integration, and executable
management, but **no bespoke scanner or save tracker**. Detection runs an
override-free chain: `install_path_override` → `install_dir_name` under a Steam
`steamapps/common/` library → modde's generic launcher detection keyed on the
game ID. If none of those resolve, modde has nowhere to deploy. The most common
causes are an `install_dir_name` that doesn't exactly match the Steam folder, a
missing `steam_app_id`, or an `executable_dir` pointing at the launcher stub
instead of the real binary.

**Fix:**

1. Find the real executable directory. `modde game detect` walks an install path
   (up to 4 levels deep) and lists executable-bearing directories, largest first:

   ```bash
   modde game detect "/home/you/.local/share/Steam/steamapps/common/Voidrunner"
   ```

   The top result (largest `.exe`) is almost always your `executable_dir`. If it
   reports `No executable-bearing directories found under <path>`, the path is
   wrong — pass the actual install root.

2. Inspect and correct the spec:

   ```bash
   modde game show <id>      # full resolved spec + source path
   modde game list           # all user-defined games and their files
   ```

3. Re-register with the right detection fields (use `--force` to overwrite):

   ```bash
   modde game add <id> \
     --display-name "<name>" \
     --executable-dir "Binaries/Win64" \
     --steam-app-id 998877 \
     --install-dir-name "Voidrunner" \
     --force
   ```

   `install_path_override` is **not** settable from `add`. If detection still
   fails (non-Steam install, unusual layout), edit
   `~/.local/share/modde/games/<id>.toml` by hand to add an absolute
   `install_path_override = "/abs/path"`, or `modde game import` a spec that
   contains it. When set and present on disk, the override short-circuits all
   other detection.

> **What you do not get:** `modde scan --game <id>` has no game-specific
> heuristics for a generic game, and `modde save` save-profile features are
> unavailable — generic games ship `scanner: None` and `save_tracker: None`.
> That is the boundary of the `Partial` status, not a bug. Import mods directly
> into a profile instead of scanning.

See [Generic & user-defined games](../games/generic-games.md) for the full
GameSpec schema, validation rules, and the `modde game` CLI surface.

## Wabbajack install issues

**Symptom / cause / fix** for the common Wabbajack failure modes. See the
[Wabbajack modlists guide](wabbajack.md) for the full workflow.

If a Skyrim SE modlist reports that it references local game files, pass
`--game-dir "/path/to/Skyrim Special Edition"` or set `profiles.<name>.gameDir`
in Home Manager. Lists such as Legends of the Frost need vanilla files from the
local `Data/` directory (these are Wabbajack `GameFileSourceDownloader` entries —
not downloads, but reads of the installed game).

If Home Manager says a profile is awaiting game install, install the game with
Steam or Heroic first. modde waits for launcher-managed game installs; it does
not install Skyrim itself. After the game exists, set `gameDir` and rebuild Home
Manager. (Use `installMode = "await-game"` to keep activation non-fatal until
then — see [Deployment & VFS](deployment.md#wabbajack-profiles).)

If a local game file fails hash verification, verify that Skyrim SE is fully
installed, up to date, and not modified in place. Wabbajack expects exact
vanilla file contents for game-file sources.

If Home Manager reports a Wabbajack URL/hash mismatch, recompute the Nix fetch
hash for the actual authored-files `.wabbajack` URL and update
`wabbajackList.hash`. Note that some authored-files CDN links resolve through
Wabbajack's chunked download page, so `pkgs.fetchurl` may 404 even when modde's
chunk-aware downloader works — in that case download with
`modde wabbajack download` and use `wabbajackList.path` instead.

If downloads fail before staging begins, check that
`programs.modde.nexus.apiKeyFile` points to a readable Nexus API key file (see
[Nexus authentication](nexus.md#authentication)).

If modde reports unavailable Wabbajack authored files, the upstream
authored-files entries referenced by the `.wabbajack` manifest are missing.
The error lists each archive, expected hash, metadata URL, and a `curl -fI`
validation command. The upstream modlist or Wabbajack authored-files publisher
must restore those exact files or publish a newer `.wabbajack` manifest.

If you have the exact archives locally, import them explicitly:

```bash
modde wabbajack import-archive /path/to/list.wabbajack /path/to/archive.7z
```

Import matches by Wabbajack hash only. A file with the same name but a different
hash is refused.

## Nexus API authentication fails

**Symptom:** Browsing, downloading, or installing from Nexus fails with an auth
error, or downloads are refused.

**Cause:** A missing/invalid API key, or a Free (non-Premium) account trying to
generate CDN download links. See [Nexus Mods](nexus.md) for the full setup.

**Fix.** Check your API key status:

```bash
modde nexus status
```

If invalid, re-authenticate:

```bash
modde nexus auth
```

**API key lookup order**: OAuth token, `~/.config/modde/nexus_api_key`,
`NEXUS_API_KEY` env var, system keyring, `NEXUS_API_KEY_FILE` env var, legacy
`settings.toml` key.

**CDN downloads require Premium**: Free Nexus accounts cannot generate CDN
download links. You'll see an error if you try to download without Premium. For
declarative (sops-nix / Home-Manager) key wiring, see
[Nexus → Using sops-nix](nexus.md#using-sops-nix-declarative).

## Crash-log correlation

**Symptom:** You have a Crash Logger SSE or Trainwreck log and need to know
which installed mod, plugin, DLL, or asset it points at in the current modde
profile.

**Cause:** Web analyzers and MO2/Vortex integrations can parse the log, but
they do not have modde's local install database, profile experiment stack, or
managed file manifest. modde's crash command is a correlation tool: it maps
evidence mentioned by the log to the active profile. It does not claim a root
cause unless the log itself makes that explicit.

**Fix:**

```bash
modde doctor crash /path/to/crash-2026-06-11-10-38-45.log --game skyrim-se
# or let modde pick the newest log from the game's default crash-log dirs:
modde doctor crash --game skyrim-se
```

The log path is optional for games with known crash-log locations (Skyrim
SE/AE/LE, Fallout 4, Fallout 76): modde scans the default directories under
`~/Documents/My Games/` and analyzes the newest log. Use `--profile <name>` to
analyze against a non-active profile and `--json` for the structured report.
Raw logs and reports are stored locally in the modde database for history;
nothing is uploaded.

## Isolating a bad mod (bisect)

**Symptom:** the game crashes (or performance regressed) only with mods
enabled, and you do not know which of dozens or hundreds of mods is
responsible.

**Cause:** any one of the enabled mods — manual elimination is O(n) game
launches; a binary search is O(log n).

**Fix:** run a bisect session. Each step deploys a candidate profile with half
of the remaining suspects disabled (load-order and master-dependency
constraints are respected, so dependents move with their masters):

```bash
modde bisect start --game skyrim-se --profile my-skyrim --oracle crash
modde bisect run <session-id>      # launch next candidate; repeat until converged
modde bisect status <session-id>   # progress and remaining suspects
modde bisect history <session-id>  # steps, halves, and observed signals
```

With `--oracle crash`, a new crash log appearing in the game's crash-log
directory marks the step bad automatically. With `--oracle manual`, record
each verdict yourself via `modde bisect mark <session-id> good|bad`. With
`--oracle perf`, capture a known-good baseline first with `modde perf run` and
pass its run ID as `--baseline-run`. See the
[CLI reference](../reference/cli.md#modde-bisect) for the full flag list,
including the perf-oracle statistical thresholds.

Disabling mods mid-playthrough can corrupt saves for some games; bisect refuses
risky sessions unless you pass `--force-save-risk`. Candidate profiles are
cleaned up when the session ends (`--keep-profiles` to keep them), and
`modde bisect abort <session-id>` restores the source profile early.

## Deployment fails

See [Deployment & VFS](deployment.md) for how the symlink farm is built and
deployed.

### Symlink errors

**Symptom:** deployment fails with symlink errors.

**Cause:** the game directory is missing, read-only, or has files locked by
another process; or the staging directory is incomplete.

**Fix:**

- Check that the target game directory exists and is writable.
- Ensure no other process has locked files in the game directory.
- Verify the staging directory at `~/.local/share/modde/staging/<profile>/` is
  intact.

### Rollback

**Symptom:** a deployment left the game in a broken state.

**Cause:** the most recent deploy produced an unwanted layout; the previous good
state is preserved in `staging.bak/`.

**Fix:**

```bash
modde rollback --profile my-skyrim
```

This atomically swaps back to the previous deployment. To check that deployed
files still match their sources, run `modde verify --profile my-skyrim` (see
[Deployment → Verifying integrity](deployment.md#verifying-integrity)).

## Tool apply / revert failures

**Symptom:** `modde tool apply <tool> --game <id>` or
`modde tool revert <tool> --game <id>` fails, or prints that there was nothing
to do.

**Cause and fix** depend on the message:

| Message | Cause | Fix |
| ------- | ----- | --- |
| `unknown tool: '<tool>'` | The tool ID is not one modde manages. | Use a valid ID: `mangohud`, `vkbasalt`, `gamemode`, `reshade`, `optiscaler`, or `proton`. See [Tools & overlays](tools.md#supported-tools). |
| `unsupported game: '<id>'` | The game ID isn't a built-in or registered user-defined game. | Check `modde game list` / `modde game show <id>`; register it with [`modde game add`](../games/generic-games.md#modde-game-add). |
| `could not detect install dir for <game>` | `apply`/`revert` need the real game directory and detection failed. | Make sure the game is installed and detected (see [Game not detected](#game-not-detected)). For a generic game, fix detection (see [Generic game not detected](#generic-game-not-detected)). |
| `No files to apply for <tool>` | The tool is a launch/config integration (Proton, MangoHud, vkBasalt, GameMode) that does **not** write files into the game directory — or the chosen profile/config yields no files. | This is expected for those tools; they contribute env vars, wrapper commands, generated config files, and Wine DLL overrides at deploy/launch time, not patched files. Only ReShade and OptiScaler normally stage files into the game directory. |
| `No applied files to revert for <tool>` | modde has no record of files it applied for that tool/game, so there is nothing to remove. | Nothing to revert. If you applied files *outside* modde, it will not own or remove them — modde only reverts what it recorded via `tool apply`. |

**How tracking works.** `modde tool apply` records the exact relative paths it
wrote into the database, and `modde tool revert` removes only those recorded
files, then clears the record. This is why reverting is clean and why a manual,
out-of-band copy of a DLL is not something `revert` will touch. If you adopted an
*existing* OptiScaler install (rather than letting modde install it), modde
records the working files so `revert` stays accurate — see
[Tools → Applying tool patches](tools.md#applying-tool-patches).

```bash
# Re-apply cleanly: revert what modde tracked, then apply again
modde tool revert reshade --game skyrim-se
modde tool apply  reshade --game skyrim-se
```

## OptiScaler first-boot crash

**Symptom:** After applying an OptiScaler profile (for example Stellar Blade's
`community-dxgi`), the game **crashes on the very first launch** with the proxy
DLL in place.

**Cause:** This is a known OptiScaler quirk for some titles: the proxy DLL
(`dxgi.dll`) initialises on first boot and the game can die before the next
launch succeeds. It is documented in the bundled profile notes, **not** a bad
install.

**Fix:** **Relaunch the game.** It typically works on the second launch. Before
assuming the install is broken:

- Confirm the prerequisites first. For Proton/UE titles like Stellar Blade, the
  Proton prefix must exist (`compatdata/<appid>/pfx`) — **launch the game once**
  so Proton materialises the prefix, then apply OptiScaler and relaunch.
- Verify the proxy and companion files were staged:

  ```bash
  modde tool apply optiscaler --game stellar-blade
  ```

  This writes `dxgi.dll` and companion files into the game's `Binaries/Win64`.

- If frame-gen HUD elements look wrong (interpolation artifacts on the HUD under
  DLSSG), set the **in-game sharpness slider to 0** — also a documented
  workaround, not a modde bug.

If it still crashes on every launch, revert and re-apply to rule out a partial
write:

```bash
modde tool revert optiscaler --game stellar-blade
modde tool apply  optiscaler --game stellar-blade
```

See [Tools & overlays](tools.md#applying-tool-patches) for the OptiScaler source
selector (official releases vs GOverlay builds) and the
[Stellar Blade](../games/stellar-blade.md#optiscaler-the-community-dxgi-profile)
page for that game's specific profile notes.

## Save vault issues

See [Save management](saves.md) for how the git-backed vault, branches, and
fingerprinting work.

### "Adoption required"

**Symptom:** Switching to a profile for the first time refuses to proceed because
existing saves are detected.

**Cause:** modde will not silently overwrite saves it has never seen. It asks you
to **adopt** them into a profile first, so they become the initial vault snapshot.

**Fix:**

```bash
modde save adopt --game skyrim-se --profile my-skyrim
```

### Restoring from an incompatible snapshot

**Symptom:** `save restore` warns about a fingerprint mismatch.

**Cause:** the snapshot was created with a different set of save-breaking mods
(modde embeds a SHA-256 fingerprint of save-breaking mods in each snapshot).

**Fix:** You can still restore, but the save may not load correctly in-game.
Browse history to find a compatible snapshot:

```bash
modde save history --game skyrim-se --profile my-skyrim
```

See [Save fingerprinting](saves.md#save-fingerprinting) for how compatibility is
computed and reported.

## Profile is locked

**Symptom:** you can't reorder or remove mods.

**Cause:** the profile is locked — automatically after a Wabbajack install, a
Nexus Collection, or a TOML import (to preserve a curator's load order), or
manually.

**Fix.** Inspect the lock:

```bash
modde profile lock-info my-skyrim
```

To unlock:

```bash
modde profile unlock my-skyrim
```

Or fork the profile with `--unlock` to create a freely editable copy (this strips
both the profile-level lock and all per-mod pins):

```bash
modde profile fork my-skyrim my-skyrim-custom --game skyrim-se --unlock
```

See [Profile management → Load order locking](profiles.md#load-order-locking).

## FOMOD install stuck on "pending user input"

**Symptom:** a mod's install status is `PendingUserInput` and it never finishes.

**Cause:** the mod ships a FOMOD installer that needs your selections (texture
resolution, compatibility patches, and so on).

**Fix.** Either complete the wizard in the GUI, or apply a declarative config:

1. Open the GUI (`modde gui`) and complete the wizard, or
2. Generate and apply a declarative config:

```bash
modde fomod generate /path/to/mod --format toml > choices.toml
# Edit choices.toml to select your options
modde fomod apply /path/to/mod --config choices.toml --dest /path/to/output
```

See the [FOMOD installer guide](fomod.md) for inspecting a mod's options and for
passing `--fomod-config` during a Nexus install.

## Unknown install type (dossier written)

**Symptom:** modde can't determine how to install a mod and stops.

**Cause:** the archive layout doesn't match any recognized install method (single
file set, FOMOD, or a game-specific layout). modde writes a **dossier** so you can
investigate instead of guessing.

**Fix.** The dossier lands at
`~/.local/share/modde/unknown-installers/<slug>/` and contains the archive tree,
file samples, and metadata.

```bash
modde mod diagnose <mod_id>
```

This prints the dossier path for investigation. See the
[Mod installation guide](mod-installation.md) for how install methods are
detected and what a valid layout looks like.

## Missing extractors

**Symptom:** extraction fails for `.7z` or `.rar` archives.

**Cause:** modde shells out to `7z` (7-Zip) and `unrar` for certain archive
formats, and they are not on your `PATH`.

**Fix:** On NixOS these are included in the development shell (`7zz`, `unrar`).
If running modde standalone, install them and ensure they are on your `PATH`.
This is the same class of problem as a bare-host build — the Nix dev shell
provides every external tool modde needs (see
[Build / `openssl-sys` failures](#build-openssl-sys-failures-build-host-issue)).

## Database issues

**Symptom:** modde commands error out reading state, or the database looks
corrupted.

**Cause:** the SQLite database at `~/.local/share/modde/modde.db` was interrupted
mid-write (power loss, killed process), leaving recovery files behind.

**Fix:**

1. Check if a `.db-journal` or `.db-wal` file exists (SQLite recovery files).
2. Back up the database before attempting repairs.
3. Try running any modde command — the schema migration system may repair minor
   issues.

See the [Data management guide](data-management.md) for the full on-disk layout
(database, store, staging, save vaults) and backup guidance.

## Exporting your mod list

For sharing or debugging, export your profile to CSV:

```bash
modde export --profile my-skyrim --output modlist.csv
```

See [Data management](data-management.md) for other export/backup paths.

## Build / `openssl-sys` failures (build-host issue)

**Symptom:** A `cargo build`, `cargo install modde`, or `cargo test` fails in
`openssl-sys` with something like `Could not find directory of OpenSSL
installation`, or fails to *link* with errors mentioning `wayland`, `xkbcommon`,
or `vulkan`.

**Cause:** **This is a build-host environment problem, not a modde bug.** modde
does not vendor OpenSSL, and the GUI links against system Wayland / `libxkbcommon`
/ Vulkan libraries. A bare host without those headers cannot build the workspace,
and a plain `cargo build`/`cargo test` outside the Nix shell is not a reliable
signal.

**Fix — use the Nix dev shell (recommended):**

```bash
nix develop . -c cargo build --release
# or, against the remote flake:
nix develop github:caniko/modde-rs
```

Inside `nix develop`, OpenSSL, SQLite, the GUI system libraries, and the external
extractors (`7zz`, `unrar`) are all provided, and `LD_LIBRARY_PATH` is set for
you. This is the **authoritative** build/test environment.

If you must build on a bare host, install the dev headers and point `pkg-config`
at them. On Debian/Ubuntu:

```bash
sudo apt install ca-certificates gcc pkg-config \
  libssl-dev libsqlite3-dev libdbus-1-dev \
  libwayland-dev libxkbcommon-dev libvulkan-dev
```

On Fedora: `sudo dnf install openssl-devel pkg-config sqlite-devel dbus-devel
wayland-devel libxkbcommon-devel vulkan-loader-devel`. If OpenSSL is in a
non-standard prefix, export `OPENSSL_DIR` (or `PKG_CONFIG_PATH`). The full
treatment, including the Home-Manager and flake-registry pitfalls, is in the
[Installation guide's Troubleshooting section](../getting-started/installation.md#troubleshooting).

## See also

- [Playing a game](playing.md) — launch flow and game detection
- [Wabbajack modlists](wabbajack.md) — `.wabbajack` install workflow
- [Nexus Mods](nexus.md) — API keys, downloads, collections
- [Deployment & VFS](deployment.md) — symlink farm, rollback, verify
- [Tools & overlays](tools.md) — apply/revert, OptiScaler, Proton
- [Save management](saves.md) — vaults, fingerprinting, restore
- [Profile management](profiles.md) — locks, forks, experiment stack
- [FOMOD installer](fomod.md) — interactive and declarative installs
- [Generic & user-defined games](../games/generic-games.md) — register your own title
- [CLI reference](../reference/cli.md) — every command and flag
- [Installation](../getting-started/installation.md#troubleshooting) — build-host and Nix issues
