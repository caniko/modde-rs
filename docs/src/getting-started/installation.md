# Installation

modde is Linux-first today. The live user-facing paths are the Nix flake, the
Home-Manager module, source builds from the Nix development shell, and the Attic
binary cache used by release/CI infrastructure. macOS and Windows builds exist
as experimental CI outputs, and additional package-manager channels are wired or
staged, but they are not the recommended live install path until this page marks
them live.

Use this table as channel status, not as a promise that every listed package is
already published:

| Platform | Native packages | Also available |
| -------- | --------------- | -------------- |
| **Linux** | [Nix](#nix) / Home Manager | AUR, COPR, apt, Flatpak, AppImage, tarball, Cargo are staged or build-from-source paths |
| **macOS** | [Nix](#nix) / source build | Homebrew and tarball are experimental staged outputs |
| **Windows** | Experimental staged outputs | winget, Scoop, Chocolatey, and zip are not generally published yet |

When a binary package is published it includes both `modde` and `modde-ui`;
`cargo install modde` builds the CLI only. After installing, jump to the
[Quick start](quick-start.md).

## Linux

Linux releases are gated by package family rather than by every downstream
distribution name. A channel is published only after its artifact exists and its
release smoke check passes. At the moment, Nix/Home Manager is live; the other
package families below are staged release wiring.

| Family | Primary channel | Covered distributions |
| ------ | --------------- | --------------------- |
| Debian | [apt](#debian--ubuntu-apt) / `.deb` | Debian, Ubuntu, Linux Mint, Pop!_OS |
| RPM | [COPR](#fedora--rhel-copr) / SRPM | Fedora, RHEL, Rocky Linux, AlmaLinux, Bazzite, Nobara |
| Arch | [AUR](#arch-linux-aur) | Arch, Manjaro, EndeavourOS, CachyOS |
| Nix | [flake](#nix) / Home Manager | Nix and NixOS |
| Universal | [Flatpak](#flatpak), [AppImage](#appimage), [tarball](#linux-direct-download) | Desktop, immutable, and gaming-focused Linux systems |

Linux `x86_64` is the baseline for distro packages. Linux `aarch64` is currently
released through the generic tarball and Nix paths unless a distro-specific
channel adds its own `arm64` or equivalent build.

### Arch Linux (AUR)

Planned channel. Three packages are prepared for the AUR; once published, install one with your preferred helper
(`yay`, `paru`, …):

```bash
yay -S modde-bin   # prebuilt from the signed release tarball (fastest)
yay -S modde       # build the tagged release from source
yay -S modde-git   # track the development branch
```

`modde-bin` depends on `glibc >= 2.38` (satisfied by current Arch systems). All three
provide the `modde` and `modde-ui` binaries and conflict with one another. Release
tags are signed by the maintainer GPG key
`818D507F1E62139F8A17EAA64623DEA06FDACFE1`, also exported in
[`keys/maintainers.gpg`](https://github.com/caniko/rs-modde/blob/trunk/keys/maintainers.gpg).

### Fedora / RHEL (COPR)

```bash
sudo dnf copr enable caniko/rs-modde
sudo dnf install modde
```

Planned channel. COPR builds RPMs from the signed release source. Prerelease
builds are intended for the separate `caniko/rs-modde-testing` project.

### Debian / Ubuntu (apt)

```bash
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://modde.rs/apt/key.gpg.asc | sudo gpg --dearmor -o /etc/apt/keyrings/modde.gpg
echo "deb [signed-by=/etc/apt/keyrings/modde.gpg] https://modde.rs/apt/ stable main" \
  | sudo tee /etc/apt/sources.list.d/modde.list
sudo apt update
sudo apt install modde
```

Planned channel. The repository is signed with a dedicated key (fingerprint
`CCFE4A8461DF8778F5227684B6DB8F177A951E1B`), separate from the maintainer
tag-signing key and the minisign release key. See
[`SECURITY.md`](https://github.com/caniko/rs-modde/blob/trunk/SECURITY.md)
for the signing-key policy and rotation procedure.

The apt public key is a required release input. If `dist/apt/key.gpg.asc` is not
present in a release candidate, the apt channel stays blocked until the release
maintainer exports and validates the dedicated repository key.

### Flatpak

Planned channel. The desktop app is intended to be published to Flathub:

```bash
flatpak install flathub com.tartanoglu.modde
flatpak run com.tartanoglu.modde
```

The Flatpak ships `modde-ui` (the GUI). For scripting with the `modde` CLI, use one
of the other channels.

### AppImage

Planned channel. Self-contained, no installation required once release assets are published:

```bash
chmod +x modde-ui-<version>-x86_64.AppImage
./modde-ui-<version>-x86_64.AppImage
```

A CLI AppImage (`modde-<version>-x86_64.AppImage`) is published alongside the GUI
one. Download both from the
[releases page](https://github.com/caniko/rs-modde/releases).

### Linux direct download

Planned channel. Once release assets are published, grab the tarball for your
architecture from the [releases page](https://github.com/caniko/rs-modde/releases)
and extract it:

```bash
tar xzf modde-<version>-x86_64-linux.tar.gz   # or aarch64-linux
./modde --help
```

Verify the download first — see [Verifying releases](#verifying-releases).

## macOS

### Homebrew

```bash
brew tap caniko/modde https://codeberg.org/caniko/homebrew-modde
brew install modde
```

Planned channel. The formula is intended to install both `modde` and `modde-ui`
on Apple Silicon Macs (and Linux/Linuxbrew) once published.

### macOS direct download

Experimental channel. modde's macOS binaries are ad-hoc signed (no Apple Developer ID, no notarization).
macOS quarantines downloaded binaries, so clear the quarantine attribute once after
extracting:

```bash
tar xzf modde-<version>-aarch64-darwin.tar.gz
xattr -dr com.apple.quarantine modde modde-ui
./modde --help
```

Subsequent runs work without further intervention. If you would prefer notarized
binaries (Apple Developer ID, $99/yr),
[open an issue](https://github.com/caniko/rs-modde/issues) to fund or contribute
it.

## Windows

### winget

```powershell
winget install Caniko.Modde
```
Planned channel. Use only after the winget package is published.

### Scoop

```powershell
scoop bucket add modde https://github.com/caniko/scoop-modde
scoop install modde
```
Planned channel. Use only after the Scoop bucket is published.

### Chocolatey

```powershell
choco install modde
```
Planned channel. Use only after the Chocolatey package is published.

Each Windows package installs `modde.exe` and `modde-ui.exe` on your `PATH`.

### Windows direct download

Experimental channel. Once a Windows zip is published, download
`modde-<version>-x86_64-windows.zip` from the
[releases page](https://github.com/caniko/rs-modde/releases) and extract it.
The `.exe` artifacts are Authenticode-signed; verify the signature before running:

```powershell
Get-AuthenticodeSignature .\modde.exe
Get-AuthenticodeSignature .\modde-ui.exe
```

Both should report `Status : Valid`. On Linux you can verify the same files with
`osslsigncode verify -in modde.exe`.

## Cargo

modde publishes its CLI crate to crates.io. This builds the `modde` binary from
source (the GUI lives in a separate crate that is not published to crates.io):

```bash
cargo install modde
```

Because it compiles locally, you need a build environment:

- A **Rust 2024 edition** toolchain (recent stable `rustc` / `cargo`).
- **SQLite** and **OpenSSL** development headers (`openssl-sys` will not build
  without OpenSSL — see [Troubleshooting](#troubleshooting)).

On Debian/Ubuntu, for example:

```bash
sudo apt install ca-certificates gcc pkg-config \
  libssl-dev libsqlite3-dev libdbus-1-dev \
  libwayland-dev libxkbcommon-dev libvulkan-dev
```

On Fedora: `sudo dnf install gcc pkg-config openssl-devel sqlite-devel dbus-devel
wayland-devel libxkbcommon-devel vulkan-loader-devel`.

## Build from source

```bash
git clone https://github.com/caniko/rs-modde.git
cd rs-modde
nix develop . -c cargo build --release
# Binaries at target/release/modde and target/release/modde-ui
```

The Nix dev shell provides every system library and is the authoritative build and
test environment:

```bash
nix develop . -c cargo test --workspace
```

A plain `cargo build`/`cargo test` **outside** the dev shell often fails in
`openssl-sys` (and the GUI fails to link without the Wayland/`libxkbcommon`/Vulkan
libraries). Either use the dev shell or install the headers listed under
[Cargo](#cargo). See [Troubleshooting](#troubleshooting).

## Nix

If you use Nix, modde is also a flake. This is not required and not "the" way to
install it — but it is a reproducible option, and through the home-manager module it
lets you **declare your mod profiles as code**.

### Run or install

```bash
# Run without installing
nix run codeberg:caniko/rs-modde

# Install into your profile (both modde and modde-ui)
nix profile install codeberg:caniko/rs-modde
```

The Nix build wraps each binary with a CA-certificate bundle, so HTTPS downloads
from Nexus and friends work with no extra configuration.

### Flake input + home-manager module

Add modde to your flake inputs:

```nix
{
  inputs.modde = {
    url = "codeberg:caniko/rs-modde";
    inputs.nixpkgs.follows = "nixpkgs";
  };
}
```

Then import the home-manager module and declare profiles — Wabbajack lists, Nexus
Collections, and tool overlays — that deploy on activation:

```nix
{ inputs, ... }:
{
  imports = [ inputs.modde.homeManagerModules.modde ];

  programs.modde = {
    enable = true;
    profiles.lotf = {
      game = "skyrim-se";
      installMode = "await-game";   # wait until the game is installed
      wabbajackList = {
        url = "https://example.com/lotf.wabbajack";
        hash = "sha256-...";
      };
    };
  };
}
```

modde waits for launcher-managed game installs; it does not install the base game
itself. For every option, see the
[Home-Manager module reference](../configuration/hm-module.md). You can also drop the
package straight into a system or user closure:

```nix
environment.systemPackages = [ inputs.modde.packages.x86_64-linux.modde ];
```

### Development shell

```bash
nix develop codeberg:caniko/rs-modde
# ...or, in a checkout:
nix develop
```

The shell carries the pinned Rust 2024 toolchain, coverage tooling, archive
extractors, the docs tooling, the `simit` release CLI, and every system library the
workspace links against.

## Verifying releases

Every release is built from a GPG-signed Git tag, and every artifact ships with a
signed checksum manifest. Tarballs, AppImages, and source RPMs additionally ship
Sigstore bundles and SLSA provenance.

### Step 1 — verify the signed checksum manifest

Download the artifact plus `SHA256SUMS.txt` and `SHA256SUMS.txt.minisig` from the
same release, then:

```bash
minisign -Vm SHA256SUMS.txt -p keys/minisign.pub
sha256sum -c SHA256SUMS.txt --ignore-missing
```

The minisign public key is pinned at `keys/minisign.pub`. If it ever changes, treat
the release as a **key-rotation event** and verify the new key from an independent,
maintainer-controlled channel before trusting it.

### Step 2 — verify Sigstore signature and SLSA provenance

```bash
cosign verify-blob \
  --bundle modde-<version>-x86_64-linux.tar.gz.cosign.bundle \
  --certificate-identity-regexp '.*caniko/rs-modde.*' \
  --certificate-oidc-issuer-regexp '.*' \
  modde-<version>-x86_64-linux.tar.gz

cosign verify-blob-attestation \
  --bundle modde-<version>-x86_64-linux.tar.gz.intoto.bundle \
  --type slsaprovenance1 \
  --certificate-identity-regexp '.*caniko/rs-modde.*' \
  --certificate-oidc-issuer-regexp '.*' \
  modde-<version>-x86_64-linux.tar.gz
```

The SLSA predicate records the source Git commit, the `flake.lock` digest, the
release-workflow digest, and the Attic substituter trust root used for release
builds. Each release also ships CycloneDX (`*.cdx.json`) and SPDX (`*.spdx.json`)
SBOMs. See
[`SECURITY.md`](https://github.com/caniko/rs-modde/blob/trunk/SECURITY.md)
for SBOM scanning and the full signing-key policy.

## Troubleshooting

### `error: cannot find flake 'codeberg:caniko/rs-modde'`

`codeberg:` is a flake-registry shorthand. On older Nix or a trimmed registry, use
the explicit Git URL and make sure flakes are enabled:

```bash
nix --extra-experimental-features 'nix-command flakes' \
  run "git+https://github.com/caniko/rs-modde"
```

```nix
# /etc/nix/nix.conf (or nix.settings on NixOS)
experimental-features = nix-command flakes
```

### Home-Manager module not recognized

`error: The option 'programs.modde' does not exist` means the module was not
imported. Confirm both halves are present and that `inputs` is threaded into the
module (via `extraSpecialArgs`/`specialArgs`):

```nix
inputs.modde.url = "codeberg:caniko/rs-modde";
# ...and in your home-manager configuration:
imports = [ inputs.modde.homeManagerModules.modde ];
```

The attribute is `homeManagerModules.modde` (note the trailing `.modde`). See the
[Home-Manager module reference](../configuration/hm-module.md).

### `openssl-sys` build failure outside the Nix shell

A `cargo build`/`cargo install`/`cargo test` on a bare host typically fails in
`openssl-sys` with `Could not find directory of OpenSSL installation`. modde does
not vendor OpenSSL. Either build inside `nix develop . -c …`, or install the headers
and point `pkg-config` at them (`libssl-dev pkg-config libsqlite3-dev` on
Debian/Ubuntu; `openssl-devel pkg-config sqlite-devel` on Fedora). If a build also
fails to **link** with `wayland`/`xkbcommon`/`vulkan` errors, install the GUI system
libraries listed under [Cargo](#cargo) — or just use the Nix shell.

## See also

- [Quick start](quick-start.md) — define and deploy your first profile
- [Your first profile](first-profile.md) — an end-to-end walkthrough
- [Home-Manager module reference](../configuration/hm-module.md) — every option
- [Settings file & environment](../configuration/settings-file.md) — the non-Nix config
- [`SECURITY.md`](https://github.com/caniko/rs-modde/blob/trunk/SECURITY.md) — signing keys, SBOMs, and rotation policy
