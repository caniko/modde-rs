# Quick Start

modde is Linux-first (macOS and Windows builds are experimental) and gives you two ways to
drive the same engine:

- **Imperative (CLI)** — `modde detect`, `modde profile create`,
  `modde install …`, `modde deploy`, `modde play`. The same commands work on
  every platform. Best for interactive, day-to-day curation where you add and
  reorder mods by hand.
- **Declarative (home-manager)** — if you use Nix, describe a profile in your
  home-manager configuration and let an activation script install and deploy it
  on rebuild. A reproducible, version-controlled option for Wabbajack lists and
  Nexus Collections.

Both paths share one database, one mod store, and one save vault, so you can
start imperatively and pin things down declaratively later (or vice versa)
without losing state.

This page is the fast tour. If you want a single narrated end-to-end run, read
[Your first profile](first-profile.md) instead. For installing modde itself,
see [Installation](installation.md).

## The imperative path (CLI)

The CLI walks the whole lifecycle in explicit steps. A minimal run looks like:

```bash
# 1. See which games modde found across Steam and Heroic
modde detect

# 2. Create a profile for one of them
modde profile create my-skyrim --game skyrim-se

# 3. Add mods to it (pick whichever source applies — see below)
modde install mod https://www.nexusmods.com/skyrimspecialedition/mods/12345 \
  --profile my-skyrim

# 4. Build and deploy the symlink farm into the game directory
modde deploy --profile my-skyrim --game skyrim-se

# 5. Deploy (again) and launch, capturing saves on exit
modde play my-skyrim --game skyrim-se
```

### Detect

```bash
modde detect
```

`modde detect` auto-discovers installed games across Steam and Heroic (GOG,
Epic, Sideload) and prints their ids and install paths. Use the printed id
(e.g. `skyrim-se`) for every subsequent `--game` flag. If your game is not
detected, you can still create a profile and pass paths explicitly, or register
a user-defined game with `modde game add`.

### Create a profile

A **profile** ties a set of mods to a specific game.

```bash
modde profile create NAME --game ID
# e.g.
modde profile create my-skyrim --game skyrim-se
```

Game ids are the short identifiers in the
[supported games](../games/supported-games.md) table (`skyrim-se`,
`cyberpunk2077`, `stellar-blade`, and so on). 15 games ship today; generic,
user-defined games are also possible via a `GameSpec` TOML (see
[Game support](../games/supported-games.md)). See
[Profile management](../guides/profiles.md) for listing, switching, forking,
locking, and the experiment stack.

### Add mods

Pick the source that matches what you are installing:

```bash
# A single Nexus mod (downloads via CDN — needs a Premium key)
modde install mod https://www.nexusmods.com/skyrimspecialedition/mods/12345 \
  --profile my-skyrim

# A mod with a FOMOD installer, driven by a saved choices file
modde install mod <url> --profile my-skyrim --fomod-config my-choices.toml

# A whole Wabbajack modlist
modde install wabbajack /path/to/modlist.wabbajack \
  --profile my-modlist \
  --game-dir "/path/to/Skyrim Special Edition"

# A Nexus Collection
modde install nexus-collection <slug> --profile my-collection
```

Set up Nexus authentication first with `modde nexus auth` (see
[Nexus Mods](../guides/nexus.md)). For FOMOD details — generating a choices
template and inspecting options — see the [FOMOD guide](../guides/fomod.md).

Collections (and Wabbajack lists) auto-lock the profile to preserve the
curator's intended load order. See [Profile management](../guides/profiles.md)
for forking a locked profile into a freely editable copy. Wabbajack lists that
read vanilla game files (Wabbajack `GameFileSourceDownloader` entries — Legends
of the Frost is one example) need `--game-dir` so modde can read and verify
those local files during installation.

### Deploy

```bash
# Deploy the active profile
modde deploy

# Deploy a specific profile
modde deploy --profile my-skyrim --game skyrim-se
```

See [what deploy actually does](#what-deploy-does) below.

### Play

```bash
modde play my-skyrim --game skyrim-se
```

`modde play` switches to the profile (swapping saves), deploys, launches the
game through the detected launcher, and auto-captures saves on exit. Useful
flags: `--no-switch`, `--no-deploy`, `--no-capture`. See
[Playing a game](../guides/playing.md).

## The declarative path (home-manager)

If you use Nix, modde is also a flake with a home-manager module, so you can
**declare your mod profiles as code** and let them deploy on rebuild. This is
one optional path — the CLI above works the same everywhere — but it is handy
for reproducible, version-controlled setups. See [Installation](installation.md)
for adding the flake input and module.

Declared in home-manager, the simplest profile is just a game id:

```nix
programs.modde = {
  enable = true;

  profiles.my-skyrim = {
    game = "skyrim-se";
  };
};
```

After `home-manager switch`, modde deploys your profiles automatically via an
activation script. There is nothing else to run for a plain profile.

### Install from a Wabbajack modlist

To install a [Wabbajack](../guides/wabbajack.md) modlist, add `wabbajackList`
and point `gameDir` at your installed game. modde reads the manifest, downloads
the referenced archives (primarily from Nexus — needs an API key), and deploys
the list.

```nix
programs.modde = {
  enable = true;

  nexus.apiKeyFile = "/run/secrets/nexus-api-key";  # sops-nix compatible

  profiles.living-skyrim = {
    game = "skyrim-se";
    gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
    wabbajackList = {
      url = "https://example.com/modlist.wabbajack";
      hash = "sha256-...";
    };
  };
};
```

`hash` is the Nix fetch hash for the `.wabbajack` file itself — see
[Computing a Wabbajack hash](#computing-a-wabbajack-hash) below. Skyrim SE lists
that reference vanilla game files (Wabbajack `GameFileSourceDownloader`
entries — Legends of the Frost is one example) need `gameDir` so modde can read
and verify those local files during installation.

For `.wabbajack` files you already have in the Nix store (via `requireFile` or a
custom fetcher), use a local path instead of `url` + `hash`:

```nix
programs.modde.profiles.living-skyrim = {
  game = "skyrim-se";
  gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
  wabbajackList = {
    path = /nix/store/...-Living-Skyrim.wabbajack;
  };
};
```

### Install from a Nexus Collection

```nix
programs.modde.profiles.my-collection = {
  game = "skyrim-se";
  nexusCollection = {
    slug = "collection-slug";
    version = "1.0.0";
  };
};
```

Collections (and Wabbajack lists) auto-lock the profile to preserve the
curator's intended load order. See [Profile management](../guides/profiles.md)
for forking a locked profile into a freely editable copy.

### Declaring a profile before the game exists

modde never installs the base game; you install it through Steam or Heroic, and
modde waits. If the game is not installed yet, set `installMode = "await-game"`
(or simply leave `gameDir` unset). Activation then skips install/deploy
non-fatally and prints the next step instead of failing the whole rebuild:

```nix
programs.modde.profiles.living-skyrim = {
  game = "skyrim-se";
  installMode = "await-game";
  wabbajackList = {
    url = "https://example.com/modlist.wabbajack";
    hash = "sha256-...";
  };
};
```

Install the game through its launcher, set `gameDir`, change `installMode` back
to `"auto"` (or remove it), and rebuild. modde then installs and deploys the
profile. Wabbajack install runs before deploy only when `gameDir` exists and
contains the expected game-content directory (e.g. `Data/` for Skyrim SE). For
every option, see the [Home-Manager module reference](../configuration/hm-module.md).

## Computing a Wabbajack hash

The declarative `wabbajackList.url` + `hash` pair needs the **Nix fetch hash**
of the `.wabbajack` file (SRI format, `sha256-…`). This is the hash of the
archive, not the Wabbajack-internal archive hashes inside the manifest. Compute
it with either of these:

```bash
# Modern Nix (preferred): prints the SRI hash and stores the file
nix store prefetch-file "https://example.com/modlist.wabbajack"

# Or via the flake prefetch helper
nix flake prefetch "https://example.com/modlist.wabbajack"
```

Both print a `sha256-…` value you paste verbatim into `hash`. If you already
have the file locally:

```bash
nix hash file ./modlist.wabbajack          # SRI sha256- hash of a local file
```

> Wabbajack registry pages usually link through to an *authored-files* URL for
> the real `.wabbajack` archive. Some of those CDN links now resolve through
> Wabbajack's chunked download page rather than a plain file response, so a bare
> prefetch (and `pkgs.fetchurl`) can 404 even though modde's chunk-aware
> downloader works. In that case, download the file first and use the local
> path:
>
> ```bash
> modde wabbajack download "<registry-url-or-machine-url>" --output ./list.wabbajack
> nix hash file ./list.wabbajack
> ```
>
> Then either set `wabbajackList.path = ./list.wabbajack;` (no `hash` needed for
> a path source) or feed it through your own fetcher.

modde can also write the home-manager snippet for you:

```bash
modde wabbajack hm-snippet "<url-or-file>" \
  --profile living-skyrim \
  --game skyrim-se \
  --game-dir "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition"
```

## First-run gotchas

A short checklist of things that trip people up on the first profile:

- **Install the base game first.** modde waits for launcher-managed installs; it
  never installs Steam or the game itself. For declarative Wabbajack profiles
  before the game exists, use `installMode = "await-game"` so activation does not
  fail the rebuild.
- **Set up Nexus auth before installing anything from Nexus.** Run
  `modde nexus auth` (or set `nexus.apiKeyFile`). CDN downloads require a Nexus
  **Premium** subscription; `modde nexus status` tells you whether your key is
  valid and Premium. See [Nexus Mods](../guides/nexus.md).
- **`gameDir` / `--game-dir` is required for lists that read vanilla files.**
  Skyrim SE lists with `GameFileSourceDownloader` entries fail without it.
- **Use the detected game id.** Run `modde detect` and copy the exact id; a
  typo'd `--game` is the most common "nothing happens" cause.
- **The `hash` is the file's Nix hash, not a manifest hash.** Compute it as
  above; a mismatch aborts the fetch by design.
- **Deploy is reversible and non-destructive.** It does not modify your original
  game files, and `modde rollback` restores the previous deployment.
- **Adopt existing saves before your first switch.** If you already have saves,
  run `modde save adopt --game skyrim-se --profile my-skyrim` so a later profile
  switch does not park them unexpectedly. See [Save management](../guides/saves.md).

## What deploy does

`modde deploy` builds a **symlink farm** and links it into the game directory
without touching your original game files. The pipeline has three phases:

1. **Build** — modde resolves the load order and maps each relative file path to
   its winning source. For any path provided by multiple mods, the mod later in
   the load order wins; hidden files and profile-level overrides are applied
   here.
2. **Materialize** — the farm is written to a staging directory
   (`~/.local/share/modde/staging/<profile>/`); each file becomes a symlink to
   the winning mod's content.
3. **Deploy** — the materialized staging tree is symlinked into the game's mod
   directory.

The previous deployment is preserved in `staging.bak/`, so
`modde rollback --profile my-skyrim` atomically swaps back. Wabbajack profiles
deploy differently: their pre-built file layouts are hardlinked (or copied)
straight into the game directory, bypassing the conflict-resolving symlink farm.
Verify a deployment with `modde verify --profile my-skyrim`. Full detail lives
in the [Deployment & VFS guide](../guides/deployment.md).

## See also

- [Your first profile](first-profile.md) — the same flow as one narrated walkthrough
- [Installation](installation.md) — get modde onto your system
- [Profile management](../guides/profiles.md) — switch, fork, lock, experiment
- [Wabbajack modlists](../guides/wabbajack.md) — installing `.wabbajack` lists
- [Nexus Mods](../guides/nexus.md) — authentication, mods, Collections
- [Deployment & VFS](../guides/deployment.md) — how deploy works in depth
- [Playing a game](../guides/playing.md) — `modde play` end to end
- [Supported games](../games/supported-games.md) — game ids and coverage
