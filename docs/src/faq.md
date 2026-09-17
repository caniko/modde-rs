# FAQ

Short answers to the questions that come up most. Each links to the deep-dive
page that backs the claim, so you can verify anything that matters to you. For
the conservative, test-coupled accounting of what is `Done` vs `Partial`, see the
[MO2 parity & capability audit](reference/parity.md).

## What is modde, and who is it for?

modde is a **cross-platform game mod manager** written in Rust, running natively
on Linux, macOS, and Windows. It deploys mods through a virtual-filesystem symlink
farm, resolves conflicts, manages Bethesda plugin order, installs FOMOD and
Wabbajack lists, talks to Nexus, and keeps git-backed save vaults — with both a
CLI and a GUI.

It is for players who want a real Mod Organizer 2 / Vortex workflow on their own
OS, scriptable from the command line, with git-backed save history. If you use
Nix, you also get an optional declarative, reproducible path via the
home-manager module.

## How is it different from Mod Organizer 2 / Vortex?

modde is not a clone. It runs natively on Linux, macOS, and Windows, and ships
things MO2/Vortex never had: git-backed save vaults with history, declarative
FOMOD, native Wabbajack on Linux, and — for Nix users — declarative home-manager
profiles. MO2 is still ahead on a few fronts (the rich mod-information dialog, a
fully interactive download queue, a complete merged-VFS browser). The full,
intentionally conservative side-by-side — including what is `Done`, `Partial`, and
not yet shipped — lives in the
[MO2 parity & capability audit](reference/parity.md).

## Do I need NixOS to run modde?

No. modde does not require NixOS, but the live user-facing install path today is
Nix/Home Manager or a source build from the Nix development shell. Other
package-manager channels are wired or staged, and macOS/Windows builds are
experimental CI outputs until the [Installation](getting-started/installation.md)
page marks those channels live.

## Does it run on Windows or macOS?

Experimental builds exist for both, but Linux is the live support target today.
macOS tarballs are ad-hoc signed and not notarized; Windows artifacts are
prepared for Authenticode signing. Treat both as staged until the
[macOS](getting-started/installation.md#macos) and
[Windows](getting-started/installation.md#windows) sections mark their channels
live.

## Do I need Nexus Premium?

Only for **automated CDN downloads**. A free Nexus account and API key give you
browse, search, update checks, metadata, endorsements, and `nxm://` handling.
Premium adds the ability to fetch files directly from the Nexus CDN without the
manual browser step — which is what makes hands-off Wabbajack and Collection
installs possible. Without Premium you can still install, you just confirm some
downloads in the browser. See the [Nexus integration guide](guides/nexus.md).

## Can I install Wabbajack lists on Linux without a Windows VM?

**Yes.** modde installs `.wabbajack` lists **natively on Linux** — no Windows VM,
no Wine-hosted Wabbajack client. It parses the manifest, runs the directives, and
deploys through the same VFS engine as everything else, using its own download
backends (Nexus, GitHub, Direct, Google Drive, MEGA, MediaFire) for the archives.
See the [Wabbajack guide](guides/wabbajack.md).

## How do I move my saves and profiles to another machine?

modde has no cloud sync, but its on-disk layout is built for it. The reliable
pattern is two parts:

- **Saves:** each game's save vault under `saves/<game_id>/` is already a git
  repository — add a remote and push it, then clone or pull on the other machine.
  Your save snapshots and their mod fingerprints ride along.
- **Profiles:** declare them in home-manager (`programs.modde.profiles`) and
  rebuild on the second machine, so the profile is *reproduced* from pinned inputs
  rather than copied. If you work imperatively, export profiles as TOML and `modde
  import` them.

Do **not** rsync `modde.db` or the large derivable caches. The full procedure,
including what not to sync, is in
[Data, instances & backups](guides/data-management.md#multi-machine-sync).

## What games are supported?

15 games ship with built-in support. Depth varies by title — some are `Done`
end-to-end, others are `Partial`. The canonical per-game table, with IDs and
links to each game's guide, is on the
[Supported games](games/supported-games.md) page.

## Is executable management supported now?

**Yes** — this is `Done`. You can save named launch targets (xEdit, BodySlide,
Nemesis, the Creation Kit, LOOT, …) with arguments, a working directory,
environment variables, and Wine DLL overrides, then run them with **overwrite
capture** so anything the tool writes lands in a mod you control. It works from
both the CLI (`modde exec ...` / `modde tool add-executable ...`) and a GUI
Executables view. See [Executables & external tools](guides/executables.md).

## Can I add a game that is not built in?

**Yes, with caveats — this is `Partial`.** The generic-game path lets you
register an arbitrary title with a small *GameSpec* TOML via `modde game add`.
You get the engine-agnostic parts (VFS deployment, conflict detection, launcher
integration, executable management), but **not** a bespoke filesystem scanner or
save tracker. Treat it as "modde can deploy and launch mods for this game", not
"modde fully understands this game". See
[Generic & user-defined games](games/generic-games.md).

## Where does modde store its data?

In a single data directory plus a small config file, following XDG on Linux —
by default `~/.local/share/modde/` (database, mod store, profiles, downloads,
stock snapshots, save vaults). You can relocate it wholesale with `--data-dir` /
`MODDE_DATA_DIR`, or run several isolated named **instances**. The full on-disk
layout is documented in [Data, instances & backups](guides/data-management.md).

## Is there any telemetry?

**Opt-in only. Normal builds send nothing.** Remote code is gated behind the
non-default `remote-telemetry` feature; default builds do not include it. Even
with that feature, no data is transmitted unless you explicitly configure the
relevant endpoint and opt-in environment variables.

Crash telemetry and compatibility reporting are separate. Crash telemetry uses
`MODDE_TELEMETRY_ENDPOINT` plus `MODDE_TELEMETRY_TOKEN` (legacy `RS_MODDE_TELEMETRY_*` names still work) for modde process
crash capture. Compatibility oracle reporting uses
`MODDE_COMPAT_ORACLE_OPT_IN=1` plus `MODDE_COMPAT_ORACLE_ENDPOINT`, and uploads
only derived hashes from local crash correlation: game id, coarse platform,
hashed mod identities, hashed pair keys, hashed mod set, and hashed crash
signature. It does not upload raw crash logs, paths, profile names, display
names, plugin names, Nexus tokens, usernames, or install IDs. The oracle service
returns only thresholded aggregate statistics.

## See also

- [Installation](getting-started/installation.md) — every channel and its commands
- [Quick Start](getting-started/quick-start.md) — define and deploy your first profile
- [MO2 parity & capability audit](reference/parity.md) — `Done` vs `Partial`, by feature and game
- [Supported games](games/supported-games.md) — the full per-game table
- [Generic & user-defined games](games/generic-games.md) — register a title modde does not ship
- [Executables & external tools](guides/executables.md) — named launch targets with overwrite capture
- [Wabbajack lists](guides/wabbajack.md) — native list installs on Linux
- [Data, instances & backups](guides/data-management.md) — storage layout and multi-machine sync
