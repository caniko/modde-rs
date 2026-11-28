# Download backends

## Overview

modde resolves and fetches mod archives through pluggable **download backends**. Each backend implements
the two-phase `DownloadSource` trait:

1. **`resolve`** — turn a download directive into a concrete `DownloadHandle` (a final URL, optional
   mirror candidates, headers, an expected hash, and a size hint).
2. **`download_with_progress`** — stream the bytes to disk, report progress, and **verify the hash**.

At runtime, `AnySource` wraps the backends so a mixed list can dispatch each directive to the right
one. Each backend computes both Wabbajack-compatible hashes while streaming: **xxhash64** (`xxh64`,
seed 0) and **xxh3-64** (`xxh3`). The hash check passes if either digest matches; otherwise modde
deletes the partial file and returns a typed `HashMismatch` error.

> **First-class vs. directive-driven.** In normal end-to-end user flows, **Nexus** is the
> first-class download surface (the Browse/Install UI and `modde install mod`). The other backends
> primarily serve **Wabbajack and collection directive flows**, where the manifest specifies the
> transport and hash. That broader backend support is **Partial**: the transports work, but not
> all have a polished standalone workflow. See
> the [parity audit](../reference/parity.md).

## Backend reference

| Backend | Directive | Auth | Resume | Notable behaviour |
| ------- | --------- | ---- | ------ | ----------------- |
| Nexus | `Nexus { game, mod, file, hash }` | API key + **Premium** | No | CDN link via `download_link.json` |
| GitHub | `GitHub { user, repo, tag, asset, hash }` | optional `GITHUB_TOKEN` | No (retry) | Resolves a release asset by name |
| Direct | `DirectURL { url, headers, mirror_resolver, hash }` | per-directive headers | **Yes (Range)** | Mirror fallback + range resume |
| Google Drive | `GoogleDrive { id, hash }` | none | No | Handles the virus-scan interstitial |
| MEGA | `Mega { url, hash }` | none | No | Client-side AES-128-CTR decrypt |
| MediaFire | `MediaFire { url, hash }` | none | **Yes (via Direct)** | HTML-scrapes the direct link |
| Manual | `Manual { url, prompt, … }` | n/a | n/a | Fails fast with guidance |
| Wabbajack CDN | `WabbajackCdn { url, hash }` | none | No | Wabbajack-hosted mirror archives |

### Nexus

The Nexus backend asks the v1 API for a time-limited CDN URL
(`…/files/{file_id}/download_link.json`) and streams it to disk. Generating that link is
**Premium-gated**: modde validates the account first and refuses non-Premium keys. Authentication,
the six-source key chain, browse/search, collections, and update checking are all documented in the
[Nexus Mods guide](nexus.md). This page covers Nexus only as a transport.

### GitHub Releases

```text
GitHub { user, repo, tag, asset, hash }
```

Resolves to a release asset by exact `name` within a tag (`GET /repos/{user}/{repo}/releases/tags/{tag}`),
then downloads `browser_download_url`. If a **`GITHUB_TOKEN`** environment variable is set, modde
sends it as a `Bearer` token — this lifts the unauthenticated rate limit and allows private-repo
assets. All requests send a `User-Agent: modde` header (GitHub rejects requests without one).
Transfers are wrapped in the shared retry/backoff helper (up to 3 attempts with exponential backoff
and jitter). The backend can also list releases and fetch a release summary by tag for tooling.

### Direct HTTPS

```text
DirectURL { url, headers, mirror_resolver, hash }
```

Direct is the most capable transport. It supports:

- **Range resume.** If a partial file already exists at the destination, modde sends a
  `Range: bytes=N-` header. On a `206 Partial Content` it appends to the existing file and **seeds
  the running hash with the bytes already on disk**, so resumed downloads still verify end to end. If
  the server ignores the range (returns `200`), modde restarts cleanly from scratch.
- **Mirror fallback.** When the directive carries a `mirror_resolver`, modde scrapes the resolver
  page for candidate URLs and tries each in order, deleting the partial file between failed
  candidates. A single-candidate download surfaces its own error; a multi-candidate download reports
  an aggregated list of every URL that failed. A resolver-supplied `User-Agent` is injected unless the
  directive already set one.
- **Custom headers.** Arbitrary per-directive request headers are forwarded verbatim.

Direct is also the engine behind MediaFire (below).

### Google Drive

```text
GoogleDrive { id, hash }
```

Google interrupts large-file downloads with an HTML "can't scan this file for viruses" interstitial
instead of serving the bytes. modde handles both paths:

- It first hits the modern `drive.usercontent.google.com/download?id=…&confirm=t` host, which usually
  skips the interstitial entirely.
- If Google still returns `text/html`, modde parses the page for the **confirm token** (it tries
  three patterns: an `&confirm=TOKEN` query parameter, a `name="confirm" value="TOKEN"` hidden input,
  and the `id="uc-download-link"` anchor) and re-requests with `&confirm=<token>` to fetch the real
  bytes.

### MEGA

```text
Mega { url, hash }
```

MEGA encrypts files client-side, so modde decrypts them locally:

- **Both URL formats** are parsed: the modern `https://mega.nz/file/HANDLE#KEY` and the legacy
  `https://mega.nz/#!HANDLE!KEY`.
- It calls the MEGA API (`https://g.api.mega.co.nz/cs`) with the file handle to obtain the encrypted
  download URL and the file size.
- The 32-byte base64url key is decoded; the AES-128 key is the **XOR of its two 16-byte halves**, and
  the IV is bytes 16–24 zero-padded (the low 8 bytes are the CTR counter, starting at 0).
- As the encrypted stream arrives, modde applies an **AES-128-CTR** keystream chunk by chunk and
  writes the plaintext to disk, then verifies the hash.

### MediaFire

```text
MediaFire { url, hash }
```

MediaFire serves a file *page*, not a direct link. modde fetches it with a browser-like
`User-Agent` and extracts the `href` of the `aria-label="Download file"` anchor. It passes that
`download…mediafire.com/…` URL to the Direct backend for range resume and retries.

### Manual

```text
Manual { url, prompt, expected_name, … }
```

Some Wabbajack archives are gated behind interactive pages that cannot be fetched automatically.
modde mirrors Wabbajack's behaviour: it **fails fast at resolve time** with a clear, actionable
message naming the expected file, the upstream URL to visit, and any prompt the list author left:

```text
manual download required for <expected_name>: visit <url> and place the downloaded
file in the modde downloads directory. Prompt from list author: <prompt>
```

To continue the install, download the file by hand and put it in the downloads directory.

## Resumable `.meta` sidecars

modde models each download as a JSON **`.meta` sidecar** written next to the destination file
(`mod_file.zip` → `mod_file.zip.meta`). The sidecar records:

- `url`, `expected_hash`, `bytes_downloaded`, `total_bytes`, and a `status` string
  (`queued` / `downloading` / `paused` / `complete` / `failed: …`)
- Nexus provenance: `nexus_mod_id`, `nexus_file_id`, `game_domain`, `mod_name`, `version`

Sidecars are the persistence layer that lets a queue be **rebuilt across process restarts**: on load,
`complete` entries are restored only if the file still exists, and any `downloading`/`paused` entry is
restored as **paused** (because no transport is running after startup). This pairs with the Direct
backend's range resume to continue a partial transfer rather than restart it.

> **Status: Partial.** The sidecar data model and round-trip are implemented and tested, but writing
> and reloading sidecars is **not yet a shipped end-to-end workflow** in the GUI. Treat durable
> cross-restart resume as in-progress; see the [parity audit](../reference/parity.md).

## The download queue

`DownloadQueue` is a pure, synchronous state machine that enforces the
**concurrency limit** without performing I/O; the caller starts the async
transfers. Each task moves
through `Queued → Active → {Complete | Paused | Failed}`:

- `enqueue(url, dest, expected_hash, meta)` adds a task and returns its ID.
- `take_next()` promotes the next `Queued` task to `Active`, but only while the active count is below
  the limit — this is how concurrency is bounded.
- `pause(id)` / `resume(id)` move a task to `Paused` and back to `Queued`; `cancel(id)` removes it.
- `save_sidecars()` / `load_from_sidecars(dir, limit)` persist and rebuild the queue from `.meta`
  files (see above).

The GUI currently constructs the queue with a concurrency of **2** and renders real queue state in
its Downloads view.

> **Status: Partial.** The queue's UI state (queued/active/paused/failed/complete) is shipped, but
> transport-level **pause/resume is not yet wired to the network layer**, and there is no speed/ETA
> calculation yet (progress callbacks report bytes, not rate). The Wabbajack engine has its own
> concurrent download path (default parallelism 4) used during modlist installs.

## Tuning environment variables

These environment variables tune the download and staging machinery. The byte cache, zstd staging
compression, and archive-retention knobs are read by the **Wabbajack install engine** and are mainly
relevant to large modlist installs (**Partial**); they are not part of the everyday single-mod
download path.

| Variable | Default | Effect | Where it applies |
| -------- | ------- | ------ | ---------------- |
| `MODDE_BYTE_CACHE_MIB` | `512` | Size (MiB) of the in-memory LRU cache of extracted archive entries | Wabbajack extraction (Partial) |
| `MODDE_ZSTD_MIN_BYTES` | `1048576` (1 MiB) | Minimum file size before a staged file is zstd-compressed | Wabbajack staging (Partial) |
| `MODDE_ZSTD_LEVEL` | `9` | zstd compression level for staged files (clamped to 1–22) | Wabbajack staging (Partial) |
| `MODDE_ARCHIVE_RETENTION` | `keep` | What to do with source archives after a batch is applied: `keep`, `prune-applied` (aliases `prune`/`delete`), or `auto` | Wabbajack install (Partial) |

Related authentication / transport variables documented elsewhere:

| Variable | Purpose | See |
| -------- | ------- | --- |
| `GITHUB_TOKEN` | Authenticated GitHub release downloads | [GitHub Releases](#github-releases) |
| `NEXUS_API_KEY` / `NEXUS_API_KEY_FILE` | Nexus credentials | [Nexus Mods guide](nexus.md) |

`MODDE_ARCHIVE_RETENTION` also has a CLI equivalent on the Wabbajack installer
(`--archive-retention keep|prune-applied|auto`); the flag wins when both are set.

## See also

- [Nexus Mods](nexus.md) — the first-class download surface: auth, browse, install, updates
- [Wabbajack modlists](wabbajack.md) — the directive-driven install flow that exercises every backend
- [Troubleshooting](troubleshooting.md) — download failures, hash mismatches, rate limits
- [Parity audit](../reference/parity.md) — what is `Done` vs `Partial`
