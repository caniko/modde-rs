# Wabbajack Modlists

## Overview

[Wabbajack](https://www.wabbajack.org/) is a modlist installer originally built
for Windows. A `.wabbajack` file is a recipe — not a bundle of mods — that
records exactly which archives to download from the internet and exactly how to
reconstruct a curated mod setup from them. modde reads that recipe and replays
it **natively on Linux**: no Windows VM, no Wine-hosted Wabbajack client, no
`.NET` runtime. modde parses the manifest, resolves and verifies every download,
applies binary patches, repacks Bethesda archives, and stages the result, all in
Rust.

This means a `.wabbajack` list authored on Windows installs the same way a
NixOS user expects any other package to install: content-addressed, hash-verified,
resumable, and reproducible.

## How it works

A `.wabbajack` file is a ZIP archive. Inside it is a single JSON **manifest**
(named `modlist` or `*.json`) plus inline patch/data blobs. modde reads the
manifest into a [`WabbajackManifest`](https://github.com/caniko/rs-modde)
with two top-level lists:

- **`Archives`** — every external file the list needs, each identified by its
  Wabbajack hash (an xxHash64 stored as little-endian base64), its size, and a
  source `State` describing where to fetch it.
- **`Directives`** — the ordered list of operations that turn downloaded
  archives plus inline data into the final mod layout.

modde converts the raw archives into typed **download directives** and the raw
directives into typed **install directives**, then drives them through a
pipeline: download/verify → resumable staging → apply directives → optional
BSA/BA2 repack → validate → deploy.

modde does not install Steam or Heroic games; install the game with its launcher
first, then point `gameDir` at the installed game directory.

### Archive sources (download directives)

Each `Archive` carries a `State` whose `$type` selects a downloader. modde maps
them to typed download directives:

| Wabbajack `State`              | modde directive   | Notes |
| ------------------------------ | ----------------- | ----- |
| `NexusDownloader`              | `Nexus`           | Resolved via the Nexus API (game domain + mod id + file id). Requires an API key. |
| `WabbajackCDNDownloader`       | `WabbajackCdn`    | Authored-files / generated-output archives hosted on Wabbajack's CDN. |
| `GitHubDownloader`             | `GitHub`          | Release asset by `user/repo`, tag, and asset name. |
| `GoogleDriveDownloader`        | `GoogleDrive`     | By Drive file id. |
| `MegaDownloader`               | `Mega`            | By MEGA URL. |
| `MediaFireDownloader`          | `MediaFire`       | By MediaFire URL. The MediaFire download backend resolves the real file. |
| `HttpDownloader`               | `DirectURL`       | Plain HTTP(S) with optional headers. |
| `ModDBDownloader`              | `DirectURL`       | Direct URL plus a ModDB HTML mirror resolver that follows the `/downloads/start/<id>/all` mirror page. |
| `ManualDownloader`             | `Manual`          | A site (workupload, sharemods, LoversLab, …) that needs a human to click through. modde cannot fetch these unattended. |
| `GameFileSourceDownloader`     | *(not a download)* | Points at a file inside the **local game install**, not the internet. See [game-file sources](#gamefilesource-vanilla-game-files). |

Every download is verified against its manifest hash before it is trusted. modde
never substitutes a similarly named file — the hash is the only safe identity for
an archive.

### Install directives

modde supports the full set of directives a typical Bethesda/MO2 modlist uses.
Each directive writes into the MO2-style staging layout (`mods/<Mod Name>/…`)
before deployment.

| Directive                | What it does |
| ------------------------ | ------------ |
| **`FromArchive`**        | Extract one file out of a source archive (identified by `ArchiveHashPath = [hash, inner/path]`) and place it at the `To` path. The bread-and-butter directive — most files in a list are `FromArchive`. |
| **`PatchedFromArchive`** | Extract a basis file from an archive, then apply an **OctoDiff** binary delta patch (`PatchID`) to reconstruct a modified file. Used when a list ships a small edit to a large vanilla/mod file rather than redistributing it. modde applies the `OCTODELTA` patch in pure Rust with bounds and output-size checks, and verifies the patched output against the directive's expected `Hash`. |
| **`InlineFile`**         | Write a file whose bytes are stored inline in the `.wabbajack` (by `SourceDataID`). Used for small generated/config files. |
| **`RemappedInlineFile`** | Like `InlineFile`, but with path-remapping applied by the author. modde treats it identically to `InlineFile` for placement and verifies its expected `Hash`. |
| **`CreateBSA`** / BA2 repack | Reconstruct a Bethesda archive from a set of `FileStates`. modde repacks the staged loose files into a real `.bsa` (Skyrim, magic `BSA\0`, SSE version 105) or `.ba2` (Fallout 4, magic `BTDX`/`GNRL`), computing the Bethesda/BA2 name hashes the games expect. The output format is chosen from the `To` extension. |
| **Manual** (`ManualDownloader` archive) | Not a directive type, but the corresponding archive must be human-fetched (see [missing archives](#missing-authored-and-manual-archives)). |
| **`GameFileSource`**     | Archives sourced from vanilla game files (see below). The directives that consume them are ordinary `FromArchive`/`PatchedFromArchive`. |

Directives that read from a single source archive are **grouped per archive** so
each downloaded archive is opened once, all of its outputs (extractions and
patches) are produced together, and the archive can then be released or pruned.
This keeps memory and I/O bounded even for lists with thousands of directives.

Unknown / unsupported directive `$type`s parse as `Unknown` and are surfaced as
hard blockers by [`modde wabbajack assess`](#assessing-readiness) rather than
being silently skipped.

### GameFileSource: vanilla game files

Some lists reference files that ship with the base game instead of downloading
them. Wabbajack records these as `GameFileSourceDownloader` archives that name a
game-relative path (e.g. `Data\Skyrim.esm`). These entries are **not downloads** —
modde reads and verifies them straight out of your installed game directory.

For example, several Skyrim SE lists (including *Legends of the Frost*) carry
`GameFileSourceDownloader` entries. Pass `--game-dir` (CLI) or set `gameDir`
(Home Manager) so modde can locate and hash those files. If a list has
game-file sources and no game directory is provided, the install is blocked
until you point it at the install.

## Prerequisites

- **A Nexus Mods API key** — required for every `NexusDownloader` archive (the
  bulk of most lists). See [Nexus integration](nexus.md).
- **The base game installed** — install it via Steam/Heroic first; modde does
  not install games.
- **The game install path** for lists that reference vanilla files
  (`GameFileSourceDownloader`) — supply `--game-dir` / `gameDir`.
- **Sufficient disk space** — you need room for the downloaded archives **and**
  the staged/deployed output simultaneously. Large Bethesda lists routinely need
  hundreds of gigabytes. Staging compresses eligible files (see
  [resumable zstd staging](#resumable-zstd-staging)) to soften this, but plan for
  the full footprint.

## Declarative installation (Home Manager)

```nix
programs.modde = {
  enable = true;
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.my-modlist = {
    game = "skyrim-se";
    installMode = "auto";
    gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
    wabbajackList = {
      url = "https://example.com/modlist.wabbajack";
      hash = "sha256-...";
    };
  };
};
```

If the modlist is already available locally, for example through
`pkgs.requireFile` or a custom fetcher, use `path` instead of `url` and `hash`:

```nix
programs.modde.profiles.my-modlist = {
  game = "skyrim-se";
  gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
  wabbajackList = {
    path = /nix/store/...-Legends-of-the-Frost.wabbajack;
  };
};
```

Use `installMode = "await-game"` while the game is not installed yet. Activation
will skip install/deploy and print the next step instead of failing.

## CLI installation

```bash
modde install wabbajack /path/to/modlist.wabbajack \
  --profile my-modlist \
  --game-dir "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition"
```

Wabbajack registry pages usually link through to an authored-files URL for the
actual `.wabbajack` archive. Some authored-files CDN links now resolve through
Wabbajack's chunked download page instead of a plain file response, so
`pkgs.fetchurl` may 404 even when modde's chunk-aware downloader works. In that
case, download the file with `modde wabbajack download` and use
`wabbajackList.path`, or use a dedicated fetcher that reconstructs the chunks.

### The full `modde install wabbajack` flag set

```bash
modde install wabbajack <PATH> [flags]
```

| Flag | Default | Effect |
| ---- | ------- | ------ |
| `--profile <name>` | derived from list | Target profile to create/update. |
| `--game-dir <path>` | auto-detect | Game install to deploy into and to read `GameFileSource` files from. |
| `--force` | off | Force a full reinstall, skipping the preflight short-circuit (the cheap existence check that lets an already-staged list skip straight to deploy). |
| `--no-deploy` | off | Stage into modde's data directory but **skip the final copy into `--game-dir`**. Ideal for Stock-Game lists that must not write into the live install, and for staging a list before the game is even present. |
| `--continue-on-error` | off | Log per-archive failures (download errors, missing files, broken upstream links) and keep going instead of aborting. Lets you drop manually-fetched archives in and re-run to fill the gaps. |
| `--reset-staging` | off | Explicitly discard existing staging before installing. By default modde **resumes** compatible staging and **adopts** incompatible-but-present staging without deleting it (see below). |
| `--skip-validate` | off | Skip post-staging hash validation before deploy. Faster, but you forfeit the safety net. |
| `--missing-archive-policy <p>` | `fail` | What to do when downloadable manual/Nexus archives are missing. One of `fail`, `omit-files`, `omit-mods` (see below). |
| `--archive-retention <p>` | `keep` | Source-archive retention after a batch is integrated: `keep`, `prune-applied`, or `auto`. |
| `--diagnostics-dir <path>` | none | Write apply diagnostics (heartbeat + per-batch JSONL) here for later analysis. |
| `--diagnostics-interval <s>` | `30` | Diagnostics heartbeat interval in seconds. |
| `--stall-warn-seconds <s>` | `600` | Warn when apply makes no batch/sentinel progress for this long. |
| `--stall-abort-seconds <s>` | `1800` | Abort when stalled this long *and* cgroup memory/swap are saturated. |
| `--acquire-missing` | off | Frontload assisted acquisition of missing manual archives before applying. |
| `--no-acquire-missing` | off | Disable the automatic frontloaded acquisition pass entirely. |
| `--acquire-download-dir <path>` | data dir | Browser download directory to watch during assisted acquisition. |
| `--acquire-include-nexus` | off | Also attempt to acquire missing **Nexus** archives during the frontload pass. |
| `--acquire-browser-controller` | off | Drive controlled Chromium tabs (with a managed profile and auto-download prefs) for acquisition instead of just opening the system browser. |
| `--acquire-timeout <s>` | `900` | Per-archive assisted-acquisition timeout in seconds. |

#### `--missing-archive-policy`

When optional manual/Nexus archives cannot be obtained, this controls how far
the install proceeds:

- **`fail`** *(default)* — abort before bulk work; print every missing archive
  with a validation command. Safest: you get a complete list back, not a
  partial install.
- **`omit-files`** — skip only the directives that depend on a missing archive
  (plus any downstream `CreateBSA` that consumed those files). Everything else
  installs.
- **`omit-mods`** — skip the entire MO2 mod root (`mods/<Mod Name>/…`) that any
  missing archive feeds, so you never end up with a half-populated mod folder.

Use [`modde wabbajack missing-impact`](#assessing-readiness) first to see exactly
which mods each policy would drop.

#### `--archive-retention`

After modde finishes integrating an archive's batch of directives it can free
the downloaded source file:

- **`keep`** *(default)* — keep all source archives (fastest re-runs; most disk).
- **`prune-applied`** — delete each source archive once its batch is fully
  applied. `GameFileSource`-backed archives are never pruned (they live in your
  game install, not the store).
- **`auto`** — prune small single-use archives but keep large or multi-directive
  archives that are likely to be re-read, balancing disk against re-extraction
  cost.

#### Acquiring missing archives during install

Unless `--no-acquire-missing` is set, `modde install wabbajack` runs an
assisted-acquisition frontload pass before applying directives. For each missing
manual archive it first tries a direct download; if the source needs a human
(login, captcha, mirror selection) it opens the page in your browser (or, with
`--acquire-browser-controller`, in managed Chromium tabs configured to
auto-download into the watched directory) and waits for a matching file to land,
importing it only when its hash matches. With `--acquire-include-nexus` it also
resolves Nexus archives via the API. If acquisition leaves archives unresolved
and the policy is `fail`, the install stops with the exact list of what is still
missing.

## Assessing readiness

For large lists, do not start blind. The `modde wabbajack` subcommands inspect a
`.wabbajack` (and your store) **without mutating anything** so you can fix
problems before committing hours of downloading.

| Command | Purpose |
| ------- | ------- |
| `modde wabbajack assess <list>` | Full readiness report: archive/directive type histograms, downloadable vs store-present counts, missing archives with remediation, manual-download list, game-file-source presence (`--game-dir`), staging layout action (create/resume/adopt), RAR support, and **hard blockers** vs **warnings**. Exits non-zero if any hard blocker exists. Supports `--profile`, `--game-dir`, `--json`. |
| `modde wabbajack missing-impact <list>` | Which downloadable archives are missing and *what they block*: directly blocked directives + output bytes, affected `CreateBSA` outputs, and the `omit-mods` blast radius. `--json` for machines; `--nix-snippet` prints a Home Manager `manualArchives` block to fill in. |
| `modde wabbajack manual-links <list>` | Just the manual-download URLs (workupload / sharemods / LoversLab) that require an operator visit, with the store path each file must end up at. |
| `modde wabbajack acquire-missing <list>` | Run the assisted-acquisition flow standalone (direct → browser → controlled Chromium), importing only hash-matching downloads. Flags: `--download-dir`, `--include-nexus`, `--browser-controller`, `--timeout`, `--json`. |
| `modde wabbajack import-archive <list> <files…>` | Import already-downloaded archives into the store by hashing them and matching against the manifest (see below). |
| `modde wabbajack analyze-diagnostics <dir>` | Summarize a previous run's `--diagnostics-dir` JSONL: heartbeats, last phase, max idle, peak RSS/swap/cgroup memory, and the ten slowest archive batches with per-phase timings. The tool you reach for when a run stalled or thrashed. |

A typical pre-flight is:

```bash
modde wabbajack assess /path/to/list.wabbajack \
  --game-dir "/path/to/game" --profile my-modlist
modde wabbajack missing-impact /path/to/list.wabbajack
```

If `assess` reports only warnings (no hard blockers) you are ready for a
validated staged deploy. If it reports missing archives but no hard blockers, it
will tell you a partial staging is possible with
`install wabbajack --no-deploy --continue-on-error --skip-validate`.

## Resumable zstd staging

Wabbajack installs are long. modde stages into a versioned, **resumable** layout
so an interrupted run picks up where it left off instead of starting over.

- **Layout versioning** — staging records a `staging-layout.json` describing the
  layout version and compression policy. On the next run modde compares it:
  - matching → **resume** (reuse staged work);
  - present but incompatible → **adopt** in place (keep files, rewrite the
    layout marker) — it does *not* delete your work;
  - absent → **create** fresh.
  `--reset-staging` (or `assess`'s `adopt` notice) is the only thing that
  deletes staging.
- **Per-archive and per-BSA sentinels** — completed archive batches and
  `CreateBSA` outputs drop sentinel files so a resumed run skips finished work.
  `assess` reports `archive sentinels: X/Y` and `BSA sentinels: X/Y` so you can
  see how far a previous run got.
- **Transparent zstd compression** — large, compressible staged files (textures,
  meshes, audio blobs) are stored zstd-compressed with a `.modde-zst` suffix.
  modde resolves, hashes, and materializes logical paths regardless of whether a
  file is plain or compressed, so validation and deploy are oblivious to it.
  Files that compress poorly or must stay byte-exact are **never** compressed:
  plugins (`.esp`/`.esm`/`.esl`), executables and DLLs, config formats
  (`.ini`/`.json`/`.toml`/`.yaml`/`.xml`/…), MO2 metadata (`meta.ini`/`meta.json`),
  BSA temp-build files, and anything under the staging `_state` directory. The
  size threshold and compression level are tunable with the `MODDE_ZSTD_MIN_BYTES`
  and `MODDE_ZSTD_LEVEL` environment variables (level is clamped to 1–22).
- **Post-staging validation** — before deploy (unless `--skip-validate`), modde
  re-hashes every expected file against the manifest. `InlineFile`,
  `RemappedInlineFile`, and `PatchedFromArchive` carry expected output hashes and
  are fully verified; `FromArchive` outputs are existence-checked (the manifest
  does not carry a post-extraction hash for them). Validation accepts both xxHash64
  and xxh3 digests and reads compressed staged files transparently.

## Missing authored and manual archives

Some Wabbajack modlists reference generated authored-files archives hosted by
Wabbajack, or manual-download archives behind a site that needs a human. If those
upstream entries disappear, modde fails before bulk downloads (under the default
`fail` policy) and prints every missing archive plus a `curl -fI` validation
command. modde will not substitute similarly named files because the manifest
hash is the only safe identity for an archive.

If you already have the exact missing archives in an old Wabbajack cache or a
backup, import them into the modde store:

```bash
modde wabbajack import-archive /path/to/list.wabbajack \
  /path/to/missing-archive-1.7z \
  /path/to/missing-archive-2.7z
```

The import command hashes each file and imports only archives whose Wabbajack
hash matches an archive referenced by the manifest. It classifies each input as:

- **imported** — hash matched a referenced archive; hard-linked (or copied) into
  the store and re-verified;
- **already-present** — the store already holds a verified copy;
- **mismatched** — the *filename* matches a manifest archive but the bytes do
  not (refused; this is the dangerous case the hash check catches);
- **unused** — the file's hash is not referenced by this manifest at all.

Mismatched and unused inputs are reported and **not** imported, and the command
exits non-zero if it refused anything.

## Troubleshooting

### Hash mismatches (and the `nix prefetch` trick)

Every archive and every patched/inline output is hash-verified. A mismatch means
the bytes you have are not the bytes the list was built against — never force it
through, because a single wrong file can corrupt the whole load order silently.

- For a missing/wrong **`.wabbajack` file itself** fetched declaratively, recompute
  the Nix hash with `nix store prefetch-file <url>` (or `nix-prefetch-url`) and
  paste the reported `sha256-…` into `wabbajackList.hash`. A `hash mismatch`
  during the Home Manager build almost always means the URL now serves a
  different (or chunk-wrapped) body — see the chunked-download note above.
- For a missing/wrong **archive inside the list**, use
  `modde wabbajack assess` / `missing-impact` to identify it by Wabbajack hash,
  obtain the exact file (correct mod/file id on Nexus, or the original manual
  source), and `import-archive` it. The import refuses anything whose hash does
  not match, so you cannot accidentally poison the store.
- A **post-staging validation mismatch** points at a bad source archive or a
  failed patch. Re-run with `--reset-staging` for the affected list after fixing
  the offending input; a corrupt compressed staged file (`.modde-zst`) is caught
  by validation and re-extracted on the next run.

### Missing authored / manual archives

If `assess` lists manual archives, run `modde wabbajack manual-links` to get the
exact URLs and target store paths, fetch them by hand (or with
`acquire-missing --browser-controller`), and `import-archive` them. If an
authored-files archive has truly vanished upstream and you have no backup, you
cannot reproduce that exact file — switch the install to `--missing-archive-policy
omit-mods` to drop just the affected mods, after reviewing the
`missing-impact` blast radius.

### Stalls and thrash

Very large lists can appear to hang during archive verification/extraction.

- Run with `--diagnostics-dir <dir>`; afterward (or on another terminal against a
  prior run) use `modde wabbajack analyze-diagnostics <dir>` to see the last
  phase, max idle time, peak memory/swap, and the slowest batches. A
  `bottleneck: archive trust/download verification wall before extraction` line
  means the time is going into hashing, not a true hang.
- `--stall-warn-seconds` surfaces a warning when no batch/sentinel progress
  happens for a while; `--stall-abort-seconds` aborts only when stalled *and*
  cgroup memory/swap are saturated (genuine thrash) so a slow-but-progressing run
  is never killed prematurely.
- If memory is the constraint, set `--archive-retention auto` (or
  `prune-applied`) to free integrated archives, and lower `MODDE_ZSTD_LEVEL` to
  reduce compression CPU.
- An interrupted run is safe to resume — just re-run the same command. modde
  resumes compatible staging and skips finished archive/BSA sentinels.

## Migrating an existing on-disk install

If you already deployed a Wabbajack list (on Windows, or a prior run) and want
modde to *track* it without reinstalling, scan the game directory and match it
against the manifest instead. See [Mod scanning](scanning.md) for
`modde scan --manifest …`, threshold tuning, and duplicate pruning.

## Supported games

Not all Wabbajack games are supported yet. See the [Wabbajack game mapping](../games/supported-games.md#wabbajack-game-mapping)
for the current list.

## See also

- [Mod scanning](scanning.md) — track an already-installed list via manifest matching
- [Nexus integration](nexus.md) — API key setup and download resolution
- [Deployment](deployment.md) — how staged mods reach the game directory
- [Profiles](profiles.md) — load-order locks applied by Wabbajack installs
- [Troubleshooting](troubleshooting.md) — general install/runtime issues
- [Supported games](../games/supported-games.md) — game coverage and Wabbajack mapping
- [CLI reference](../reference/cli.md) — every flag, generated from the binary
