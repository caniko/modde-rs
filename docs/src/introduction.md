# modde

**modde** is a Linux-first game mod manager written in Rust. It gives you
declarative, reproducible mod management with a virtual-filesystem deployment
that keeps your game directory clean, git-backed save vaults, profile
experiments, and graph-based conflict detection — and it installs Wabbajack
modlists and Nexus Collections natively, no Windows VM required.

The live install path today is the Nix flake/Home-Manager integration or a
source build from the Nix development shell. macOS and Windows builds exist as
experimental CI outputs, and package-manager channels are staged until the
installation guide marks them live.

- **Project site:** <https://modde.tartanoglu.com/>
- **Source:** <https://github.com/caniko/rs-modde>

## What modde does

- **Virtual filesystem deployment** — a symlink farm overlays mods without touching
  the original game files, with atomic rollback. See [Deployment & VFS](./guides/deployment.md).
- **Wabbajack & Nexus** — install `.wabbajack` modlists and Nexus Collections, browse
  and search Nexus, and resolve downloads from many backends. See
  [Wabbajack modlists](./guides/wabbajack.md) and [Nexus Mods](./guides/nexus.md).
- **Profiles & experiments** — per-game mod sets with a non-destructive, git-like
  experiment stack. See [Profiles](./guides/profiles.md).
- **Save vaults** — git-backed save snapshots with SHA-256 fingerprinting and
  pre-restore compatibility warnings. See [Save management](./guides/saves.md).
- **Conflict detection** — graph-based collision analysis with severity
  classification. See [Conflicts & load order](./guides/conflicts.md).
- **Tools & executables** — MangoHud, vkBasalt, GameMode, ReShade, OptiScaler, and
  Proton, plus named external executables with overwrite capture. See
  [Tools & overlays](./guides/tools.md) and [Executables](./guides/executables.md).

## Where to start

- New here? Read [Installation](./getting-started/installation.md), then
  [Quick start](./getting-started/quick-start.md) and the end-to-end
  [Your first profile](./getting-started/first-profile.md) walkthrough.
- Want to know if your game is supported? See [Supported games](./games/supported-games.md)
  — modde ships 15 titles across seven engine families, plus
  [user-defined games](./games/generic-games.md).
- Curious how it compares to Mod Organizer 2? See the
  [MO2 parity & capability audit](./reference/parity.md).
- Looking for a command or option? See the [CLI reference](./reference/cli.md),
  the [Home-Manager module](./configuration/hm-module.md), the
  [settings file](./configuration/settings-file.md), and the
  [Architecture](./reference/architecture.md) overview.

Status claims in this documentation use a deliberately conservative vocabulary —
`Done`, `Partial`, and `Not shipped` — anchored to the canonical
`docs/capability-matrix.toml` in the repository. The [FAQ](./faq.md) answers the
most common questions.
