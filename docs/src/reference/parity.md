# MO2 parity & capability audit

modde does not try to be a clone of [Mod Organizer 2](https://github.com/ModOrganizer2/modorganizer). It targets a different
platform story — Linux-first, declarative, reproducible — and ships some things MO2 has never had (git-backed save vaults,
declarative FOMOD, native Wabbajack on Linux). This page is the honest accounting of how the two overlap.

It is intentionally conservative:

- **`Done`** means the feature is shipped end to end and reachable by users.
- **`Partial`** means core logic exists, but the UX, integration, or production safety is still incomplete.
- **`Not shipped`** means the repository should not market it as available yet.

The canonical status baseline lives in [`docs/capability-matrix.toml`](https://github.com/caniko/rs-modde/blob/trunk/docs/capability-matrix.toml).
The guardrail test `crates/modde-core/tests/repo_truth_tests.rs` fails the build if this page, the README, the
[supported games table](../games/supported-games.md), and the landing-site comparison drift from that file.

## Feature baseline

| Capability | Status | Notes |
| ---------- | ------ | ----- |
| Core VFS deployment | `Done` | Symlink-farm deployment, per-file hiding, atomic rollback, and conflict resolution are shipped. See [Deployment & VFS](../guides/deployment.md). |
| Profile switching & save vaults | `Done` | Profile activation, the experiment stack, git-backed save history, and save fingerprints ship for games with real save trackers. |
| Bethesda plugin management | `Done` | Plugin-order backup/restore, `plugins.txt` IO, LOOT parsing, Form 43 detection, and missing-master checks are shipped. |
| Nexus install pipeline | `Done` | API-key auth, browse/search (REST + GraphQL), update checks, `nxm://`, single-mod installs, and Collections are shipped. |
| FOMOD | `Done` | Interactive wizard plus declarative TOML/JSON/Nix config generation and non-interactive application are shipped. |
| Executable management | `Done` | Named executables with arguments, working directory, environment, Wine DLL overrides, and a configurable output (overwrite) mod, plus overwrite capture on run — in both the CLI (`modde exec` / `modde tool add-executable`) and the UI. See [Executables & external tools](../guides/executables.md). |
| Patcher pipelines | `Partial` | CLI and Home Manager can configure required deployment-time Synthesis/command stages, import generated output mods, and redeploy. GUI editing and Rust-native patchers are future work. See [Patcher pipelines](../guides/patcher-pipelines.md). |
| Hot-deploy | `Partial` | Experimental Cyberpunk-first path-level VFS patching for cosmetic-only mod toggles. It updates the live symlink deployment without promising that the running game reloads every touched asset immediately. |
| Starfield save tracking | `Done` | Starfield `.sfs` files are captured by the save tracker; the record-level save-contamination removal gate is not implemented. |
| Instance switching | `Partial` | CLI/runtime instance selection changes the active data root, but there is no MO2-style portable/global UX yet. |
| Diagnostics | `Partial` | The CLI and UI share a real diagnostics engine fed by resolved conflicts and plugin order, but there is no MO2-style "problems" button with guided fixes. |
| Data tab | `Partial` | The UI renders real conflict rows from the resolver/collision engine, but it is not yet a full merged-VFS browser with archive/hidden filters. |
| Downloads UI | `Partial` | The Downloads view is wired to real queue state, but pause/resume is UI-side state rather than a durable transport-level pipeline. |
| Tool management | `Partial` | The Tools view loads DB-backed tool configs and can apply/revert tracked file patches for MangoHud, vkBasalt, GameMode, ReShade, OptiScaler, and Proton, but the catalog is narrower than MO2's plugin ecosystem. |
| Mod information dialog | `Partial` | A right-rail panel shows Nexus metadata, the image gallery, and endorse/track toggles, but there is no MO2-style file-tree / INI editor / conflict-tab dialog. |
| Non-Nexus download backends | `Partial` | GitHub, Direct, Google Drive, MEGA, and MediaFire backends exist and are exercised by Wabbajack/directive workflows, but they are not yet first-class `install mod` UX. |
| BAIN | `Partial` | Detection and execution scaffolding exist, but the interactive sub-package selection flow is unfinished. |
| Generic game support | `Partial` | User-defined games ship through `modde game add` / import-export of a `GameSpec` TOML and a generic loader, but without a polished UI or per-game scanner/save-tracker. See [Generic & user-defined games](../games/generic-games.md). |

## Supported games

Depth varies by game. `Done` titles are the strongest end-to-end paths; `Partial` titles deploy and track saves but have not been hardened to the same degree.

| Game | Status | Notes |
| ---- | ------ | ----- |
| Skyrim Special Edition / Anniversary Edition | `Done` | Plugins, LOOT, diagnostics, VFS, saves, and Wabbajack/Nexus workflows are the strongest path today. |
| Fallout 4 | `Done` | Plugins, diagnostics, VFS, and save tracking are shipped. |
| Cyberpunk 2077 | `Done` | REDmod / CET / TweakXL-aware install and launch flows are shipped. |
| Fallout 76 | `Partial` | VFS, plugin handling, and BA2 scanning exist; saves are effectively server-side and only lightly represented locally. |
| Starfield | `Partial` | Game plugin, plugin handling, VFS, diagnostics, and `.sfs` save capture exist; record-level save-contamination gating does not. |
| Stellar Blade | `Partial` | UE4/UE5-style deployment, scanning, conflicts, OptiScaler integration, and local save tracking are present, below the Bethesda/Cyberpunk depth. |
| Baldur's Gate 3, Stardew Valley, Fallout: New Vegas, Oblivion, Oblivion Remastered, Bannerlord, The Witcher 3, Subnautica 2 | `Partial` | Engine-appropriate deployment, scanning, conflict classification, and save tracking ship; depth and game-specific polish are still maturing. |

See [Supported games](../games/supported-games.md) for the full per-game table, IDs, and per-game guides.

## Where MO2 is still ahead

These are the gaps that matter most for a "drop-in MO2/Vortex replacement" claim:

1. **Mod information dialog** — file tree, text/INI editing, image preview, conflict tabs, and optional per-mod plugin management.
2. **Durable download pipeline** — transport-level pause/resume/cancel, ETA/speed, concurrency, update-all, and resumable metadata sidecars (modde has resumable sidecars for Wabbajack/direct downloads, but not yet a unified interactive queue).
3. **Full merged-VFS browser** — archive visibility, overwrite actions, hidden-file filters, and "go to the conflicting mod" navigation in the Data tab.
4. **Breadth of tool/plugin ecosystem** — MO2's plugin surface (preview, checker, and integration plugins) is much wider than modde's curated overlay set.

## Where modde is ahead

Features with no MO2 equivalent: git-backed save vaults with full history and branching, SHA-256 save fingerprinting with pre-restore compatibility warnings, the non-destructive experiment stack, declarative FOMOD configs, native Wabbajack installation on Linux, Nexus Collections, declarative home-manager profiles, profile forking that clones save history, stock-game tree-hash verification, and unified Steam + Heroic (GOG/Epic/sideload) detection. See the [landing-site comparison](https://modde.tartanoglu.com/comparison/) for the side-by-side.
