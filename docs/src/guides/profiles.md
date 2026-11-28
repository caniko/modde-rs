# Profile Management

## What a profile is

A **profile** groups four things for one game:

1. **An ordered mod set** — the list of `EnabledMod` entries (each with an `enabled` toggle, version, Nexus metadata, category, tags, notes, and an optional per-mod lock). Position in the list *is* the install priority: later mods win file conflicts. See [Conflicts & priority](conflicts.md).
2. **A load order** — the mod ordering above, plus an independent **plugin order** (`.esp`/`.esm`/`.esl`) for engines that distinguish the two, plus declarative `load_order_rules` (`load_after` / `load_before` / `incompatible`).
3. **Save assignments** — a branch in the game's git-backed save vault, swapped automatically whenever the active profile changes. See [Save management](saves.md).
4. **Deployment state** — the `overrides` directory and the per-mod file manifest that records exactly what was staged, so deployment and uninstall are precise. See [Deployment](deployment.md).

modde stores one row per profile in its SQLite database (`modde.db`). The
`(name, game_id)` pair is unique, so Skyrim's `default` and Fallout 4's
`default` don't collide. Each profile records **provenance** (`ProfileSource`:
`Manual`, `NexusCollection`, or `Wabbajack`) and an optional **load order lock**.

See [Data, instances & backups](data-management.md) for where modde stores
profiles, the database, saves, and other state.

## Creating profiles

### Via home-manager (declarative)

```nix
programs.modde.profiles.my-skyrim = {
  game = "skyrim-se";
};
```

For Wabbajack profiles, Home Manager can also manage the first install. If the
game is not installed yet, keep the profile in an awaiting state:

```nix
programs.modde.profiles.my-skyrim = {
  game = "skyrim-se";
  installMode = "await-game";
  wabbajackList = {
    url = "https://example.com/modlist.wabbajack";
    hash = "sha256-...";
  };
};
```

Use `wabbajackList.path` instead when the `.wabbajack` file is already present
locally or in the Nix store.

After installing the game through Steam or Heroic, set `gameDir` and switch back
to the default `installMode = "auto"`. The full option set lives in the
[Home-Manager module reference](../configuration/hm-module.md).

### Via CLI (imperative)

```bash
modde profile create my-skyrim --game skyrim-se
```

Before writing to the database, modde rejects empty names, names over 255
characters, and names with filesystem-unsafe characters (`/ \ NUL : * ? " < > |`).
The name doubles as a directory under
`<modde_data>/profiles/<name>/`.

## Listing, inspecting, and switching

```bash
# List all profiles (grouped by game)
modde profile list

# List profiles for a specific game
modde profile list --game skyrim-se

# See which profile is active (and the current experiment depth)
modde profile active --game skyrim-se

# Switch to a profile (automatically swaps saves)
modde profile switch my-skyrim --game skyrim-se
```

`profile list` shows each profile's name, game, mod count, and source type.
`profile active` also reports **experiment depth**: whether you're on a
`try`/rollback chain and how deep it goes. See [Experiment stack](#experiment-stack).

### What switching actually does

When you switch profiles, modde:

1. Captures the **current** profile's saves into its vault branch (with a mod
   fingerprint embedded for later compatibility warnings).
2. Restores the **target** profile's saves from its branch.
3. Sets the target as the active profile for that game.

If existing saves are detected on disk with **no active profile** assigned (you
modded the game before adopting modde), the switch reports an *adoption required*
state instead of silently overwriting anything — adopt them first with
`modde save adopt` (see [Save management](saves.md)).

## Forking profiles

`fork` creates a full clone of a profile — mods, load order rules, locks, and a
forked save branch — under a new name. The source profile is never modified.

```bash
modde profile fork my-skyrim my-skyrim-experimental --game skyrim-se
```

A fork is a faithful copy by default. If the source has a profile-level load
order lock (for example, from a Wabbajack list), the fork inherits that lock
and every per-mod pin. Use this to keep a known-good setup as a backup.

Pass `--unlock` to make a freely editable copy:

```bash
modde profile fork my-skyrim my-skyrim-custom --game skyrim-se --unlock
```

`--unlock` strips both the profile-level `load_order_lock` and every per-mod pin
from the new profile, so you can reorder it however you like. The source remains
locked and untouched. This is the canonical "I want to experiment on top of a
Wabbajack list without breaking the authoritative ordering" workflow.

## Load order locking

Reordering a mod list that came from an authoritative source (a Wabbajack
manifest, a Nexus Collection, or a shared TOML export) usually breaks it. modde
models this with a **lock**, which comes in two granularities: a whole-profile
lock and a per-mod pin.

### Automatic locks

A profile-level lock is applied automatically when a profile is created from an
authoritative source, recording *why* in a structured `LockReason`:

| Source | `LockReason` | Provenance recorded |
| ------ | ------------ | ------------------- |
| Wabbajack modlist | `Wabbajack` | `manifest_hash` (verifies which manifest) |
| Nexus Collection | `NexusCollection` | `slug` + `version` |
| TOML import (`modde import`) | `TomlImport` | `source_path` at import time |

If the imported TOML carries a lock (for example, one exported from a Wabbajack
profile), modde preserves that lock and its original provenance; otherwise it
adds a `TomlImport` lock. Exporting and reimporting a locked profile
through TOML preserves its origin.

Provenance (`ProfileSource`) is metadata only; the lock (`LoadOrderLock`)
controls reorder enforcement. Forking with `--unlock` clears the lock but keeps
provenance.

### Manual locks

Lock any profile manually to protect a hand-tuned load order:

```bash
# Lock a profile with an explanatory note
modde profile lock my-skyrim --note "tested and stable"

# Check lock status (shows the LockReason and the locked-at timestamp)
modde profile lock-info my-skyrim

# Unlock
modde profile unlock my-skyrim
```

A manual lock records `LockReason::Manual { note }` and an ISO-8601 UTC
`locked_at` timestamp.

### Per-mod pins

Sometimes only a handful of mods are order-sensitive (a patch that must sit right
after its parent, say). Pin those individually while leaving the rest of the list
freely reorderable:

```bash
# Pin a mod in place (per-mod lock)
modde profile lock-mod my-skyrim "skyrim_12345_67890" --note "load order sensitive"

# Release the pin
modde profile unlock-mod my-skyrim "skyrim_12345_67890"
```

When a profile is locked at the profile level you can still **add** new mods —
they append to the end of the list — but you cannot reorder or remove locked
mods. Per-mod pins protect individual entries even on an otherwise unlocked
profile.

### The reorder enforcement model (`ReorderError`)

One function handles every one-step move in the UI and CLI. It refuses the
move at the first matching condition below and leaves the profile untouched:

| Order | Condition | Refusal (`ReorderError`) |
| ----- | --------- | ------------------------ |
| 1 | The whole profile is locked | `ProfileLocked { reason }` |
| 2 | The target `mod_id` isn't in the profile | `ModNotFound { mod_id }` |
| 3 | The target mod itself carries a pin | `ModPinned { mod_id, reason }` |
| 4 | The move would run off the top/bottom of the list | `AtBoundary` |
| 5 | The swap partner (the neighbor one step over) is pinned | `AdjacentPinned { neighbor_id, reason }` |

Moving unpinned mod **A** past pinned neighbor **B** would shift **B**, so
modde refuses the move. Structured fields such as the offending id and
`LockReason` let the UI or CLI explain the refusal without matching error text.

## Portable `modde.lock` exports

`modde.lock` is a signed JSON snapshot of one profile's reproducible state:
profile provenance, mod priority order, Nexus/Wabbajack source identities,
tracked staged files with content hashes, plugin order, hidden files, patcher
configuration and outputs, and managed tool output files.

```bash
modde lock export --profile my-skyrim --game skyrim-se --output modde.lock
modde lock keygen --public modde-lock.pub.json --secret modde-lock.secret.json
modde lock sign modde.lock --secret-key modde-lock.secret.json
modde lock verify modde.lock --profile my-skyrim --game skyrim-se
```

Export is strict by default. If a mod was installed before modde recorded source
archive hashes or staged file manifests, export fails and prints the missing
artifact, why it is required, how to regenerate it, and the validation command.
Use `--allow-incomplete` only for a diagnostic lock that explicitly sets
`reproducible = false`; incomplete locks are not byte-for-byte replay promises.

`modde lock import` verifies signatures and imports profile metadata, but v1
does not silently download or fabricate missing archives. Re-run the normal
Nexus/Wabbajack install flows to materialize files from the source identities in
the lock.

## Experiment stack

The experiment stack lets you try profile changes non-destructively, like git
branches for your mod setup. `try` pushes the currently-active profile onto a
per-game stack and activates a different one; `rollback` pops back one level;
`commit` accepts wherever you've landed and clears the history.

```bash
# Push the current profile onto the stack and activate "experimental-skyrim"
modde profile try experimental-skyrim --game skyrim-se

# Try another on top (the stack grows)
modde profile try ultra-graphics --game skyrim-se

# Something broke? Roll back one level
modde profile rollback --game skyrim-se

# Happy with where you are? Commit — clears the whole stack
modde profile commit --game skyrim-se
```

### How depth works — a worked example

The **experiment depth** is the number of entries on the stack. `profile
active` reports it. Start with profile **A**
active and depth `0`:

| Command | Stack (bottom → top) | Active | Depth |
| ------- | -------------------- | ------ | ----- |
| *(start)* | `[]` | **A** | 0 |
| `try B` | `[A]` | **B** | 1 |
| `try C` | `[A, B]` | **C** | 2 |
| `rollback` | `[A]` | **B** | 1 |
| `rollback` | `[]` | **A** | 0 |
| `try B` | `[A]` | **B** | 1 |
| `commit` | `[]` | **B** | 0 |

The two important rules:

- **`rollback` from depth 0 is an error** (`NotInExperiment`). There is nothing on
  the stack to pop back to — you are not in an experiment.
- **`commit` requires depth ≥ 1.** It accepts the *current* active profile and
  throws away the rollback history; it does **not** revert anything. In the table
  above, the final `commit` leaves **B** active and the stack empty.

Each `try` and each `rollback` swaps saves along with the profile, exactly like a
plain `switch`, and embeds the mod fingerprint in the capture so later restores
can warn about mismatches. See [Save management](saves.md) for the fingerprint
mechanics.

> Forgetting to `commit` is harmless — you simply stay "in an experiment" with a
> non-zero depth. The only consequence is that `rollback` remains available until
> you commit. The stack is per game, so experimenting in Skyrim never touches your
> Fallout 4 state.

## Categories and organization

Each profile can group its mods into **categories** (collapsible separators with
an optional color), and each mod row carries **notes** and **tags** for filtering
and export:

- `mod_categories(name, color, sort_index)` — named, ordered, optionally colored
  groups. Deleting a category does **not** delete its mods; their `category_id` is
  simply nulled out (they become uncategorized).
- `EnabledMod.category_id` — the category a mod belongs to.
- `EnabledMod.notes` — free-text per-mod notes.
- `EnabledMod.tags` — a list of strings (stored as a JSON array) for filtering.

This mirrors MO2's separator/category model. The data layer also keeps **mod
priority** (install order — which files win, driven by list position) and
**plugin order** (which plugin loads in the engine, stored independently per
profile) as two distinct orderings, the same left-pane/right-pane split MO2 uses.
For the precise feature-by-feature comparison against MO2 — including the
organization features that are tracked in the schema but not yet surfaced in the
UI — see the [Parity audit](../reference/parity.md).

## Deduplication

When you combine a Wabbajack install with a filesystem scan (see
[Scanning](scanning.md)), the scanner can re-detect mods the Wabbajack manifest
already installed, leaving duplicate rows in the profile. The `dedup` command
finds and optionally removes those leaked rows. It runs in two layers:

**Layer 1 — heuristic (no `--manifest`).** A read-only report. On a locked
profile it lists filesystem-scanner rows whose ids look manifest-owned
(`cet/*`, `reds/*`, `tweak/*`, `archive/*`, `redmod/*`) as *suspects*. It never
deletes anything.

```bash
# Heuristic suspects only — pure dry-run
modde profile dedup my-modlist --game cyberpunk2077
```

**Layer 2 — manifest classification (`--manifest <path>`).** Uses the
`.wabbajack` file's install directives as the authoritative reference to classify
each suspect as **LEAKED** (the manifest installs it, so the scanner row is a
duplicate — safe to delete) or **GENUINE** (a real user addition the manifest
does *not* cover — keep it).

```bash
# Classify against the manifest, but don't delete (dry-run report)
modde profile dedup my-modlist --manifest /path/to/modlist.wabbajack

# Actually delete the rows classified as LEAKED
modde profile dedup my-modlist --manifest /path/to/modlist.wabbajack --apply
```

`--apply` is the only thing that mutates the profile, and it only removes rows
classified **LEAKED** — GENUINE user additions are always preserved.

## Deleting profiles

```bash
modde profile delete my-skyrim
# or, when the name is ambiguous across games:
modde profile delete my-skyrim --game skyrim-se
```

Deleting a profile row removes all dependent records via `ON DELETE CASCADE`:
its `profile_mods`, `load_order_rules`, assigned `saves`
rows, `hidden_files`, `plugin_order`, `mod_categories`, `installed_mod_files`
manifest, and any `experiment_stack` / `active_profiles` entries that pointed at
it.

> What delete does **not** touch: the content-addressed mod **store**, the
> downloads cache, the stock vanilla snapshot, or the **git save vault** on disk.
> The save vault is the authoritative history of your saves and is intentionally
> preserved — deleting a profile does not throw away its save snapshots. Manage
> those through the data directory (see [Data, instances & backups](data-management.md)).

If a name exists for more than one game, the command errors with an
*ambiguous profile* message listing the candidate games; disambiguate with
`--game`.

## Decision tree: lock, pin, or fork?

Use this to decide how to protect (or escape) a load order:

```
You have a working, order-sensitive setup. What do you want to do?
│
├─ Keep it frozen exactly as-is, but keep using it
│   └─ A few mods are order-sensitive, rest is free  → lock-mod (per-mod pins)
│   └─ The whole order matters                        → profile lock (--note)
│
├─ It came from Wabbajack / a Collection / a TOML import
│   ├─ I just want to RUN it, never edit it           → leave the automatic lock on
│   ├─ I want to tweak a FEW things on top of it       → fork --unlock, then edit the fork
│   └─ I want a safety copy before I touch anything     → fork (faithful, keeps the lock)
│
├─ I want to A/B test changes and bail if they break  → try … / rollback … / commit
│
└─ I'm done hand-tuning and never want to nudge it again → profile lock --note "final"
```

Rules of thumb:

- **Lock** when *you* are the source of truth and want to stop *future you* from
  fat-fingering the order. Unlock when you genuinely need to edit.
- **Pin** (`lock-mod`) when only specific mods are fragile and you still want to
  reorganize everything else.
- **Fork** when you want to *diverge* from an authoritative list — use
  `--unlock` to start editable, or a plain fork to keep a frozen safety copy.
- **Experiment** (`try`/`rollback`) when you want reversible A/B testing of
  whole-profile swaps without committing to a fork.

## See also

- [Save management](saves.md) — git-backed vaults, fingerprinting, and the auto-swap that rides along with every switch/try/rollback
- [Deployment](deployment.md) — how the active profile's mods reach the game directory
- [Conflicts & priority](conflicts.md) — what "load order" means for file overrides
- [Wabbajack lists](wabbajack.md) — installing the locked profiles `dedup` and `--unlock` exist for
- [Scanning](scanning.md) — the filesystem detection that `dedup` reconciles against a manifest
- [Data, instances & backups](data-management.md) — where profiles, the database, and save vaults live on disk
- [Playing with modde](playing.md) — deploy and launch the active profile
- [Parity audit](../reference/parity.md) — the precise `Done` / `Partial` status of every profile and organization feature
- [Home-Manager module reference](../configuration/hm-module.md) — declaring profiles in Nix
