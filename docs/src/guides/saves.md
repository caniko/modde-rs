# Save Management

## Overview

modde manages save files with a **git-backed save vault**: one git repository per game, one branch per profile. Every capture is an ordinary git commit, so you get the full power of git for free — complete history, branching, snapshots, and the ability to roll a profile's saves back to any previous point. On top of plain git, modde adds a **mod fingerprint** so it can warn you before you restore a save that was made under a different set of save-breaking mods.

The whole system is built so you can run multiple profiles of the same game side by side — a vanilla playthrough, a heavily modded one, a testbed — without their saves ever colliding, and without losing the ability to go back.

## The vault layout

There is one vault per game, living under modde's data directory:

```
<data>/saves/<game_id>/
    .git/            # full git history; this is a real git repo
    <save files>     # the working tree for the currently checked-out profile branch
```

On Linux `<data>` is `~/.local/share/modde` by default, so a Skyrim SE vault lives at `~/.local/share/modde/saves/skyrim-se/`. The path is resolved internally by `save_vault_dir(game_id)`; you never have to construct it by hand.

Key facts about the vault:

- **One repository per game.** All profiles for a game share the same repo and differ only by branch.
- **One branch per profile.** The branch name is the profile name with git-unsafe characters (` ~ ^ : ? * [ \`) replaced by `-`. So a profile named `My Skyrim` becomes the branch `My-Skyrim`.
- **`main` is the root.** When a vault is first created, modde makes an empty `init save vault` commit on `main`. Profile branches are cut from there.
- **The working tree is whatever profile is checked out.** modde checks out the relevant branch (force checkout) before any capture, deploy, or restore, so the files on disk always belong to exactly one profile.

### It's plain git — go ahead and inspect it

The vault is not a custom binary format. It is a normal git repository, and modde uses [`git2`](https://docs.rs/git2/) (libgit2) under the hood. You can point any git tooling at it to audit what modde has stored:

```bash
cd ~/.local/share/modde/saves/skyrim-se/
git branch          # one branch per profile
git log --oneline   # the snapshot history of the checked-out branch
git show <commit>   # exactly what changed in a snapshot, including the fingerprint trailers
```

Treat it as read-only-ish: modde owns the working tree and will force-checkout over local edits. Inspecting, logging, and diffing are safe; hand-committing into it is not recommended.

## Save-breaking mods and the fingerprint

A **save-breaking mod** is a mod whose presence (or absence) can make a save game incompatible — typically anything that changes the game's serialized data: script extenders and their plugins, mods that add scripts or records baked into saves, framework mods, and so on. Cosmetic mods (textures, reshades, UI tweaks) are *not* save-breaking: adding or removing them does not invalidate a save.

Each game plugin classifies its own mods. modde does not hard-code a list in the save layer; instead `SaveFingerprint::compute` takes a `classify` callback so the save vault stays independent of game-specific logic — the caller resolves the game plugin and asks it whether each mod is save-breaking.

### How the fingerprint is computed

The fingerprint is a SHA-256 hash over **only the enabled, save-breaking mods** of the profile:

1. Take every mod in the profile that is **enabled** *and* classified **save-breaking**. Disabled mods and cosmetic mods are ignored entirely.
2. Collect their mod IDs, sort them, and de-duplicate.
3. Feed each ID (followed by a `\0` separator) into SHA-256.
4. The hex digest is the fingerprint; the first 12 hex characters are used for display.

Because the input is sorted and de-duplicated, **two profiles with the same set of enabled save-breaking mods produce the same fingerprint**, regardless of install order or how many cosmetic mods differ between them. A profile with no save-breaking mods has the canonical "empty" fingerprint.

### Where the fingerprint is stored

When modde captures saves, it embeds the fingerprint as git commit trailers in the snapshot's commit message:

```
capture saves for profile 'my-skyrim'

Mod-Fingerprint: a1b2c3d4e5f6
Save-Breaking-Mods: skse, ussep, immersive_armors
```

`Mod-Fingerprint` carries the short hash; `Save-Breaking-Mods` is the human-readable list (omitted when there are no save-breaking mods). On restore, modde reads these trailers back out to compare against your current profile.

## Capturing saves

A capture copies the live save files into the vault working tree and commits them on the profile's branch. modde skips the commit entirely if the tree is byte-identical to the last snapshot, so repeated captures with no new saves don't clutter history.

Saves are captured automatically in several situations:

- **On profile switch** — `profile switch`, `profile try`, and `profile rollback` capture the *outgoing* profile's saves before swapping (see [Profile-integrated swapping](#profile-integrated-save-swapping)).
- **On game exit after `modde play`** — unless you pass `--no-capture`. See [Playing a Game](playing.md).
- **Via the launch wrapper** — for Steam launches, modde's generated launch wrapper runs `modde save auto-capture` when the game process exits.
- **On demand** with `save auto-capture`, which detects new saves and commits them.
- **Continuously** with `save watch`.

You can also capture manually at any time:

```bash
modde save capture --game skyrim-se --profile my-skyrim -m "before dragon fight"
```

The optional `-m`/`--message` text is recorded as the commit message; the fingerprint trailers are appended automatically.

### Auto-capture on game exit

```bash
modde save auto-capture --game skyrim-se
```

This is the hook the launch wrapper calls. With no `--profile`, it captures into the game's active profile. It also runs the game plugin's save tracker to produce a descriptive commit subject (for example `capture: Lydia — Save 14 [manual]`), so history reads like a save list rather than a string of identical messages.

### Continuous watching

For games you launch outside of `modde play`, run a watcher that captures as you save:

```bash
modde save watch --game skyrim-se --interval 60
```

`--interval` is the poll interval **in seconds** (default `30`). With `--profile` omitted, the watcher captures into the active profile. It watches the save directory for changes and commits a new snapshot whenever the saves change. Leave it running in a terminal alongside the game.

## Browsing history

```bash
modde save history --game skyrim-se --profile my-skyrim --limit 10
```

`--limit` defaults to `20`. Each entry is a snapshot (a git commit) showing the short commit ID, timestamp, a parsed title (character name + save label where the tracker could extract them), the file count, and the mod fingerprint when present. The short ID is what you pass to `save restore`.

## Restoring saves

Restore the saves from a specific snapshot back onto your live save directory:

```bash
modde save restore --game skyrim-se --profile my-skyrim <commit>
```

`<commit>` is a snapshot ID (or unique prefix) from `save history`. modde checks out the profile branch, resets it to that commit, clears the active live saves (preserving Steam Cloud and parked metadata — see below), and copies the snapshot's files back to the game.

### Pre-restore compatibility warnings

Before restoring, modde compares the snapshot's stored fingerprint against your **current** profile's fingerprint and reports one of:

| Result | Meaning |
| ------ | ------- |
| **Compatible** | Fingerprints match — the same save-breaking mods are present. Safe to restore. |
| **No fingerprint** | The snapshot predates fingerprinting (no trailer). Restore at your own risk. |
| **Mismatch** | The save-breaking mods differ. modde lists which mods were **added** to your current profile and which were **removed** since the snapshot. |

A mismatch is a warning, not a hard block: you may legitimately want to restore an older save and then re-add the mods. But restoring a save under a *different* set of save-breaking mods is the classic recipe for corrupted or broken saves, so read the diff before you proceed. modde also nudges you toward `modde save history` to find a snapshot whose fingerprint matches your current setup.

## Adopting existing saves

If you already have save files from before you set up modde, adopt them into a profile so they become the first snapshot on that profile's branch:

```bash
modde save adopt --game skyrim-se --profile my-skyrim
```

This ensures the profile's branch exists and captures every existing save in the game's save directory as the initial snapshot. If no profile is active for that game yet, the adopted profile is also made **active**, so that the next profile switch can safely park those saves rather than treating them as orphaned.

This adoption flow is also what protects you during a normal profile switch. If modde finds saves in the game directory but no profile is active, it refuses to overwrite them and tells you to adopt first — see the `AdoptionRequired` path in [Playing a Game](playing.md).

## Steam Cloud handling

Profile-managed launches require Steam Cloud disabled for the entitlement and a
saved per-installation `steam_cloud_disabled: true` assertion. Restart Steam after
changing its cloud setting. Preserving cloud metadata below does not coordinate
Steam's external writes with profile switching. Heroic likewise needs explicit
`autoSyncSaves: false` and a restart; see the [playing guide](playing.md).

modde does **not** treat the live save directory as disposable. Many games sync their save folder through Steam Cloud, which drops a `steam_autocloud.vdf` marker file there. Blowing that directory away on every profile switch would fight Steam Cloud and risk re-downloading stale saves.

Instead, during a profile switch modde:

1. Captures the outgoing profile's saves into its vault branch (the git history is the authoritative record).
2. **Parks** the outgoing root saves under a modde-owned directory, `.modde/profiles/<profile>/`, *inside the live save folder*. Because the files are moved rather than deleted, Steam Cloud sees them as relocated, not lost.
3. Leaves the `steam_autocloud.vdf` marker (and the `.modde/` directory) untouched.
4. Restores the incoming profile's saves at the root.

So the game only ever sees the active profile's saves at the top of its save directory, while Steam's cloud marker stays put and the git vault remains the source of truth. The `steam_autocloud.vdf` marker and the `.modde/` live-state directory are treated as **live metadata**: modde never copies them into the vault and never deletes them when clearing active saves.

If Steam later downloads stale root saves behind modde's back, just switch profiles again or run `modde save restore` for the snapshot you actually want.

## Profile-integrated save swapping

Save management is wired into the profile system so switching profiles swaps saves automatically. The full activate flow (`activate_with_fingerprint`) does the following when you move from a current profile to a new one:

1. **Capture** the current profile's live saves into its vault branch, embedding the current fingerprint in the commit.
2. **Park** the current profile's root saves under `.modde/profiles/<current>/`, preserving Steam Cloud metadata.
3. **Ensure** the new profile's branch exists (creating it if needed).
4. **Deploy** the new profile's saves from its vault branch to the live save directory root.

This is what makes `modde play <profile>` and `modde profile switch` feel seamless: your saves follow your profile.

### Forking a profile branches its saves

When you fork a profile, modde also forks its save vault branch:

```bash
modde profile fork <source> <new-name> --game skyrim-se
```

Under the hood, `fork_saves` looks up the source profile's branch tip and creates a new branch pointing at that same commit. The fork therefore **starts with a complete copy of the source profile's save history** and then diverges — new captures on the fork land on the fork's branch, new captures on the source land on the source's branch, and neither disturbs the other. This is the git-native answer to "I want to try a risky change but keep my main playthrough safe."

## Cross-profile save loading — be careful

The whole point of per-profile branches is that **a save made under one profile belongs to that profile's mod set**. modde does not stop you from manually pointing one profile at another profile's save (for instance, by restoring a commit from a different branch or hand-copying files), but doing so is exactly the situation the fingerprint exists to warn about.

Loading a save into a profile whose enabled save-breaking mods differ from the ones the save was made with can corrupt the save or crash the game — missing scripts, dangling records, version mismatches. If you must move a save across profiles:

- Prefer `modde profile fork`, which carries the history across cleanly.
- If you restore across branches, **read the fingerprint mismatch report** and reconcile the save-breaking mods first.
- When in doubt, match the mod set to the save rather than the save to the mod set.

## Managing individual save assignments

Separate from the vault, modde keeps a lightweight database table that tracks individual save files by path and links them to a profile with an optional label. This is handy for annotating specific saves without committing a full snapshot.

```bash
# Assign a save file (or directory) to a profile, with an optional label
modde save assign ~/saves/quicksave.ess --profile my-skyrim --label "quick save"

# List the saves assigned to a profile
modde save list --profile my-skyrim

# Find saves in the game directory not yet assigned to any profile
modde save scan --game skyrim-se

# Remove an assignment (the file itself is untouched)
modde save unassign ~/saves/quicksave.ess
```

`save assign` and `save unassign` take the save path as a positional argument. `save scan` reports files in the game's save directory that aren't yet tracked. These assignments are purely metadata — they record *that* a save belongs to a profile; the vault is still what captures and restores the actual bytes.

## Command reference

| Command | Purpose |
| ------- | ------- |
| `save adopt --game G --profile P` | Import existing game-directory saves as the first snapshot |
| `save capture --game G --profile P [-m MSG]` | Create a snapshot on the profile branch |
| `save auto-capture --game G [--profile P]` | Detect and commit new saves (the game-exit / wrapper hook) |
| `save watch --game G [--profile P] [--interval N]` | Poll every `N` seconds (default 30) and capture changes |
| `save history --game G --profile P [--limit N]` | List snapshots (default limit 20) |
| `save restore --game G --profile P <commit>` | Restore a snapshot, with a pre-restore fingerprint check |
| `save assign <path> --profile P [--label L]` | Link a save file/dir to a profile |
| `save unassign <path>` | Remove a save assignment |
| `save list --profile P` | List assigned saves |
| `save scan --game G` | Find unassigned saves in the game directory |

## See also

- [Playing a Game](playing.md) — how `modde play` captures saves on exit
- [Profiles](profiles.md) — switching, trying, rolling back, and forking profiles
- [Supported Games](../games/supported-games.md) — which games ship with save trackers
- [CLI reference](../reference/cli.md) — full command and flag listing
- [Troubleshooting](troubleshooting.md) — recovering from a bad restore or a Steam Cloud fight
