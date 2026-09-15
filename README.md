# modde

<!-- simit:badges:start -->

![CI](https://img.shields.io/badge/CI-drift-2088ff) [![Nix](https://img.shields.io/badge/Nix-managed-5277c3)](flake.nix) [![docs](https://img.shields.io/badge/docs-enabled-6f42c1)](docs) [![crates.io](https://img.shields.io/badge/crates.io-ready-f46623)](https://crates.io/crates/modde) [![release](https://img.shields.io/badge/release-configured-2ea44f)](.forgejo/workflows/release.yml) [![artifacts](https://img.shields.io/badge/artifacts-configured-2ea44f)](.forgejo/workflows/release.yml) [![Homebrew](https://img.shields.io/badge/Homebrew-configured-2ea44f)](https://codeberg.org/caniko/homebrew-modde.git) [![Chocolatey](https://img.shields.io/badge/Chocolatey-configured-7b3f99)](https://community.chocolatey.org/) [![Scoop](https://img.shields.io/badge/Scoop-configured-2ea44f)](https://github.com/caniko/scoop-modde.git) [![AUR](https://img.shields.io/badge/AUR-configured-1793d1)](dist/aur) [![COPR](https://img.shields.io/badge/COPR-configured-3f51b5)](.copr/Makefile) [![apt](https://img.shields.io/badge/apt-configured-a81d33)](dist/apt/conf/distributions) [![Flatpak](https://img.shields.io/badge/Flatpak-configured-4a86cf)](https://github.com/flathub/com.tartanoglu.modde) [![winget](https://img.shields.io/badge/winget-configured-0078d4)](https://github.com/microsoft/winget-pkgs/tree/master/manifests/c/Caniko/Modde)

<!-- simit:badges:end -->

A Linux-first game mod manager written in Rust, with experimental macOS and Windows builds. Provides mod management with virtual filesystem deployment, profile management, save versioning, and conflict detection.

Project site: <https://modde.tartanoglu.com/>
Documentation: <https://modde.tartanoglu.com/docs/>

## Supported games

| Game | Current status |
| ---- | -------------- |
| Skyrim SE/AE | `Done`: plugins, VFS, LOOT sorting, diagnostics, save tracking |
| Fallout 4 | `Done`: plugins, VFS, LOOT sorting, diagnostics, save tracking |
| Cyberpunk 2077 | `Done`: REDmod, CET, TweakXL, scripts, conflict detection |
| Fallout 76 | `Partial`: plugins, VFS, BA2 scanning; saves are effectively server-side |
| Starfield | `Partial`: plugins, VFS, diagnostics, `.sfs` save capture with a save-contamination removal gate |
| Fallout: New Vegas | `Partial`: Gamebryo plugins, VFS, scanning, save tracking |
| Oblivion | `Partial`: Gamebryo plugins, VFS, scanning, save tracking |
| Oblivion Remastered | `Partial`: hybrid UE5 pak + ESP plugins, VFS, save tracking |
| The Witcher 3 | `Partial`: `mods/` deployment, `.ws` script-conflict scan, save tracking |
| Stellar Blade | `Partial`: UE4/UE5 deployment, scanning, conflicts, OptiScaler, save tracking |
| Subnautica 2 | `Partial`: UE4 pak deployment, scanning, conflicts, save tracking |
| Baldur's Gate 3 | `Partial`: pak deployment, `modsettings.lsx` load order, save tracking |
| Stardew Valley | `Partial`: SMAPI mod deployment, scanning, save tracking |
| Mount & Blade II: Bannerlord | `Partial`: `Modules/` deployment, `SubModule.xml` dependency checks, save tracking |

Fifteen titles ship across the Creation Engine, Gamebryo, REDengine, Unreal 4/5,
Larian, SMAPI, and Bannerlord engines; additional titles can be added at runtime
as [user-defined games](docs/src/games/generic-games.md). Games are
auto-detected via Steam (Proton) and Heroic (GOG, Epic, sideload) launchers.
The canonical status baseline for these claims lives in `docs/capability-matrix.toml`.

## Features

- **Virtual filesystem deployment**: Symlink farm keeps the game directory clean and unmodified; atomic rollback to previous deployments
- **Wabbajack on Linux**: Native parsing of `.wabbajack` modlist archives without a Windows VM
- **Profile management**: Create, fork, switch, and delete profiles; stackable experiments with rollback (like git branches); load order locking
- **Save management**: Git-backed save vaults with SHA-256 fingerprinting, compatibility warnings, and auto-capture for games with real save tracker support
- **Conflict detection**: Graph-based collision analysis with classification (dangerous vs cosmetic) and resolution suggestions
- **Nexus-first installs**: Nexus Mods API (REST + GraphQL browse/search), `nxm://`, Browse Nexus, Wabbajack modlists, and Nexus Collections are the primary shipped install flows
- **Additional download backends**: GitHub, Direct, Google Drive, MEGA, and MediaFire backends exist today mainly for Wabbajack/directive installs
- **Installers**: FOMOD is shipped end to end (interactive wizard plus declarative TOML/JSON/Nix configs); BAIN detection/execution exists but still requires the user-input selection flow
- **Gaming tools & executables**: MangoHud, vkBasalt, GameMode, ReShade, OptiScaler, and Proton are configured and patched from both the CLI and UI; named external executables (xEdit, BodySlide, Nemesis, …) run with overwrite capture via `modde exec` / `modde tool add-executable`
- **Diagnostics**: CLI and UI diagnostics use real plugin order plus resolved conflicts instead of placeholder inputs
- **Crash-log correlation**: Import local Crash Logger SSE or Trainwreck logs and correlate mentioned plugins, DLLs, and assets with modde's managed profile, versions, install timestamps, and load order
- **Reachable advanced views**: Downloads, Data Files, Diagnostics, Tools, and Executables are connected in the UI; some remain `Partial` rather than MO2-complete (see [parity audit](docs/src/reference/parity.md))

## Architecture

| Crate           | Purpose                                                                                                                                              |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `modde-core`    | SQLite database, VFS/symlink farm, profiles & experiments, load-order resolver, collision detection, save vaults, installer pipeline, stock snapshots |
| `modde-games`   | Game plugins for 15 titles across the Creation Engine, Gamebryo, REDengine, Unreal 4/5, Larian, SMAPI, and Bannerlord engines; the `GamePlugin` trait, launcher detection (Steam/Heroic), overlay tools, and user-defined games |
| `modde-sources` | Download backends (Nexus REST + GraphQL, Wabbajack, GitHub, Direct, Google Drive, MEGA, MediaFire), archive extraction (zip/7z/rar/BSA/BA2), FOMOD, and partial BAIN support |
| `modde`         | 24+ top-level commands with 60+ subcommands covering detect, install, deploy, profiles, saves, tools, executables, and user-defined games           |
| `modde-ui`      | Iced GUI with Mod List, Browse Nexus, Collections, Wabbajack, Downloads, Data Files, Diagnostics, Tools, Executables, FOMOD wizard, and Settings views |

## Usage

```bash
# Detect installed games
modde detect

# Install a Wabbajack modlist
modde install wabbajack /path/to/modlist.wabbajack \
  --profile my-skyrim \
  --game-dir "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition"

# Deploy mods and play
modde play my-skyrim --game skyrim-se

# Manage profiles
modde profile fork my-skyrim experimental-build --game skyrim-se
modde profile try experimental-build --game skyrim-se   # start experiment
modde profile rollback experimental-build --game skyrim-se  # revert

# Manage saves
modde save capture --game skyrim-se --profile my-skyrim
modde save history --game skyrim-se --profile my-skyrim
modde save watch --game skyrim-se  # auto-capture on changes

# Analyze conflicts
modde collisions --profile my-skyrim --game skyrim-se

# Diagnose profile state and explain a local crash log with cited evidence
modde doctor profile --game skyrim-se --profile my-skyrim
modde doctor crash /path/to/crash-2026-06-11-10-38-45.log --game skyrim-se
modde doctor explain /path/to/crash-2026-06-11-10-38-45.log --game skyrim-se --provider local

# Check for updates
modde update check --profile my-skyrim --game skyrim-se --period 1w

# Launch the GUI
modde gui
```

## Installation

The currently live user-facing install paths are the Nix flake, the
Home-Manager module, source builds from the Nix development shell, and the Attic
binary cache used by CI. Other distribution channels are wired or staged in the
release workflow, but should not be treated as generally available until the
[installation guide](docs/src/getting-started/installation.md) marks them live.

### Linux

Linux support is staged by package family: Debian/Ubuntu/Mint/Pop!_OS through
apt and `.deb`, Fedora/RHEL/Rocky/Alma/Bazzite/Nobara through COPR/SRPM, Arch
derivatives through AUR, Nix/NixOS through the flake and Home Manager module, and
Flatpak/AppImage/tarballs as universal fallback channels. Today, only the Nix
flake/Home-Manager path is live for end users; the other commands below document
the intended channel wiring and become user-facing only after their release gate
is marked live.

```bash
# Planned Arch (AUR) — modde-bin (prebuilt), modde (source), or modde-git
yay -S modde-bin

# Planned Fedora / RHEL (COPR)
sudo dnf copr enable caniko/rs-modde
sudo dnf install modde

# Planned Debian / Ubuntu (apt)
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://modde.rs/apt/key.gpg.asc | sudo gpg --dearmor -o /etc/apt/keyrings/modde.gpg
echo "deb [signed-by=/etc/apt/keyrings/modde.gpg] https://modde.rs/apt/ stable main" \
  | sudo tee /etc/apt/sources.list.d/modde.list
sudo apt update && sudo apt install modde

# Planned Flatpak (GUI)
flatpak install flathub com.tartanoglu.modde

# Planned AppImage (self-contained) or tarball from the releases page
chmod +x modde-ui-<version>-x86_64.AppImage && ./modde-ui-<version>-x86_64.AppImage
```

### macOS

```bash
brew tap caniko/modde https://codeberg.org/caniko/homebrew-modde
brew install modde
```

The Homebrew formula and macOS tarballs are staged release outputs, not the
recommended live install path today. When a macOS release asset is published,
clear the quarantine attribute once after extracting, then run normally:

```bash
tar xzf modde-<version>-aarch64-darwin.tar.gz
xattr -dr com.apple.quarantine modde modde-ui
./modde --help
```

macOS binaries are experimental, ad-hoc signed, and not notarized. The `xattr
-dr` step removes the "downloaded from the internet" flag that triggers
Gatekeeper; subsequent runs work without further intervention.

### Windows

```powershell
winget install Caniko.Modde      # or: scoop install modde / choco install modde
```

Windows packages and zip artifacts are staged release outputs, not the
recommended live install path today. When published, each Windows package
installs `modde.exe` and `modde-ui.exe` on your `PATH`. For a downloaded `.zip`,
verify the Authenticode signature before running:

```powershell
Get-AuthenticodeSignature .\modde.exe
Get-AuthenticodeSignature .\modde-ui.exe
```

Both should report `Status : Valid`. On Linux you can verify the same files with
`osslsigncode verify -in modde.exe`.

### Cargo

```bash
cargo install modde
```

This builds the `modde` CLI from source (the GUI lives in a separate crate not
published to crates.io). It requires a Rust 2024 edition toolchain plus SQLite
and OpenSSL development headers — `openssl-sys` will not build without OpenSSL.
See the [installation guide](docs/src/getting-started/installation.md) for the
per-distro package lists.

### From source

```bash
git clone https://github.com/caniko/rs-modde.git
cd rs-modde
nix develop . -c cargo build --release
# Binaries at target/release/modde and target/release/modde-ui
```

Use `nix develop . -c cargo test --workspace` for authoritative validation. A
plain `cargo test --workspace` outside the Nix shell is not a reliable signal
because `openssl-sys` will fail to locate OpenSSL on an unprepared host.

### Nix

If you use Nix, modde is also a flake — a reproducible install that, through the
home-manager module, additionally lets you declare your mod profiles as code.
It's one option among many, not required and not "the" way in.

```bash
# Run directly
nix run codeberg:caniko/rs-modde#modde

# Install to profile (both modde and modde-ui)
nix profile install codeberg:caniko/rs-modde#modde

# Development shell
nix develop codeberg:caniko/rs-modde
```

To wire the flake into your own config and declare profiles, add it as a flake
input and import the home-manager module (see [Home-Manager
Module](#home-manager-module) below):

```nix
# In your flake.nix inputs:
inputs.modde.url = "codeberg:caniko/rs-modde";
```

## Privacy

### Telemetry

modde has a `remote-telemetry` Cargo feature in the `modde` crate. It is **opt-in**
and off by default in published builds. Normal builds send nothing.

When built with `remote-telemetry`, two independent remote paths can exist:

- Crash telemetry through `RS_MODDE_TELEMETRY_ENDPOINT` and
  `RS_MODDE_TELEMETRY_TOKEN`, used only for modde process crash capture.
- Compatibility oracle reporting through `MODDE_COMPAT_ORACLE_OPT_IN=1` and
  `MODDE_COMPAT_ORACLE_ENDPOINT`, used only after local crash-log correlation.

The compatibility oracle uploads only derived fields: schema version, client
version, game id, coarse platform, salt epoch, hashed mod identities, hashed
mod-pair keys, a hashed mod set, and a hashed crash signature. It never uploads
raw crash logs, paths, profile names, display names, plugin names, Nexus tokens,
usernames, or install IDs. Failed uploads are queued under the local telemetry
directory and retried later. The backend suppresses query results below the
minimum cohort threshold before returning aggregate crash-rate statistics.

Until you explicitly build the feature and set the relevant opt-in environment
variables, assume modde sends nothing. To confirm feature state, inspect
`cargo tree -e features` from a Nix development shell.

## Home-Manager Module

For Nix users, a home-manager module is included as the declarative option:
configure your mod profiles as code and have them deploy on activation.

```nix
{
  programs.modde = {
    enable = true;
    profiles = {
      my-skyrim = {
        game = "skyrim-se";
        installMode = "auto";
        gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
        wabbajackList = {
          url = "https://example.com/modlist.wabbajack";
          hash = "sha256-...";
        };
      };
      my-local-skyrim = {
        game = "skyrim-se";
        gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
        wabbajackList = {
          path = /nix/store/...-Legends-of-the-Frost.wabbajack;
        };
      };
      my-cyberpunk = {
        game = "cyberpunk2077";
        nexusCollection = {
          slug = "my-collection";
          version = "1";
        };
      };
    };
  };
}
```

Set `installMode = "await-game"` while the game is not installed yet. modde
waits for Steam/Heroic-managed game installs and does not install the base game
itself.

Add the module to your home-manager imports from the flake:

```nix
# In your flake.nix inputs:
inputs.modde.url = "codeberg:caniko/rs-modde";

# In your home-manager config:
imports = [ inputs.modde.homeManagerModules.modde ];
```

## Platform Support

modde's live support target is Linux. macOS and Windows builds are produced by
CI for validation and future distribution work, but they remain experimental
until the installation guide marks their channels live.

| Platform | Architectures            | Status          |
| -------- | ------------------------ | --------------- |
| Linux    | x86_64, aarch64          | Fully supported |
| macOS    | aarch64                  | Experimental CI build |
| Windows  | x86_64                   | Experimental CI build |

## CI

Forgejo Actions on Codeberg runs `cargo fmt --check`, `cargo clippy`,
coverage tests, Rust release builds, Nix package builds, and `nix flake check`
on every push and pull request. Pushes to `trunk` also build and deploy the
documentation site and presentation website to Codeberg Pages.

Tag releases are managed through `cargo xtask release`. Run
`cargo xtask release X.Y.Z --dry-run` to preview a workspace release, then
`cargo xtask release X.Y.Z` to publish stable crates.io crates and push the bare
`X.Y.Z` tag. Release tags build Linux, macOS, and Windows CLI/GUI artifacts,
but only the channel statuses in the installation guide decide what is
advertised as live.

## Website and docs

The project site is generated from `website/plinth-project.toml` by the
Plinth `plinth-project` tool; the documentation (`docs/`) is an
[mdBook](https://rust-lang.github.io/mdBook/). The combined `site` output places
the website at the root and the docs under `/docs/`.
The public custom domain is `https://modde.tartanoglu.com/`; the Codeberg
Pages branch must contain a `.domains` file with exactly `modde.tartanoglu.com`.

```bash
nix build .#website
nix build .#docs
nix build .#site
```

For local editing:

```bash
just website-serve       # render, serve, and open the project site
plinth-project serve --config website/plinth-project.toml --out website/public --watch --no-open
cd docs && mdbook serve  # documentation
```

`plinth-project serve --watch` rerenders and reloads browser tabs when the site
config, capability matrix, or static assets change. Rust generator or renderer
edits still require restarting the command.

Publish with `nix run .#deploy-pages`. Because this site uses a custom domain,
keep the legacy Codeberg Pages model: build `.#site`, force-push the generated
output to the `pages` branch, include `.domains`, and point DNS at
`rs-modde.caniko.codeberg.page`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## Security

See [SECURITY.md](SECURITY.md) for the security policy.

## License

GPL-3.0-only

[issues]: https://github.com/caniko/rs-modde/issues
