# Your first profile

This walkthrough takes you from a fresh modde install to a deployed, playable
Skyrim Special Edition profile with a couple of mods, all from the command line.
The commands are identical on Linux, macOS, and Windows, and every one uses the
`skyrim-se` game id. If you use Nix and prefer the declarative home-manager
workflow, see the [Quick Start](quick-start.md); this page is the imperative
tour for newcomers.

By the end you will have:

- modde installed and a Nexus key configured
- Skyrim SE detected
- a profile named `first-run`
- two mods installed (one plain Nexus mod, one with a FOMOD installer)
- a conflict report you understand
- the profile deployed into the game directory
- the game launched and a save captured into the vault

## 0. Before you start

modde never installs the base game. Install **Skyrim Special Edition** through
Steam or Heroic first and launch it once so the launcher writes its files. modde
will detect that install in the next step.

You also need a [Nexus Mods](https://www.nexusmods.com/) account. Downloading
mod files over CDN links requires a Nexus **Premium** subscription.

## 1. Install modde

Install modde however suits your platform — a native package manager, a direct
download, Cargo, or, if you use Nix, the flake. Every channel ships both the
`modde` CLI and the `modde-ui` desktop app. A few examples:

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

If you use Nix, modde is also a flake — `nix run github:caniko/modde-rs -- detect`
to try it without installing, or `nix profile install github:caniko/modde-rs`
to add both binaries to your profile. The home-manager module additionally lets
you declare your profiles as code.

See [Installation](installation.md) for the full channel list, the exact
commands, the macOS Gatekeeper quarantine-clear step, the Windows Authenticode
signature check, and release verification. From here on, the walkthrough assumes
`modde` is on your `PATH`.

## 2. Configure Nexus authentication

Save your Nexus API key to the system keyring once:

```bash
modde nexus auth
```

This prompts for your key and stores it securely in your OS keyring (the
secret-service on Linux, Keychain on macOS, the Credential Manager on Windows).
Confirm it worked — and check whether you have Premium — with:

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

Profiles are cheap. Later you can fork `first-run` to experiment without risking
your working setup — see [Profile management](../guides/profiles.md).

## 5. Add your first mod (a plain Nexus mod)

Install a single mod straight from its Nexus page URL into the profile. modde
fetches the file list, picks the latest main file, downloads it over CDN
(Premium required), extracts it, and adds it to the profile:

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

Your selections are saved to the profile and replayed on future deployments, so
the result is deterministic. The [FOMOD guide](../guides/fomod.md) covers
generating, inspecting, and applying configs in depth.

## 7. Analyze conflicts

Two mods that both ship the same file will collide; only one can win. modde
resolves collisions by **load order priority** — the mod later in the list wins.
Before deploying, see what overlaps:

```bash
modde collisions --profile first-run
```

The report groups conflicts by severity:

| Level    | Meaning                  |
| -------- | ------------------------ |
| Critical | Will cause game issues   |
| Major    | Likely visible problems  |
| Cosmetic | Minor visual differences |

By default cosmetic conflicts are hidden; add `--all` to see them. To get
ready-to-run commands that hide files which never win anyway:

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

Launch the modded game. `modde play` switches to the profile (swapping saves),
deploys, launches through the detected launcher, and auto-captures saves when
the game exits:

```bash
modde play first-run --game skyrim-se
```

A note on launchers:

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

Browse the history and restore an earlier snapshot when you want:

```bash
modde save history --game skyrim-se --profile first-run
modde save restore <commit-id> --game skyrim-se --profile first-run
```

If you already had saves before setting up modde, adopt them once with
`modde save adopt --game skyrim-se --profile first-run` so the first profile
switch handles them cleanly. Fingerprinting, watching, and assignments are
covered in [Save management](../guides/saves.md).

## Where to go next

You now have a complete, reproducible modded profile. Branch out from here:

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
