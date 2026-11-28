# Settings file & environment

The settings file is modde's primary configuration on Linux, macOS, and Windows
and works the same without Nix. The CLI and GUI read and write plain TOML files
directly in your platform's config directory. Environment variables override
paths and behavior at runtime. This page covers the file schema, environment
variables, instance registry, and precedence.

On Nix, the [home-manager module](hm-module.md) lets you declare profiles as
code on top of the same runtime without replacing the settings file.

The authoritative sources are
[`crates/modde-core/src/settings.rs`](https://github.com/caniko/modde-rs/blob/trunk/crates/modde-core/src/settings.rs),
[`crates/modde-core/src/paths.rs`](https://github.com/caniko/modde-rs/blob/trunk/crates/modde-core/src/paths.rs),
and
[`crates/modde-core/src/instance.rs`](https://github.com/caniko/modde-rs/blob/trunk/crates/modde-core/src/instance.rs).

## `settings.toml`

### Location

`settings.toml` lives in modde's config directory, `<config_dir>/modde/`:

| OS      | Config dir                                                  | Full path                                          |
| ------- | ---------------------------------------------------------- | -------------------------------------------------- |
| Linux   | `$XDG_CONFIG_HOME`, else `~/.config`                       | `~/.config/modde/settings.toml`                    |
| macOS   | `~/Library/Application Support`                            | `~/Library/Application Support/modde/settings.toml`|
| Windows | `%APPDATA%` (e.g. `C:\Users\X\AppData\Roaming`)            | `%APPDATA%\modde\settings.toml`                    |

On Linux, `XDG_CONFIG_HOME` relocates the directory. modde writes the file with
`toml::to_string_pretty` to keep it readable and editable by hand.

### Loading behavior

- A missing file loads as all-defaults.
- A parse error discards the malformed file and loads all defaults instead of failing.
- modde ignores unknown keys, so a newer file won't break an older binary.
- Missing keys take their defaults, so a partial file is valid.

### Keys

| Key                    | Type                       | Default | Description                                                       |
| ---------------------- | -------------------------- | ------- | ----------------------------------------------------------------- |
| `nexus_api_key`        | string                     | `""`    | Legacy inline Nexus API key (lowest-precedence key source — prefer the keyring or `nexus auth`) |
| `game_paths`           | array of `{ game_id, path }` | `[]`  | Configured game install paths (typically 1–4 games)               |
| `download_dir`         | path (optional)            | unset   | Override for the downloads directory                              |
| `theme`                | string                     | `""`    | UI theme name (e.g. `Nord`); empty means the built-in default     |
| `selected_game`        | string (optional)          | unset   | The game last selected in the UI                                  |
| `update_check.enabled` | bool                       | `true`  | Whether modde checks for newer releases on startup                |
| `database`             | table                      | SQLite  | Storage backend and optional PostgreSQL connection settings       |

modde writes `nexus_api_key` only when non-empty, so a clean settings file omits
it. modde reads this **legacy** source for backward compatibility, but it ranks last in the
[Nexus key precedence](#nexus-api-key-precedence). Prefer `modde nexus auth`, the
system keyring, or `NEXUS_API_KEY` / `NEXUS_API_KEY_FILE`.

`game_paths` is an array of tables pairing each `game_id` with its install
`path`. modde looks up paths by `game_id`. Setting a path adds or updates an
entry without creating duplicates.

### Database settings

By default, modde stores profile, mod, tool, save, and snapshot state in SQLite
at `<modde_data>/modde.db`. The default backend doesn't need a `[database]` table:

```toml
[database]
backend = "sqlite"
```

To use PostgreSQL, set `backend = "postgres"` and provide either a full
connection URL or the discrete connection fields. The settings-file database
name key is `dbname`:

```toml
[database]
backend = "postgres"
url = "postgres://modde@localhost/modde"
password_file = "/run/secrets/modde-postgres-password"
```

```toml
[database]
backend = "postgres"
host = "localhost"
port = 5432
dbname = "modde"
user = "modde"
password_file = "/run/secrets/modde-postgres-password"
```

`url` takes precedence over the discrete `host`, `port`, `dbname`, and `user`
fields. Use a URL for details without dedicated settings fields, such as socket
directories or TLS parameters. For the password, `settings.toml` stores only the
`password_file` path; modde reads and trims the file contents at runtime.

Database configuration resolves in this order:

1. Database environment variables.
2. The `[database]` table in `settings.toml`.
3. SQLite defaults.

Within the PostgreSQL layer, `MODDE_DATABASE_URL` wins over
`MODDE_DATABASE_HOST`, `MODDE_DATABASE_PORT`, `MODDE_DATABASE_NAME`, and
`MODDE_DATABASE_USER`, just as `url` wins over the discrete settings fields.
Environment variables override matching settings-file fields individually.
`MODDE_DB_PASSWORD_FILE` overrides `password_file`; both set only the password
file path.

### Example

```toml
nexus_api_key = "legacy-key-if-you-must"
download_dir = "/data/modde/downloads"
theme = "Nord"
selected_game = "cyberpunk2077"

[[game_paths]]
game_id = "cyberpunk2077"
path = "/data/games/Cyberpunk 2077"

[[game_paths]]
game_id = "skyrim-se"
path = "/data/games/Skyrim Special Edition"

[update_check]
enabled = true
```

A minimal file is valid: omitted keys use their defaults.

```toml
selected_game = "skyrim-se"
```

## Data, cache, and downloads directories

`settings.toml` stores only configuration. The mod store, profiles, database,
and downloads live in a separate, platform-specific **data** directory:

| OS      | Data dir base                                |
| ------- | -------------------------------------------- |
| Linux   | `$XDG_DATA_HOME`, else `~/.local/share`      |
| macOS   | `~/Library/Application Support`              |
| Windows | `%APPDATA%`                                  |

The default data root is `<data_dir>/modde/` (see
[Precedence](#data-directory-resolution) for overrides):

| Path                                    | Purpose                                          |
| --------------------------------------- | ------------------------------------------------ |
| `<modde_data>/store/`                   | Content-addressed mod file store                 |
| `<modde_data>/staging/`                 | Staging scratch space                            |
| `<modde_data>/profiles/`               | Per-profile state                                |
| `<modde_data>/downloads/`               | Default downloads directory                      |
| `<modde_data>/stock/`                   | Stock (vanilla) game snapshots                   |
| `<modde_data>/saves/`                   | Save vaults (one git repo per game)              |
| `<modde_data>/wabbajack_cache/`         | Cached `.wabbajack` manifest files               |
| `<modde_data>/modde.db`                 | SQLite database                                  |

The cache directory is `<cache_dir>/modde/` (`$XDG_CACHE_HOME` or `~/.cache` on
Linux, `~/Library/Caches` on macOS, `%LOCALAPPDATA%` on Windows). The downloads
directory defaults to `<modde_data>/downloads/`; the `download_dir` key in
`settings.toml` overrides it.

## Environment variables

| Variable                     | Effect                                                                                             |
| ---------------------------- | -------------------------------------------------------------------------------------------------- |
| `MODDE_DATA_DIR`             | Override the data directory. Equivalent to the global `--data-dir` flag (the flag wins if both set) |
| `NEXUS_API_KEY`              | Nexus API key, read directly from the environment (see [precedence](#nexus-api-key-precedence))     |
| `NEXUS_API_KEY_FILE`         | Path to a file containing the Nexus API key — sops-nix / agenix friendly; set by the home-manager module |
| `MODDE_NO_UPDATE_CHECK`      | When set to `1`/`true`/`yes`/`on`, disables the startup update check regardless of `settings.toml`  |
| `MODDE_UPDATE_CHECK_URL`     | Override the release endpoint queried by the update check (defaults to the Codeberg releases API)   |
| `MODDE_DATABASE_BACKEND`     | Override the storage backend. Accepts `sqlite`, `postgres`, `postgresql`, or `pg`                    |
| `MODDE_DATABASE_URL`         | PostgreSQL connection URL; wins over discrete PostgreSQL connection fields                           |
| `MODDE_DATABASE_HOST`        | PostgreSQL host when no URL is set                                                                  |
| `MODDE_DATABASE_PORT`        | PostgreSQL port when no URL is set                                                                  |
| `MODDE_DATABASE_NAME`        | PostgreSQL database name when no URL is set                                                         |
| `MODDE_DATABASE_USER`        | PostgreSQL user when no URL is set                                                                  |
| `MODDE_DB_PASSWORD_FILE`     | Path to a file containing the PostgreSQL password; the password contents are read at runtime         |
| `XDG_CONFIG_HOME`            | Linux: relocates the config dir (and therefore `settings.toml` and `instances.toml`)                |
| `XDG_DATA_HOME`              | Linux: relocates the data dir base                                                                  |
| `XDG_CACHE_HOME`             | Linux: relocates the cache dir base                                                                 |

Outside normal configuration, `MODDE_HEAP_PROFILE` writes a DHAT heap profile
(requires `heap-profile`), and `MODDE_CHROMIUM` sets the Chromium binary path
for Wabbajack login. `MODDE_TELEMETRY_ENDPOINT` / `MODDE_TELEMETRY_TOKEN` work
only in builds with the optional `remote-telemetry` feature (legacy
`RS_MODDE_TELEMETRY_*` names still work).

Use the global `--data-dir` CLI flag (backed by `MODDE_DATA_DIR`) to override
the data directory for one invocation:

```bash
modde --data-dir /mnt/big/modde profile list
# or, equivalently for the whole session:
MODDE_DATA_DIR=/mnt/big/modde modde profile list
```

## Instance registry (`instances.toml`)

**Instances** are independent, self-contained data directories registered in
`<config_dir>/modde/instances.toml` next to `settings.toml`. Each names a
`data_dir`. You can mark one as default; only one can be active at a time.
If an instance is active, modde uses its `data_dir` (see
[Data directory resolution](#data-directory-resolution)).

Schema:

| Field                | Type                                          | Description                                        |
| -------------------- | --------------------------------------------- | -------------------------------------------------- |
| `active`             | string (optional)                             | Name of the currently active instance              |
| `instances`          | array of `{ name, data_dir, is_default }`     | Registered instances                               |
| `instances[].name`   | string                                        | Unique instance name                               |
| `instances[].data_dir` | path                                        | The instance's self-contained data directory       |
| `instances[].is_default` | bool                                      | Whether this is the default instance               |

Example:

```toml
active = "modding-rig"

[[instances]]
name = "default"
data_dir = "/home/me/.local/share/modde"
is_default = true

[[instances]]
name = "modding-rig"
data_dir = "/mnt/nvme/modde"
is_default = false
```

Creating an instance makes and registers its directory. The first instance
becomes default and active. Switching changes `active`. Names must be unique.

## Configuration precedence

### Data directory resolution

modde uses the first matching data directory:

1. **Explicit override** — the global `--data-dir` flag or `MODDE_DATA_DIR`
   environment variable (the flag takes precedence over the env var). This also
   wins over an in-process override set at startup.
2. **Active instance** — the `data_dir` of the active instance in
   `instances.toml`, if one is set.
3. **Default** — `<data_dir>/modde/`, where `<data_dir>` honors `XDG_DATA_HOME`
   on Linux and the platform data directory elsewhere.

### Nexus API key precedence

When modde needs Nexus credentials, it uses the first source below that yields a
non-empty credential (from
[`crates/modde-sources/src/nexus/auth.rs`](https://github.com/caniko/modde-rs/blob/trunk/crates/modde-sources/src/nexus/auth.rs)):

1. **OAuth token** — a non-expired token from `modde nexus auth` OAuth login.
2. **modde config file** — `<config_dir>/modde/nexus_api_key`, written by
   `modde nexus auth` (created with `0600` permissions on Unix).
3. **`NEXUS_API_KEY`** — the environment variable, read directly.
4. **System keyring** — the secret-service / D-Bus keyring entry
   (`service = "modde"`, `key = "nexus-api-key"`).
5. **`NEXUS_API_KEY_FILE`** — a file path; the file's trimmed contents are used.
   This is the sops-nix-friendly source that the home-manager module's
   `nexus.apiKeyFile` exports.
6. **Legacy `settings.toml`** — the `nexus_api_key` key, kept for backward
   compatibility and ranked last.

If none yield a key, modde reports
`No Nexus API key found. Set NEXUS_API_KEY env var or run "modde nexus auth".`

### How Nix, the settings file, env vars, and the keyring relate

- The **home-manager module** owns the *declarative* surface — profiles,
  Wabbajack lists, Nexus collections, and per-tool config — and drives the CLI on
  every rebuild. It exports `NEXUS_API_KEY_FILE` when you set
  `nexus.apiKeyFile`, and exports `MODDE_DATABASE_*` / `MODDE_DB_PASSWORD_FILE`
  session variables when `programs.modde.database.backend = "postgres"`, but it
  does **not** write `settings.toml` keys like `theme`, `download_dir`, or
  `selected_game`.
- **`settings.toml`** owns the *imperative, user-facing* preferences (game paths,
  download dir, theme, selected game, update-check toggle). The CLI and GUI read
  and write it. For the Nexus key specifically it is the **lowest-precedence**
  source.
- **Environment variables** override at runtime — `MODDE_DATA_DIR` for the data
  dir, `NEXUS_API_KEY` / `NEXUS_API_KEY_FILE` for the key, `MODDE_NO_UPDATE_CHECK`
  for the update check, and `MODDE_DATABASE_*` / `MODDE_DB_PASSWORD_FILE` for
  database selection. They take effect regardless of `settings.toml`.
- The **system keyring** is the recommended interactive store for the Nexus key
  (set it with `modde nexus auth`); it outranks `NEXUS_API_KEY_FILE` and the
  legacy `settings.toml` key, but is itself outranked by `NEXUS_API_KEY` and the
  modde-owned config file.

On every platform, use the CLI/GUI to manage `settings.toml` and the keyring or
environment for the Nexus key. On Nix, the home-manager module adds declarative
profiles and `nexus.apiKeyFile` without replacing the settings file.

## See also

- [Home-Manager module](hm-module.md) — the declarative configuration surface
- [Installation](../getting-started/installation.md) — install channels and the
  home-manager module import
- [Nexus guide](../guides/nexus.md) — authenticating with Nexus Mods
- [Downloads guide](../guides/downloads.md) — the download directory and backends
