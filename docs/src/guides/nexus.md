# Nexus Mods

## Overview

modde integrates with [Nexus Mods](https://www.nexusmods.com/) for authentication, browsing,
searching, downloading, installing, and update-checking mods. The integration uses both the
**v1 REST API** (`https://api.nexusmods.com/v1`) and the **v2 GraphQL API**
(`https://api.nexusmods.com/v2/graphql`). A Nexus Mods account with an API key is required for
every authenticated call, and **automated CDN downloads additionally require a Premium
subscription** — see [Premium and what free accounts get](#premium-and-what-free-accounts-get).

Beyond installs, modde supports browse/search/trending feeds, Nexus Collections, the `nxm://`
handler, and update checks and their safeguards.

## Authentication

### The six-source resolution chain

modde uses the **first** source that yields a non-empty Nexus key. The config file takes
precedence over the environment variable, which takes precedence over the keyring.

| Priority | Source | How it is set | Best for |
| -------- | ------ | ------------- | -------- |
| 1 | **OAuth token** | `OAuth2` PKCE flow, stored in the keyring (`modde/nexus-oauth-token`); used only while unexpired (60 s skew buffer) | Future interactive desktop login |
| 2 | **modde config file** | `modde nexus auth` writes `~/.config/modde/nexus_api_key` (mode `0600`) | Single-user workstations |
| 3 | **`NEXUS_API_KEY`** | Environment variable | CI, containers, one-off shells |
| 4 | **System keyring** | secret-service over D-Bus, service `modde`, key `nexus-api-key` | Desktop sessions with an unlocked keyring |
| 5 | **`NEXUS_API_KEY_FILE`** | Environment variable pointing at a file whose contents are the key | **sops-nix / agenix / systemd credentials** |
| 6 | **Legacy `settings.toml`** | `nexus_api_key` field in modde's settings file | Backward compatibility only |

If none of the six yields a key, modde fails with:

```text
No Nexus API key found. Set NEXUS_API_KEY env var or run `modde nexus auth`.
```

Every key is trimmed of surrounding whitespace before use, and empty/whitespace-only sources are
skipped (they do not short-circuit the chain). The OAuth entry at priority 1 is checked first but
falls through to the API-key chain when no token is stored or the stored token has expired.

> **Note on the keyring vs. the config file.** Priority 2 (the `modde nexus auth` config file) and
> priority 4 (the system keyring) are distinct. The current `modde nexus auth` flow writes the
> mode-`0600` config file at `~/.config/modde/nexus_api_key`; the keyring slot is also read if
> present (e.g. written by another tool or a future flow). Because the config file sits **above**
> the keyring in the chain, a key written by `modde nexus auth` always takes precedence.

### Recommendation per environment

| Environment | Recommended source | Why |
| ----------- | ------------------ | --- |
| **NixOS / home-manager** | `NEXUS_API_KEY_FILE` via `programs.modde.nexus.apiKeyFile` | Declarative, secret stays out of the Nix store, sops-nix/agenix compatible |
| Single-user desktop | `modde nexus auth` (config file) | One command, restrictive permissions, survives reboots |
| CI / ephemeral containers | `NEXUS_API_KEY` env var | No persistent state, easy to inject as a secret |
| Shared / multi-user host | `NEXUS_API_KEY_FILE` with per-user file permissions | Avoids leaking the key into a shared keyring or shell history |

### Saving your API key (`modde nexus auth`)

```bash
modde nexus auth
```

This stores your personal API key so that subsequent commands can authenticate non-interactively.
Get your key from [Nexus Mods → My Account → API Keys](https://www.nexusmods.com/users/myaccount?tab=api).

### Using sops-nix (declarative, recommended on NixOS)

For NixOS/home-manager setups, point modde at a secret file. The home-manager module wires
`apiKeyFile` to `NEXUS_API_KEY_FILE`, which is **priority 5** in the chain:

```nix
programs.modde = {
  enable = true;
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";
};
```

The file contents are the raw key; modde trims trailing newlines. Because the path is read at
runtime, the secret never enters the Nix store. This is compatible with sops-nix, agenix, and
systemd credentials. See the [Home-Manager module reference](../configuration/hm-module.md) for
the full option.

### Checking status (`modde nexus status`)

```bash
modde nexus status
```

modde calls the v1 `users/validate.json` endpoint to check your key and report the account name
and **Premium** status. Use it to confirm the key is valid and the account meets the Premium
requirement for automated downloads.

## Premium and what free accounts get

Automated CDN downloads — the `download_link.json` endpoint that returns a time-limited CDN URL —
are **Premium-only**. Before generating any download link, modde validates the account and refuses
non-Premium keys with:

```text
Nexus Premium is required for automated downloads.
Please upgrade at https://next.nexusmods.com/premium
```

| Capability | Free account | Premium account |
| ---------- | ------------ | --------------- |
| `modde nexus status` / account validation | Yes | Yes |
| Browse / search / trending / updated feeds (REST + GraphQL) | Yes | Yes |
| Mod metadata, file listings, collection manifests | Yes | Yes |
| Update **checking** (`modde update check`) | Yes | Yes |
| Endorse / track / untrack mods | Yes | Yes |
| **Automated CDN download** (`install mod`, `update apply`, `nxm handle`, collection installs) | No | Yes |

Free accounts can still drive a `nxm://` "Download with modde" link **when the link carries its own
short-lived `key`/`expires` pair** issued by the website (see [the nxm:// handler](#nxm-protocol-handler)),
because that path does not require the Premium `download_link.json` call. Everything that resolves a
CDN URL through the API requires Premium.

## Browsing and searching

For browse/search, modde tries **v2 GraphQL** first: one request returns thumbnails,
summaries, and endorsement and download counts for each tile. If the schema changes or the endpoint
errors, modde **falls back to the v1 REST** equivalent so the UI keeps rendering.

| Feed | GraphQL (preferred) | REST fallback (v1) |
| ---- | ------------------- | ------------------ |
| Trending | `browse_feed_gql(domain, Trending)` | `GET /games/{domain}/mods/trending.json` |
| Monthly top | `browse_feed_gql(domain, MonthlyTop)` | `GET /games/{domain}/mods/updated.json?period=1m` |
| Full-text search | `search_mods_gql(domain, term, page)` | `GET /games/{domain}/mods/search.json?search=…&page=…` |
| Collections feed/search | `collections_feed_gql(domain, term)` | `GET /games/{domain}/collections.json?search=…` |

Additional v1 REST endpoints back the rest of the integration:

| Purpose | Endpoint |
| ------- | -------- |
| Mod details | `GET /games/{domain}/mods/{id}.json` |
| File listing | `GET /games/{domain}/mods/{id}/files.json` |
| Recently updated | `GET /games/{domain}/mods/updated.json?period={1d\|1w\|1m}` |
| Collection by slug | `GET /collections/{slug}.json` (game domain auto-discovered) |
| Collection revision | `GET /games/{domain}/collections/{slug}/revisions/{rev}.json` |
| Endorse / abstain | `POST …/mods/{id}/endorse.json` · `…/abstain.json` |
| Tracked mods | `GET`/`POST`/`DELETE /user/tracked_mods.json` |

The mod-image gallery is fetched via a small dedicated GraphQL query (`modImages`), falling back to
the single `picture_url` from the v1 mod response when GraphQL is unavailable.

> The base URLs honour `MODDE_NEXUS_BASE_URL` and `MODDE_NEXUS_GRAPHQL_URL` for pointing the client
> at a mock server in tests. Production never sets them.

## Rate-limit awareness

Nexus enforces hourly and daily request limits. modde reads the response headers on every v1 call
and reacts:

- **`x-rl-hourly-remaining`** — when fewer than **10** requests remain in the hour, modde logs a
  warning so you can slow down before hitting the wall.
- **HTTP `429`** — a hard rate-limit error. modde surfaces it as
  `Nexus API rate limit exceeded. Please wait before retrying.` Respect the `Retry-After` header
  the server returns and wait before re-running.

For update checks, modde **groups mods by game domain** and makes exactly **one** `updated_mods`
call per domain, regardless of how many mods you track
(see [Checking for updates](#checking-for-updates)).

## Installing a single mod

```bash
modde install mod https://www.nexusmods.com/skyrimspecialedition/mods/12345 --profile my-skyrim
```

The pipeline:

1. Resolve the mod, pick the latest **MAIN** file, and generate a CDN download link (Premium-gated).
2. Download the archive into the store, then **extract into a temporary staging tree** (not the
   store) for analysis.
3. Hash the archive (`xxhash64`) and record it on the install plan.
4. **Analyze** the staged tree to detect the install method (simple copy, FOMOD, BAIN, …).
5. Move files into the canonical store layout `store/{domain}_{mod}_{file}/`, or, when more input is
   needed, keep the staged copy and route you to a wizard.

The pipeline is idempotent: if `store/{domain}_{mod}_{file}/` already exists it returns
`AlreadyStaged` and skips the download. The same code path backs both the CLI and the UI
**Browse Nexus → Install** button.

### Non-interactive FOMOD installs

If the mod uses a FOMOD installer, supply a declarative config to answer its choices without a
wizard:

```bash
modde install mod <url> --profile my-skyrim --fomod-config my-choices.toml
```

The config can be TOML or JSON. Without it, a FOMOD mod is staged and marked
`pending_user_input` so a wizard can run later **without re-downloading**. See the
[FOMOD guide](fomod.md) for authoring `--fomod-config` files.

### When detection fails

If modde can't classify the archive, it writes a **dossier** next to the staged files and marks
the install `unknown`. The skill tooling uses the dossier's mod name, author, version, Nexus URL, and
probe trace to add support for the layout.

## Nexus Collections

A Nexus Collection is a curated, version-pinned set of mods. modde resolves them in **two steps**,
which lets you install by slug alone without knowing the game domain up front:

1. **Discover** — `GET /collections/{slug}.json` returns the collection's game domain and its latest
   published revision number.
2. **Fetch the manifest** — `GET /games/{domain}/collections/{slug}/revisions/{rev}.json` returns the
   full, pinned manifest.

### From the CLI

```bash
# Install the latest published revision
modde install nexus-collection <slug> --profile my-collection

# Pin a specific revision/version
modde install nexus-collection <slug> --version 7 --profile my-collection
```

Without `--version`, modde uses the latest published revision from step 1. With it, modde still
uses step 1 to find the game domain, then fetches the specified revision directly.

### Declaratively (home-manager)

```nix
programs.modde.profiles.my-collection = {
  game = "skyrim-se";
  nexusCollection = {
    slug = "collection-slug";
    version = "1.0.0";
  };
};
```

`nexusCollection` is mutually exclusive with `wabbajackList`. See the
[Home-Manager module reference](../configuration/hm-module.md).

### Version pinning and the profile lock

Installing a collection **locks the profile** with a `NexusCollection` lock reason. The lock
preserves the curator's intended load order and prevents `modde update apply` from silently drifting
the profile away from the pinned revision. To update a locked profile you must opt in explicitly —
see [the update guardrails](#safety-guardrails).

## nxm:// protocol handler

Nexus's "Download with [manager]" buttons emit `nxm://` URIs. To make modde the system handler:

```bash
modde nxm install
```

This registers an XDG desktop entry for the `nxm://` scheme. Clicking a download link on the Nexus
website then hands the URI to modde, which fetches the file. You can also handle a URI by hand:

```bash
modde nxm handle "nxm://skyrimspecialedition/mods/12345/files/67890?key=abc&expires=123" \
  --profile my-skyrim
```

The URI carries the game domain, mod ID, file ID, and a short-lived `key`/`expires` pair the website
mints for the download. `--profile` routes the result into a specific profile.

## Checking for updates

### `modde update check`

```bash
# Check for updates in the last week (default period)
modde update check --profile my-skyrim

# Check updates from the last day or month
modde update check --profile my-skyrim --period 1d
modde update check --profile my-skyrim --period 1m
```

`--period` accepts `1d`, `1w` (default), or `1m`. Only enabled mods that carry Nexus metadata
(`nexus_mod_id` + `nexus_game_domain` + `installed_timestamp`) are considered — mods installed via
`modde install mod` and Nexus Collections record this automatically. modde groups the tracked mods by
game domain, makes **one** `updated_mods` call per domain, and reports a mod as updatable when its
`latest_file_update` timestamp is newer than your local `installed_timestamp`.

> `modde update check --mods` checks your Nexus-tracked profile mods. Without `--mods` (and with no
> profile mods to check) the same command surface also covers modde's own product update check.

### `modde update apply`

This command downloads and installs the latest MAIN file for each updatable mod, then points the
profile entry at the new `(mod_id, file_id)` pair. Schedule `check` and `apply` together to keep a
profile current.

```bash
# Preview only — list what would change, download nothing
modde update apply --profile my-skyrim --dry-run

# Apply non-breaking updates
modde update apply --profile my-skyrim
```

### Safety guardrails

`update apply` is deliberately conservative because auto-updating can corrupt a curated setup:

- **Locked-profile guard.** Wabbajack, Nexus Collection, and TOML-import profiles ship a pinned,
  authoritative load order. `apply` **refuses to run** on a locked profile and tells you to either
  pass `--confirm-locked` (acknowledging the drift) or run `modde profile unlock <name>` first. With
  `--confirm-locked` it proceeds but warns that the locked source will not be re-verified afterward.
- **Breaking-semver guard.** Updates that look like a **major** version bump (e.g. `1.x → 2.0`) are
  flagged breaking. They are skipped unless you pass `--accept-breaking`, and even then each breaking
  mod prompts for an interactive `y/N` confirmation. `--yes` assumes "yes" to those per-mod prompts
  but still refuses any breaking update unless `--accept-breaking` is also set. Version strings that
  can't be parsed fall through as "not breaking".

```bash
# Update a locked Collection profile, accepting major bumps non-interactively
modde update apply --profile my-collection --confirm-locked --accept-breaking --yes
```

## See also

- [Download backends](downloads.md) — every transport modde can fetch from, and the resume/queue model
- [Wabbajack modlists](wabbajack.md) — the other major Nexus-backed install surface
- [FOMOD installers](fomod.md) — authoring `--fomod-config` files
- [Home-Manager module reference](../configuration/hm-module.md) — `nexus.apiKeyFile`, `nexusCollection`
- [Parity audit](../reference/parity.md) — what is `Done` vs `Partial`
