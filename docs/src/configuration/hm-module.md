# Home-Manager Module

If you use Nix, modde's home-manager module lets you **declare your mod profiles
as code**. It is built directly on the flake's package output, and on every
`home-manager switch` it runs an activation script that drives the `modde` CLI to
install and deploy the profiles you declare. This is one powerful option for Nix
users; it sits alongside the cross-platform
[settings file & environment](settings-file.md), which is how everyone configures
modde imperatively (and the only thing you need if you do not use Nix — manage
preferences there and drive modde with the CLI). This page documents **every**
option the module exposes, the validation assertions it enforces, the per-tool
settings schemas, and several complete worked examples.

The option source is [`nix/hm-module.nix`](https://github.com/caniko/rs-modde/blob/trunk/nix/hm-module.nix);
the typed tool-settings schema is generated into
[`nix/tool-schema.nix`](https://github.com/caniko/rs-modde/blob/trunk/nix/tool-schema.nix)
by `modde dev export-tool-schema`. To import the module, see
[Installation → Flake input + home-manager module](../getting-started/installation.md#flake-input--home-manager-module).

## Top-level options

### `programs.modde.enable`

Whether to enable the modde game mod manager. When `false`, the module adds
nothing to your closure and runs no activation work.

- **Type:** `bool`
- **Default:** `false`

### `programs.modde.package`

The modde package to use. Defaults to the flake's `modde` package for your
system, which puts both `modde` (CLI) and `modde-ui` (GUI) on `PATH`. Override it
to pin a specific build or substitute a patched package.

- **Type:** `package`
- **Default:** `flake.packages.${pkgs.system}.modde`

### `programs.modde.nexus.apiKeyFile`

Path to a file containing your Nexus Mods API key. The activation script exports
its path as the `NEXUS_API_KEY_FILE` environment variable, which modde resolves
late in its [API-key lookup chain](settings-file.md#nexus-api-key-precedence) —
making it sops-nix / agenix friendly, since the secret never enters the Nix
store.

- **Type:** `null` or `path`
- **Default:** `null`

```nix
programs.modde.nexus.apiKeyFile = config.sops.secrets.nexus-api-key.path;
```

### `programs.modde.database`

Storage backend configuration for modde's profile, mod, tool, save, and snapshot
state. The default is SQLite; PostgreSQL is configured by setting
`backend = "postgres"` and providing either `url` or at least `name`.

| Option         | Type                       | Default    | Description                                                         |
| -------------- | -------------------------- | ---------- | ------------------------------------------------------------------- |
| `backend`      | enum `sqlite` / `postgres` | `"sqlite"` | Storage backend                                                     |
| `url`          | `null` or `str`            | `null`     | Full PostgreSQL connection URL; wins over discrete fields           |
| `host`         | `null` or `str`            | `null`     | PostgreSQL host when `url` is not set                               |
| `port`         | `null` or port             | `null`     | PostgreSQL port when `url` is not set                               |
| `name`         | `null` or `str`            | `null`     | PostgreSQL database name when `url` is not set                      |
| `user`         | `null` or `str`            | `null`     | PostgreSQL user when `url` is not set                               |
| `passwordFile` | `null` or `path`           | `null`     | Path to a file containing the PostgreSQL password                   |

The Home Manager option is named `name`; modde exports it as
`MODDE_DATABASE_NAME`, and the settings file stores the same value as `dbname`.

When PostgreSQL is selected, the module exports the selected backend and
connection fields as `home.sessionVariables`:

| HM option      | Runtime variable            |
| -------------- | --------------------------- |
| `backend`      | `MODDE_DATABASE_BACKEND`    |
| `url`          | `MODDE_DATABASE_URL`        |
| `host`         | `MODDE_DATABASE_HOST`       |
| `port`         | `MODDE_DATABASE_PORT`       |
| `name`         | `MODDE_DATABASE_NAME`       |
| `user`         | `MODDE_DATABASE_USER`       |
| `passwordFile` | `MODDE_DB_PASSWORD_FILE`    |

Those variables are also exported inside the activation script before it invokes
`modde`. New terminal and graphical sessions launched after activation see
`home.sessionVariables`; already-running shells keep their existing environment.
For an immediate imperative change outside Home Manager, use
[`modde config set-database`](../reference/cli.md#modde-config).

`passwordFile` configures only the path. The password contents are read by modde
at runtime and are never written to `settings.toml` or embedded in the Nix store
by this option.

### `programs.modde.profiles`

An attribute set of mod profiles to manage. The attribute name is the profile
name modde uses for `--profile`. Each value is a [profile submodule](#the-profile-submodule).

- **Type:** `attrsOf` profile submodule
- **Default:** `{}`

## The profile submodule

Each entry under `programs.modde.profiles.<name>` accepts the following options.

### `profiles.<name>.game`

Game identifier string, passed to the CLI as `--game`. This is a free-form
string in the module — modde validates it at runtime — so generic / user-defined
games declared via `modde game add` and a GameSpec TOML work here too.

- **Type:** `str`
- **Required:** yes

The 15 shipping games use identifiers such as `"skyrim-se"`, `"skyrim-ae"`,
`"fallout4"`, `"fallout76"`, `"starfield"`, `"cyberpunk2077"`, and
`"stellar-blade"`. See [Supported games](../games/supported-games.md) for the
full, authoritative list of built-in identifiers.

### `profiles.<name>.gameDir`

Runtime path to the game installation. modde does **not** install base games; it
waits for a launcher-managed install. `gameDir` is required for Wabbajack
modlists that reference local vanilla game files — for example a Skyrim SE list
built from the vanilla `Data` directory.

- **Type:** `null`, `path`, or `str`
- **Default:** `null`

During activation, for Bethesda games (`skyrim-se`, `skyrim-ae`, `fallout4`,
`fallout76`, `starfield`) the module additionally checks that `gameDir` contains
a `Data` subdirectory before installing. If `gameDir` is unset, does not exist,
or is missing the required `Data` directory, a Wabbajack profile prints an
"awaiting game install" message and continues without failing.

### `profiles.<name>.installMode`

Controls what home-manager activation does for this profile.

- **Type:** enum — `"auto"`, `"await-game"`, or `"disabled"`
- **Default:** `"auto"`

| Value          | Behavior                                                                                  |
| -------------- | ----------------------------------------------------------------------------------------- |
| `"auto"`       | Install and deploy when prerequisites are present; otherwise print an awaiting message    |
| `"await-game"` | Always skip install/deploy and print the next setup step (use before the game is present) |
| `"disabled"`   | Skip all activation work for the profile entirely                                         |

### `profiles.<name>.wabbajackList`

Wabbajack modlist source. **Mutually exclusive** with `nexusCollection` (see
[Assertions](#assertions)).

- **Type:** `null` or submodule
- **Default:** `null`

| Option                 | Type                                | Default  | Description                                                     |
| ---------------------- | ----------------------------------- | -------- | -------------------------------------------------------------- |
| `url`                  | `null` or `str`                     | `null`   | URL to the `.wabbajack` modlist file                           |
| `hash`                 | `null` or `str`                     | `null`   | SHA-256 hash of the modlist file (paired with `url`)           |
| `path`                 | `null`, `path`, or `str`            | `null`   | Local or Nix store path to an already-available `.wabbajack`   |
| `missingArchivePolicy` | enum `fail` / `omit-files` / `omit-mods` | `"fail"` | What to do when optional manual/Nexus archives are absent  |
| `manualArchives.<key>` | `attrsOf` submodule                 | `{}`     | User-provided manual / Nexus archives (see below)              |

Set **exactly one** source: either `path`, or both `url` and `hash` together. Use
`path` when composing with `requireFile` or another fetcher that already
materializes the `.wabbajack` file. When `url` + `hash` are used, the module
fetches the modlist with `pkgs.fetchurl` at build time.

`missingArchivePolicy` is forwarded to the CLI as
`--missing-archive-policy` when the profile is installed:

| Policy        | Effect                                                          |
| ------------- | -------------------------------------------------------------- |
| `"fail"`      | Abort the install if any required archive cannot be resolved   |
| `"omit-files"`| Skip individual files sourced from a missing archive           |
| `"omit-mods"` | Skip entire mods whose archive is missing                      |

#### `wabbajackList.manualArchives.<key>`

Manual or Nexus archives that Wabbajack cannot download automatically (premium
Nexus files, off-site mirrors, etc.). Each entry maps an archive to a local
source file. The attribute key may be **either** the Wabbajack xxh64 hex hash
(16 hex characters) **or** a readable label, in which case `hash` must be set
inside the entry.

| Option     | Type                     | Default | Description                                                                   |
| ---------- | ------------------------ | ------- | ----------------------------------------------------------------------------- |
| `hash`     | `null` or `str`          | `null`  | Wabbajack xxh64 hex hash; required when the key is a readable label, not a hash |
| `path`     | `null`, `path`, or `str` | `null`  | Path to the exact source archive for this archive hash                        |
| `optional` | `bool`                   | `false` | Allow activation to omit affected outputs when this archive is absent         |

During activation, every entry with a non-null `path` is imported via
`modde wabbajack import-archive` before the install runs. Required entries (those
not marked `optional`) **must** set `path`, or the module fails an assertion.

### `profiles.<name>.nexusCollection`

Nexus Collection source. **Mutually exclusive** with `wabbajackList`.

- **Type:** `null` or submodule
- **Default:** `null`

| Option    | Type  | Required | Description                   |
| --------- | ----- | -------- | ----------------------------- |
| `slug`    | `str` | yes      | Nexus Collection slug         |
| `version` | `str` | yes      | Collection version to install |

### `profiles.<name>.tools.<id>`

Per-tool configuration for this profile, keyed by tool ID. Every tool slot
defaults to `null` (unconfigured). Setting a tool to an attrset opts it in to the
activation logic — `enable = true` runs `modde tool enable`, while leaving
`enable` at its default of `false` runs `modde tool disable` during activation.

- **Type:** `null` or per-tool submodule
- **Default:** `null` (for each known tool ID)

Recognized tool IDs:

| Tool ID      | Description                                      | `settings` shape |
| ------------ | ------------------------------------------------ | ---------------- |
| `mangohud`   | Performance HUD overlay                          | free-form        |
| `vkbasalt`   | Vulkan post-processing                           | typed            |
| `gamemode`   | System performance tuning                        | typed            |
| `reshade`    | D3D / OpenGL post-processing for Wine-backed games | typed          |
| `optiscaler` | DLSS / FSR / XeSS upscaling                      | free-form        |
| `proton`     | Proton runtime selection and DLL overrides       | free-form        |

Common options on every tool submodule:

| Option              | Type                  | Default | Description                                                       |
| ------------------- | --------------------- | ------- | ----------------------------------------------------------------- |
| `enable`            | `bool`                | `false` | Whether the tool is enabled for this profile                      |
| `applyOnActivation` | `bool`                | `false` | Re-run `modde tool apply` after configuration during activation   |
| `settings`          | typed or free-form attrset | see below | Tool-specific settings (schema depends on the tool)         |
| `release`           | `null` or release submodule | `null` | Pinned release asset (only valid for release-backed tools)   |
| `profile`           | `null` or `str`/enum  | `null`  | OptiScaler per-game preset (only exists on the `optiscaler` tool) |

`settings` is **typed** for `vkbasalt`, `gamemode`, and `reshade` (each key is a
distinct option with its own type, default, and validation derived from the Rust
`settings_schema()`), and **free-form** for `mangohud`, `optiscaler`, and
`proton` (an `attrsOf` accepting `bool`, `int`, `float`, `str`, or a list of
strings). The per-tool key tables are in [Tool settings reference](#tool-settings-reference)
below. The reserved keys `_game_id` and `optiscaler_profile` are ignored if set
directly in `settings` (the module prints a warning and uses the dedicated
`profile` option instead).

### `profiles.<name>.patchers`

Profile-scoped patcher stages configured before activation deploy. `modde
deploy` owns execution, so the same required-stage behavior applies from both
Home Manager and the CLI.

- **Type:** attrs of patcher submodules, keyed by stage name
- **Default:** `{}`

Common options:

| Option      | Type                         | Default | Description                          |
| ----------- | ---------------------------- | ------- | ------------------------------------ |
| `type`      | `synthesis-cli` or `command` | —       | Stage implementation                 |
| `order`     | `int`                        | —       | Execution order                      |
| `enable`    | `bool`                       | `true`  | Whether the stage runs during deploy |
| `outputMod` | `str`                        | —       | Generated output mod                 |
| `strict`    | `bool`                       | `true`  | Forward-compatible; v1 requires true |

Synthesis stages additionally require these `settings`:

| Option              | Type            | Description                         |
| ------------------- | --------------- | ----------------------------------- |
| `executable`        | `path` or `str` | Synthesis CLI executable            |
| `pipelineSettings`  | `path` or `str` | Synthesis `PipelineSettings.json`   |
| `synthesisProfile`  | `str`           | Synthesis profile nickname or GUID  |

Command stages use these `settings`:

| Option        | Type                   | Default | Description                     |
| ------------- | ---------------------- | ------- | ------------------------------- |
| `executable`  | `path` or `str`        | —       | Command executable              |
| `args`        | list of `str`          | `[]`    | Arguments passed to the command |
| `environment` | attrs of `str`         | `{}`    | Extra environment variables     |
| `workingDir`  | `null`, `path`, `str`  | `null`  | Optional working directory      |

```nix
patchers = {
  synthesis = {
    type = "synthesis-cli";
    order = 10;
    outputMod = "synthesis-output";
    settings = {
      executable = "/tools/Synthesis/Synthesis.CLI.exe";
      pipelineSettings = "/tools/Synthesis/PipelineSettings.json";
      synthesisProfile = "my-skyrim";
    };
  };
};
```

#### `tools.<id>.release`

Pins a downloaded release asset for a release-backed tool. Today **only
`optiscaler` supports release pinning** (sourced from
[`nix/release-supporting-tools.nix`](https://github.com/caniko/rs-modde/blob/trunk/nix/release-supporting-tools.nix));
setting `release` on any other tool fails an assertion.

| Option  | Type                     | Default | Description                                            |
| ------- | ------------------------ | ------- | ------------------------------------------------------ |
| `tag`   | `str`                    | —       | Release tag to pin (required)                          |
| `asset` | `str`                    | —       | Release asset file name (required)                     |
| `url`   | `null` or `str`          | `null`  | Download URL for the pinned asset                      |
| `hash`  | `null` or `str`          | `null`  | Fixed-output hash for the pinned asset (paired with `url`) |
| `path`  | `null`, `path`, or `str` | `null`  | Local / store path to an already-downloaded asset      |

Provide the asset **either** by `path` **or** by `url` + `hash` (the two are
mutually exclusive). When `url` + `hash` are set the module materializes the
asset with `pkgs.fetchurl`; with `path` you can hand it a `requireFile` result.
During activation the asset is installed with
`modde tool install-release-from-path <id> --game <game> --tag <tag> --asset <asset> <src>`.

```nix
tools.optiscaler.release = {
  tag = "v0.7.7";
  asset = "OptiScaler_v0.7.7.7z";
  path = pkgs.requireFile {
    name = "OptiScaler_v0.7.7.7z";
    sha256 = "0000000000000000000000000000000000000000000000000000";
    url = "https://github.com/optiscaler/OptiScaler/releases";
  };
};
```

#### `tools.optiscaler.profile`

Selects an OptiScaler per-game preset. This option exists **only** on the
`optiscaler` tool. Its type is dynamic: if the game has registered presets (see
[`nix/optiscaler-profiles.nix`](https://github.com/caniko/rs-modde/blob/trunk/nix/optiscaler-profiles.nix)),
the option becomes an `enum` of those presets; if the game has no registered
presets, setting `profile` to a non-null value fails an assertion.

- **Type:** `null`, `str`, or `enum` of the game's registered presets
- **Default:** `null`

Currently `stellar-blade` registers the `community-dxgi` preset; other games have
no registered presets yet. The chosen preset is forwarded to the CLI as the
`optiscaler_profile=<preset>` setting.

## Assertions

The module evaluates a set of assertions per profile and per configured tool.
Each is reported with a `programs.modde.profiles.<name>...` message so failures
point straight at the offending option.

| Assertion                                | Rule                                                                                                        |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Exclusive source                         | `wabbajackList` and `nexusCollection` are mutually exclusive — set at most one                              |
| Wabbajack source                         | `wabbajackList` must set **exactly one** of: `path`, or `url` + `hash` together                              |
| Wabbajack url/hash pairing               | `wabbajackList.url` and `wabbajackList.hash` must be set together (neither alone)                            |
| Required manual archives                 | Each `manualArchives` entry must set `path`, or mark `optional = true`                                       |
| Manual-archive hashes                    | A `manualArchives` entry keyed by a readable label (not a 16-hex-char hash) must set `hash`                  |
| Duplicate manual-archive hashes          | `manualArchives` entries must not resolve to duplicate hashes                                                |
| Tool release support                     | `release` may only be set on a tool that supports release pinning (today: `optiscaler` only)                 |
| Tool release source                      | `release.path` is mutually exclusive with `release.url` + `release.hash`; exactly one source must be present |
| Tool release url/hash pairing            | `release.url` and `release.hash` must be set together                                                        |
| OptiScaler profile registered            | `tools.optiscaler.profile` may only be non-null if the profile's game has registered OptiScaler presets     |
| PostgreSQL has connection source         | `database.backend = "postgres"` requires either `database.url` or at least `database.name`                   |
| PostgreSQL URL/discrete exclusivity      | Set `database.url` or the discrete `database.host` / `port` / `name` / `user` fields, not both               |
| SQLite has no PostgreSQL fields          | `database.url`, `host`, `port`, `name`, `user`, and `passwordFile` are only valid when `backend = "postgres"` |

## Tool settings reference

The tables below enumerate every key in the typed and notable free-form tool
schemas, generated from
[`nix/tool-schema.nix`](https://github.com/caniko/rs-modde/blob/trunk/nix/tool-schema.nix).
Every key defaults to `null` (unset → modde uses its own default), and each typed
key is validated to its declared type during evaluation.

### vkbasalt (typed)

| Key                  | Type                     | Description                                                  |
| -------------------- | ------------------------ | ------------------------------------------------------------ |
| `casSharpness`       | float (0 – 1, step 0.05) | Contrast Adaptive Sharpening amount                          |
| `effects`            | text                     | Colon/comma-separated effect list, such as `cas` or `cas:fxaa` |
| `enableOnLaunch`     | bool                     | Start with vkBasalt effects enabled                          |
| `reshadeIncludePath` | path                     | Optional ReShade shader include directory for vkBasalt       |
| `reshadeTexturePath` | path                     | Optional ReShade texture directory for vkBasalt              |
| `toggleKey`          | text                     | Key used to toggle vkBasalt                                  |

### gamemode (typed)

`gamemode` exposes no typed settings keys — its schema in `tool-schema.nix` is
empty. Use `tools.gamemode.enable = true;` (and optionally `applyOnActivation`)
to turn it on; there are no per-key settings to configure.

### reshade (typed)

| Key          | Type                                       | Description                                                          |
| ------------ | ------------------------------------------ | ------------------------------------------------------------------- |
| `dll_name`   | enum: `dxgi.dll`, `d3d11.dll`, `dinput8.dll` | DLL name copied into the executable directory                     |
| `source_dir` | path                                       | Directory containing ReShade DLLs, `ReShade.ini`, and shader folders |

### optiscaler (free-form)

`optiscaler` settings are free-form (passed through to modde), but the schema
documents the recognized keys and their accepted values:

| Key                                      | Type / values                                                                 | Description                                                                 |
| ---------------------------------------- | ----------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `copy_companion_files`                   | bool                                                                          | Copy fakenvapi, nvngx wrapper, and other DLLs found next to OptiScaler      |
| `dll_overrides`                          | text                                                                          | Comma/whitespace-separated Wine DLL override base names                     |
| `emulate_fp8`                            | bool                                                                          | Manual FP8-emulation toggle; auto hardware tuning owns this value by default |
| `enable_optipatcher`                     | bool                                                                          | Use OptiPatcher to unlock DLSS / DLSS frame-gen inputs without whole-game spoofing |
| `fsr4_variant`                           | enum: `latest_fp8`, `int8_402`                                                | FSR4 payload copied as `amd_fidelityfx_upscaler_dx12.dll`; auto hardware tuning owns this value by default |
| `hardware_tuning`                        | enum: `auto`, `manual`                                                        | `auto` lets modde choose GPU-specific FSR4 settings; `manual` preserves explicit settings |
| `ini_overrides.FSR.FGIndex`              | enum: `auto`, `0`, `1`                                                        | OptiScaler `[FSR] FGIndex` override                                          |
| `ini_overrides.FSR.UpscalerIndex`        | enum: `auto`, `0`, `1`, `2`                                                   | OptiScaler `[FSR] UpscalerIndex` override                                    |
| `ini_overrides.Menu.Scale`               | float (0.5 – 2, step 0.1)                                                     | OptiScaler `[Menu] Scale` override                                           |
| `ini_overrides.Menu.ShortcutKey`         | enum: `auto`, `INSERT`, `HOME`, `END`, `DELETE`, `BACKQUOTE`, `F1`…`F12`      | OptiScaler `[Menu] ShortcutKey` override                                     |
| `ini_overrides.NvApi.OverrideNvapiDll`   | tri-state bool                                                                | OptiScaler `OverrideNvapiDll` override                                       |
| `ini_overrides.fakenvapi.enable_trace_logs` | tri-state bool                                                             | fakenvapi `enable_trace_logs` override                                      |
| `ini_overrides.fakenvapi.force_latencyflex` | tri-state bool                                                             | fakenvapi `force_latencyflex` override                                      |
| `ini_overrides.fakenvapi.force_reflex`   | enum: `0`, `1`, `2`                                                           | fakenvapi `force_reflex` override                                           |
| `ini_overrides.fakenvapi.latencyflex_mode` | enum: `0`, `1`, `2`                                                         | fakenvapi `latencyflex_mode` override                                       |
| `proxy_dll`                              | enum: `dxgi.dll`, `version.dll`, `dbghelp.dll`, `d3d12.dll`, `wininet.dll`, `winhttp.dll`, `winmm.dll`, `nvngx.dll`, `OptiScaler.asi` | DLL name used to load OptiScaler |
| `source_mode`                            | enum: `github_release`, `goverlay_builds`, `goverlay_fgmod`, `local_dir`     | Where modde should get OptiScaler files from                                 |
| `spoof_dlss`                             | bool                                                                          | Fallback DXGI spoofing path for games that still need whole-game spoofing    |

### proton (free-form, typed values)

`proton` settings are free-form, but most keys are boolean environment-variable
toggles. Notable typed keys:

| Key                    | Type / values                                                | Description                                                |
| ---------------------- | ------------------------------------------------------------ | ---------------------------------------------------------- |
| `version_mode`         | enum: `launcher_default`, `installed_version`, `install_with_protonup_rs` | How modde chooses the Proton runner               |
| `selected_version`     | enum: `latest`                                               | Installed or requested GEProton version                    |
| `install_target`       | enum: `steam`                                                | Target application passed to protonup-rs                   |
| `dll_override_mode`    | enum: `auto`, `forced`, `off`                                | How Proton contributes forced DLL overrides                |
| `forced_dll_overrides` | text                                                         | Comma/whitespace-separated DLL base names (e.g. `dxgi, winmm`) |
| `extra_env`            | text                                                         | Additional `KEY=VALUE` lines exported at launch            |
| `prefix_path_override` | path                                                         | Optional Proton/Wine prefix override                       |
| `wrapper_order`        | enum: `after-modde`, `before-tools`                          | Where Proton wrapper integration appears in the launch chain |

The remaining `proton_*`, `radv_*`, `mesa_*`, `glx_*`, `enable_*`, `staging_*`,
and `steamdeck` keys are booleans that export the matching environment variable
(for example `proton_enable_nvapi = true` exports `PROTON_ENABLE_NVAPI=1`,
`proton_enable_wayland` exports `PROTON_ENABLE_WAYLAND=1`, `radv_perftest_rt`
exports `RADV_PERFTEST=rt,emulate_rt`). See `nix/tool-schema.nix` for the full
list and the exact variable each one sets.

### mangohud (free-form passthrough)

`mangohud` settings are free-form and forwarded to modde verbatim. The schema
documents the full MangoHud key surface (`fps`, `gpu_temp`, `cpu_temp`,
`frametime`, `position`, `font_size`, `background_alpha`, `fps_limit`,
`toggle_hud`, and dozens more), but no key is constrained by the Nix type system
beyond the free-form value types (`bool`, `int`, `float`, `str`, list of `str`).
For example:

```nix
tools.mangohud = {
  enable = true;
  settings = {
    fps = true;
    gpu_temp = true;
    cpu_temp = true;
    frametime = true;
    position = "top-left";
    font_size = 20;
  };
};
```

## Worked examples

### Wabbajack Skyrim profile with `gameDir` and `await-game`

A complete Skyrim SE Wabbajack list, declared before the game is installed. With
`installMode = "await-game"`, the first rebuild prints the next setup step and
does nothing else; once Steam has installed the game, switch to
`installMode = "auto"` and set `gameDir`, and the next rebuild installs and
deploys the list. This example also pins a manual (premium-Nexus) archive.

```nix
{ config, ... }:
{
  programs.modde = {
    enable = true;
    nexus.apiKeyFile = config.sops.secrets.nexus-api-key.path;

    profiles.lotf = {
      game = "skyrim-se";
      installMode = "auto";  # flip from "await-game" once the game exists
      gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
      wabbajackList = {
        url = "https://example.com/legends-of-the-frost.wabbajack";
        hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        missingArchivePolicy = "fail";
        manualArchives = {
          # Keyed by a readable label, so `hash` is required:
          unofficial-patch = {
            hash = "0123456789abcdef";
            path = "/home/me/Downloads/USSEP.7z";
          };
          # An optional archive — activation may omit affected outputs if absent:
          optional-enb = {
            hash = "fedcba9876543210";
            optional = true;
          };
        };
      };
    };
  };
}
```

### Nexus Collection Cyberpunk profile

```nix
programs.modde.profiles.cyberpunk-mods = {
  game = "cyberpunk2077";
  installMode = "auto";
  nexusCollection = {
    slug = "my-cyberpunk-collection";
    version = "2.1.0";
  };
};
```

### MangoHud + OptiScaler with real settings

A Stellar Blade profile that turns on MangoHud with a real overlay layout and
enables OptiScaler with its `community-dxgi` preset, real INI overrides, and a
pinned release asset applied on every activation.

```nix
{ pkgs, ... }:
{
  programs.modde.profiles.stellar-blade = {
    game = "stellar-blade";
    installMode = "auto";
    gameDir = "/home/me/Games/StellarBlade";

    tools = {
      mangohud = {
        enable = true;
        settings = {
          fps = true;
          frametime = true;
          gpu_temp = true;
          cpu_temp = true;
          gpu_load_change = true;
          position = "top-left";
          font_size = 20;
          background_alpha = 0.4;
        };
      };

      optiscaler = {
        enable = true;
        applyOnActivation = true;
        profile = "community-dxgi";
        settings = {
          source_mode = "github_release";
          proxy_dll = "dxgi.dll";
          hardware_tuning = "auto";
          "ini_overrides.FSR.UpscalerIndex" = "auto";
          "ini_overrides.Menu.Scale" = 1.2;
        };
        release = {
          tag = "v0.7.7";
          asset = "OptiScaler_v0.7.7.7z";
          path = pkgs.requireFile {
            name = "OptiScaler_v0.7.7.7z";
            sha256 = "0000000000000000000000000000000000000000000000000000";
            url = "https://github.com/optiscaler/OptiScaler/releases";
          };
        };
      };
    };
  };
}
```

With `applyOnActivation = true`, the release asset is written into the game
directory every time home-manager switches to the profile, keeping the pin in
sync automatically. Leave it off if you prefer to run `modde tool apply` by hand.

### sops-nix `apiKeyFile`

Keep the Nexus API key out of the Nix store entirely by pointing `apiKeyFile`
at a sops-nix (or agenix) secret. The module exports it as `NEXUS_API_KEY_FILE`
during activation, which modde reads through its
[API-key precedence chain](settings-file.md#nexus-api-key-precedence).

```nix
{ config, ... }:
{
  sops.secrets.nexus-api-key = {
    sopsFile = ./secrets.yaml;
    # mode/owner default to the activating user
  };

  programs.modde = {
    enable = true;
    nexus.apiKeyFile = config.sops.secrets.nexus-api-key.path;
    profiles.cyberpunk-mods = {
      game = "cyberpunk2077";
      nexusCollection = {
        slug = "my-cyberpunk-collection";
        version = "2.1.0";
      };
    };
  };
}
```

## First install flow

modde does not install Steam or Heroic games. You can declare a profile before
the game exists by omitting `gameDir` or setting `installMode = "await-game"`.
home-manager activation prints the next step and continues without failing.

After installing the game through Steam or Heroic, set `gameDir` to its install
directory and use `installMode = "auto"`. The next activation installs the
Wabbajack profile (if its lock info is missing), then deploys it and applies the
configured tools. For non-Wabbajack profiles, activation simply runs
`modde deploy` and applies tools.

## See also

- [Settings file & environment](settings-file.md) — the imperative
  `settings.toml`, environment variables, and full Nexus key precedence
- [Installation](../getting-started/installation.md) — install channels and how
  to import the module
- [Supported games](../games/supported-games.md) — authoritative game identifiers
- [Tools guide](../guides/tools.md) — what each tool does at runtime
- [Wabbajack guide](../guides/wabbajack.md) — manual archives and modlist install
