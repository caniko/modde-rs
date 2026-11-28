# Your first profile

Set up a playable Skyrim Special Edition profile with two mods from the command
line. The commands are the same on Linux, macOS, and Windows, using `skyrim-se`
as the game id. For the declarative Nix home-manager workflow, see the
[Quick Start](quick-start.md).

By the end you will have:

- modde installed and a Nexus key configured
- Skyrim SE detected
- a profile named `first-run`
- two mods installed (one plain Nexus mod, one with a FOMOD installer)
- a conflict report you understand
- the profile deployed into the game directory
- the game launched and a save captured into the vault

## 0. Before you start

Install **Skyrim Special Edition** through Steam or Heroic and launch it once
so the launcher writes its files. In the next step, modde detects the install.
modde never installs the base game.

You also need a [Nexus Mods](https://www.nexusmods.com/) account. Downloading
mod files over CDN links requires a Nexus **Premium** subscription.

## 1. Install modde

Install via your platform's package manager, direct download, Cargo, or the Nix
flake. Every channel ships the `modde` CLI and `modde-ui` desktop app. Examples:

```bash
# Arch Linux (AUR)
yay -S modde-bin

# Fedora / RHEL (COPR)
sudo dnf copr enable caniko/modde-rs && sudo dnf install modde

# Debian / Ubuntu (apt) — see Installation for the keyring setup
sudo apt install modde

# macOS (Homebrew)
brew tap caniko/modde https://codeberg.org/caniko/homebrew-modde
brew install modde

# Windows (winget)
winget install Caniko.Modde

# Any platform with Cargo (builds the CLI from source)
cargo install modde
```

With Nix, try modde without installing via `nix run github:caniko/modde-rs -- detect`
or install both binaries with `nix profile install github:caniko/modde-rs`.
The home-manager module lets you declare profiles as code.

See [Installation](installation.md) for the full channel list, the exact
commands, the macOS Gatekeeper quarantine-clear step, the Windows Authenticode
signature check, and release verification. From here on, the walkthrough assumes
`modde` is on your `PATH`.

## 2. Configure Nexus authentication

Save your Nexus API key to the system keyring once:

```bash
modde nexus auth
```

modde prompts for the key and stores it in your OS keyring: secret-service on
Linux, Keychain on macOS, or Credential Manager on Windows. Verify the key and
check your Premium status with:

```bash
modde nexus status
```

If you manage secrets declaratively, set `nexus.apiKeyFile` in home-manager
instead. The full credential precedence (OAuth token, key file, env var,
keyring, …) is documented in the [Nexus Mods guide](../guides/nexus.md).

## 3. Detect your game

```bash
modde detect
```

modde scans Steam and Heroic (GOG, Epic, Sideload) and prints every game it
found with its id and install path. You should see a row for Skyrim Special
Edition with the id `skyrim-se`. Copy that id; you will pass it as `--game` to
nearly every command.

If Skyrim does not appear, make sure it is installed and has been launched once.
You can still proceed by passing paths explicitly, or register a custom game
with `modde game add` (see [Supported games](../games/supported-games.md)).

## 4. Create a profile

A **profile** is a named, per-game collection of mods with its own load order,
save assignments, and deployment state. Create one:

```bash
modde profile create first-run --game skyrim-se
```

Confirm it exists and see which profile is active:

```bash
modde profile list --game skyrim-se
modde profile active --game skyrim-se
```

Profiles are cheap to create. Fork `first-run` to experiment without risking
your working setup. See [Profile management](../guides/profiles.md).

## 5. Add your first mod (a plain Nexus mod)

Give modde a Nexus page URL. It fetches the file list, downloads the latest
main file over CDN (Premium required), extracts it, and adds it to the profile:

```bash
modde install mod \
  https://www.nexusmods.com/skyrimspecialedition/mods/12345 \
  --profile first-run
```

Replace `12345` with the real mod id from the URL of the mod you want. A SkyUI-
or USSEP-style utility mod is a good, low-risk first pick.

## 6. Add a second mod (one with a FOMOD installer)

Many larger Skyrim mods ship a **FOMOD** installer that asks questions (texture
resolution, compatibility patches, optional features). modde detects
`fomod/ModuleConfig.xml` automatically. For a reproducible, non-interactive
install you can record your answers in a config file first.

Inspect what a mod offers without installing it:

```bash
modde fomod inspect /path/to/extracted-mod
```

Generate a choices template, edit it, then install with it applied:

```bash
modde fomod generate /path/to/extracted-mod --format toml > my-choices.toml
# edit my-choices.toml to pick the options you want

modde install mod \
  https://www.nexusmods.com/skyrimspecialedition/mods/67890 \
  --profile first-run \
  --fomod-config my-choices.toml
```

modde saves your selections in the profile and replays them on future deployments
for deterministic results. See the [FOMOD guide](../guides/fomod.md) for generating,
inspecting, and applying configs.

## 7. Analyze conflicts

When two mods ship the same file, **load order priority** decides the winner:
the mod later in the list. Check for overlaps before deploying:

```bash
modde collisions --profile first-run
```

The report groups conflicts by severity:

| Level    | Meaning                  |
| -------- | ------------------------ |
| Critical | Will cause game issues   |
| Major    | Likely visible problems  |
| Cosmetic | Minor visual differences |

Add `--all` to show cosmetic conflicts, which are hidden by default. To print
ready-to-run commands that hide files that never win:

```bash
modde collisions --profile first-run --all --suggest-hides
```

For a broader health check (missing masters, Form 43 plugins, and more), run:

```bash
modde doctor profile --game skyrim-se --profile first-run
```

For Bethesda games you can also sort plugin order with the LOOT masterlist
(`modde loot sort --game skyrim-se`). Conflict resolution, per-file hiding, and
plugin load order are all covered in the
[Conflicts & load order guide](../guides/conflicts.md).

## 8. Deploy

Now build the symlink farm and link it into the game directory:

```bash
modde deploy --profile first-run --game skyrim-se
```

Deployment is non-destructive: it never edits your original game files, stages
everything under `~/.local/share/modde/staging/first-run/`, and keeps the
previous deployment in `staging.bak/` so you can `modde rollback` at any time.
Check that deployed files match their sources with:

```bash
modde verify --profile first-run
```

What each phase does — Build, Materialize, Deploy — is detailed in the
[Deployment & VFS guide](../guides/deployment.md).

## 9. Play

`modde play` switches to the profile (swapping saves), deploys the mods, launches
through the detected launcher, and auto-captures saves on exit:

```bash
modde play first-run --game skyrim-se
```

Save capture depends on the launcher:

- **Heroic / direct launch** — modde waits for the game to exit, then captures
  saves automatically.
- **Steam launch** — Steam returns immediately, so modde hands save capture to
  its launch wrapper. If a save is not captured, run capture manually (next
  step).

Skip steps with `--no-switch`, `--no-deploy`, or `--no-capture`. See
[Playing a game](../guides/playing.md).

## 10. Capture a save

modde keeps a **git-backed save vault** per game, with one branch per profile
and a mod fingerprint embedded in each snapshot. After `modde play` exits, your
save should already be captured. To capture explicitly at any point:

```bash
modde save capture --game skyrim-se --profile first-run -m "first run, level 5"
```

If you launched through Steam and the wrapper has not captured yet:

```bash
modde save auto-capture --game skyrim-se
```

Browse the history or restore an earlier snapshot:

```bash
modde save history --game skyrim-se --profile first-run
modde save restore <commit-id> --game skyrim-se --profile first-run
```

If you already had saves before setting up modde, adopt them once with
`modde save adopt --game skyrim-se --profile first-run` so the first profile
switch handles them cleanly. Fingerprinting, watching, and assignments are
covered in [Save management](../guides/saves.md).

## Where to go next

With a reproducible profile deployed, you can:

- [Profile management](../guides/profiles.md) — fork `first-run`, lock load order,
  and use the experiment stack to try changes non-destructively
- [Wabbajack modlists](../guides/wabbajack.md) — install a full `.wabbajack` list
  instead of hand-picking mods
- [Nexus Mods](../guides/nexus.md) — Collections, the `nxm://` handler, and update
  checks
- [Conflicts & load order](../guides/conflicts.md) — deeper conflict resolution
  and Bethesda plugin sorting
- [Tools & overlays](../guides/tools.md) — MangoHud, ReShade, OptiScaler, Proton
  settings, and running xEdit/BodySlide with overwrite capture
- [Save management](../guides/saves.md) — fingerprinting, watching, and restore

## See also

- [Quick Start](quick-start.md) — the same flow, condensed, plus the
  declarative home-manager path
- [Installation](installation.md) — install channels and verification
- [Supported games](../games/supported-games.md) — every shipping game id
- [Deployment & VFS](../guides/deployment.md) — how deploy works
- [Playing a game](../guides/playing.md) — `modde play` reference
