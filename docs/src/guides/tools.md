# Tools & Overlays

## Overview

modde manages performance overlays, graphics post-processing, upscaling and frame generation, performance boosters, and Proton launch settings. For each game and tool, modde stores an enabled flag and JSON settings in its database.

Tool management is **Partial** in the [capability matrix](../reference/parity.md) because it's narrower than Mod Organizer 2's: the tools panel uses the same enable/configure/apply workflow for a fixed set of six tools. To register arbitrary executables, use the separate, fully shipped [Executables & external tools](executables.md) feature.

Each tool contributes one or more of the following at launch or deploy time:

| Mechanism | What it does | Tools that use it |
| --- | --- | --- |
| Environment variable | Exported into the game's launch environment | MangoHud, vkBasalt, OptiScaler, Proton |
| Wrapper command | Chained before the game executable | GameMode |
| Generated config file | Written under `~/.local/share/modde/tools/<game_id>/` | MangoHud, vkBasalt |
| File patch (apply/revert) | DLLs / shaders copied into the executable directory | ReShade, OptiScaler |
| Wine DLL override | Forces Wine to load the native proxy DLL | ReShade, OptiScaler, Proton |

## Supported tools

| Tool ID | Category | Description | Linux only |
| --- | --- | --- | --- |
| `mangohud` | Overlay | Performance HUD (FPS, frame timing, CPU/GPU telemetry) | Yes |
| `vkbasalt` | Post-Processing | Vulkan post-processing layer (CAS sharpening, FXAA, shaders) | Yes |
| `gamemode` | Performance | Feral GameMode wrapper for system performance tuning | Yes |
| `reshade` | Post-Processing | ReShade shader injection via proxy DLL | No (Wine/Proton) |
| `optiscaler` | Upscaler | DLSS/FSR/XeSS upscaling and frame-generation replacement | No (Wine/Proton) |
| `proton` | Performance | Per-game Proton, prefix, environment, and DLL-override settings | No |

## Command summary

All tool commands take `--game <game_id>`. They share the same verbs, but a verb only acts on tools that implement it.

```bash
# Discovery
modde tool list --game skyrim-se        # auto-detect known external tool executables
modde tool status --game skyrim-se      # enabled/disabled + availability for all six tools
modde tool doctor --game skyrim-se      # actionable checks and next commands

# Lifecycle
modde tool enable <tool_id>  --game skyrim-se
modde tool disable <tool_id> --game skyrim-se
modde tool configure <tool_id> --game skyrim-se -- key=value key=value ...
modde tool settings <tool_id> --game skyrim-se

# File patches (ReShade, OptiScaler)
modde tool apply  <tool_id> --game skyrim-se
modde tool revert <tool_id> --game skyrim-se

# Release-backed tools (OptiScaler)
modde tool profiles optiscaler --game skyrim-se
modde tool sources  optiscaler --game skyrim-se
modde tool releases <tool_id> --game skyrim-se
modde tool install-release <tool_id> --game skyrim-se --tag <tag> --asset <asset>
```

`configure` parses each `key=value` pair: `true`/`false` become booleans, anything that parses as a number becomes a number, and everything else is stored as a string. `enable` and `configure` regenerate the tool's config file when it has one; `disable` preserves stored settings so re-enabling restores them.

## Checking status

`modde tool status --game <id>` prints every tool with its category, enabled state, the number of files it has applied, and whether the underlying program is available on the system:

```bash
modde tool status --game stellar-blade
```

MangoHud, vkBasalt, and GameMode check for their binary or Vulkan layer on Linux. Proton reports whether `protonup-rs` is present and counts GE-Proton installs. ReShade and OptiScaler always report "available": you supply their files or modde downloads them on demand. For OptiScaler, `status` also scans the executable directory and prints what it finds (see [OptiScaler](#optiscaler)).

---

## MangoHud

**Category:** Overlay · **Linux only** · enabled via environment variable + generated config.

MangoHud is a performance HUD that draws FPS, frame timing, and CPU/GPU telemetry over the running game.

### How it is enabled

When enabled, MangoHud contributes the environment variable `MANGOHUD=1`. If the per-game config is non-empty, modde also exports `MANGOHUD_CONFIG` pointing at the generated file:

```
~/.local/share/modde/tools/<game_id>/MangoHud.conf
```

### Settings

MangoHud exposes **100+ settings** organised into UI sections: *Layout*, *FPS and Frame Timing*, *CPU*, *GPU*, *Memory and IO*, *Compatibility*, *Logging*, and *Filtering*. A few representative keys:

| Key | Section | Type | Notes |
| --- | --- | --- | --- |
| `position` | Layout | select | `top-left` … `bottom-right` |
| `fps_limit` / `fps_limit_method` | FPS | text / `late`\|`early` | frame-rate cap and limiter mode |
| `background_alpha` | Layout | number 0–1 | HUD background opacity |
| `cpu_temp`, `gpu_temp`, `gpu_junction_temp` | CPU / GPU | bool | per-sensor toggles |
| `custom_text_center` | Layout | text | free-form HUD text |
| `gamemode`, `vkbasalt` | Compatibility | bool | show whether those layers are active |
| `output_folder`, `log_duration`, `upload_logs` | Logging | path / number / bool | benchmark logging |

The fresh-enable defaults turn on `position=top-left`, `fps`, `frametime`, `cpu_stats`, and `gpu_stats`.

MangoHud also powers [`modde perf`](../reference/cli.md#modde-perf), which
captures per-run CSV telemetry and stores summary statistics for comparing
profiles — including as the statistical oracle for
[`modde bisect`](../reference/cli.md#modde-bisect).

### Config generation

The generated `MangoHud.conf` is a flat key/value file. Booleans render as a bare key when `true` (e.g. `fps`) and as `key=0` when `false`; numbers and strings render as `key=value`. Keys beginning with `_` (modde's internal markers such as `_game_id`) are skipped. The file always starts with `# Generated by modde — do not edit manually`.

```bash
modde tool enable mangohud --game skyrim-se
modde tool configure mangohud --game skyrim-se -- \
  position=top-right fps_limit=60 fps_limit_method=late gpu_temp=true custom_text_center=skyrim
```

### Apply / revert

MangoHud writes no files into the game directory, so `apply`/`revert` are no-ops. Disabling the tool stops exporting the env vars; the config file is left on disk and reused on re-enable.

---

## vkBasalt

**Category:** Post-Processing · **Linux only** · enabled via environment variable + generated config.

vkBasalt is a Vulkan post-processing layer that applies Contrast Adaptive Sharpening (CAS), FXAA, and ReShade-compatible shader effects to any Vulkan game (including DXVK/VKD3D titles under Proton).

### How it is enabled

When enabled, vkBasalt contributes `ENABLE_VKBASALT=1` plus `VKBASALT_CONFIG_FILE` pointing at:

```
~/.local/share/modde/tools/<game_id>/vkBasalt.conf
```

Availability detection looks for the implicit Vulkan layer JSON (`/usr/share/vulkan/implicit_layer.d/vkBasalt.json`, `/etc/vulkan/...`), a `vkbasalt` binary on `PATH`, or `vkBasalt.json` under any directory in `VK_LAYER_PATH` (covers Nix profile installs).

### Settings

| Key | Section | Type | Default | Notes |
| --- | --- | --- | --- | --- |
| `toggleKey` | Activation | text | `Home` | key that toggles vkBasalt at runtime |
| `enableOnLaunch` | Activation | bool | `true` | start with effects already active |
| `effects` | Effects | list | `["cas"]` | colon/comma-joined list, e.g. `cas:fxaa` |
| `casSharpness` | Effects | number 0–1 | `0.4` | CAS strength |
| `reshadeTexturePath` | ReShade Paths | path | — | optional ReShade texture dir |
| `reshadeIncludePath` | ReShade Paths | path | — | optional ReShade shader include dir |

### Config generation

The generated `vkBasalt.conf` writes `toggleKey = <key>`, `enableOnLaunch = True/False`, a colon-joined `effects = ...` line, `casSharpness = ...`, and any ReShade paths. It also begins with the `# Generated by modde` banner.

```bash
modde tool enable vkbasalt --game cyberpunk2077
modde tool configure vkbasalt --game cyberpunk2077 -- casSharpness=0.5 toggleKey=F10
```

### Apply / revert

No files are written into the game directory; `apply`/`revert` are no-ops.

---

## GameMode

**Category:** Performance · **Linux only** · enabled via a wrapper command.

GameMode is Feral Interactive's daemon that applies CPU governor, scheduling, and GPU performance tuning for the lifetime of the game process.

### How it is enabled

GameMode uses no environment variables or config files. When enabled, modde prefixes the game executable with `gamemoderun`. Availability detection checks `PATH` for `gamemoderun`.

The only "setting" is a read-only note explaining that the wrapper is applied when enabled.

```bash
modde tool enable gamemode --game skyrim-se
modde tool status --game skyrim-se   # confirm "yes" availability and "enabled"
```

### Apply / revert

No files; `apply`/`revert` are no-ops. Disabling removes `gamemoderun` from the launch chain.

---

## ReShade

**Category:** Post-Processing · Wine/Proton · enabled via proxy-DLL copy + Wine DLL override.

ReShade injects post-processing shaders into DirectX games running under Wine/Proton by placing a **proxy DLL** (typically `dxgi.dll`) next to the game executable. Wine must be told to load the native proxy instead of its built-in stub, so ReShade contributes a `WINEDLLOVERRIDES` entry for the chosen DLL base name.

### Settings

| Key | Section | Type | Notes |
| --- | --- | --- | --- |
| `source_dir` | Source | path | Directory holding ReShade DLLs, `ReShade.ini`, and shader folders. **Required** for apply. |
| `dll_name` | Deployment | select | `dxgi.dll` (default), `d3d11.dll`, or `dinput8.dll` |
| `derived_executable_dir` | Detected Game | read-only | Where files land, derived from the game's metadata |

Because ReShade binaries are Windows files the user supplies, ReShade always reports as "available (user-provided)"; you must point `source_dir` at a folder you have populated.

### Apply / revert

`modde tool apply reshade` copies, into the game's executable directory:

1. the selected proxy DLL (`dll_name`) from `source_dir`,
2. `ReShade.ini` if present, and
3. the `reshade-shaders/` and `reshade-presets/` directories if present (copied recursively).

modde records every file it wrote so `modde tool revert reshade` removes exactly those files. The Wine DLL override (e.g. `dxgi`) is contributed automatically while the tool is enabled.

```bash
modde tool enable reshade --game cyberpunk2077
modde tool configure reshade --game cyberpunk2077 -- \
  source_dir=/home/me/reshade dll_name=dxgi.dll
modde tool apply reshade --game cyberpunk2077
# later …
modde tool revert reshade --game cyberpunk2077
```

During apply, an internal read-only comparison identifies missing, changed, and matching files without writing anything; apply writes only what needs updating, so reapplying after a preset edit only rewrites changed files.

---

## OptiScaler

**Category:** Upscaler · Wine/Proton · enabled via proxy-DLL copy + Wine DLL override (and an optional env var). Release-backed.

OptiScaler replaces a game's upscaler/frame-generation inputs (DLSS/FSR/XeSS) by hooking through a proxy DLL. It also subsumes modde's old **fgmod** DLL-restore logic. This is the most involved tool; it supports release downloading, community profiles, FSR4 payload variants, OptiPatcher, and companion DLLs.

### Sources

Choose the source of OptiScaler files with `source_mode`:

| `source_mode` | UI label | Meaning |
| --- | --- | --- |
| `github_release` | Official GitHub releases | Releases from `optiscaler/OptiScaler` (encoded as `official:<tag>`) |
| `goverlay_builds` | GOverlay builds | The external `benjamimgois/OptiScaler-builds` feed, with a `goverlay_channel` selector |
| `goverlay_fgmod` | GOverlay fgmod directory | An existing local `~/.local/share/goverlay/fgmod` install (the default) |
| `local_dir` | Local OptiScaler directory | A folder you point at with `local_source_dir`, containing `OptiScaler.dll` |

When `source_mode = goverlay_builds`, a `goverlay_channel` setting selects `edge` (bleeding-edge), `stable`, `master`, or `any`. Bleeding-edge FSR 4.0.2-capable builds are typically distributed through the GOverlay builds feed rather than official OptiScaler releases.

### Releases: list and install

Only OptiScaler supports releases (`supports_releases() == true`). `modde tool releases optiscaler` combines official and GOverlay releases and lists installable `.zip`/`.7z` assets. `install-release` downloads and extracts the chosen asset into modde's per-tag cache, then saves the selection to the tool config:

```bash
modde tool releases optiscaler --game stellar-blade
modde tool install-release optiscaler --game stellar-blade \
  --tag official:v0.9.1 --asset OptiScaler.7z
```

Archives are flattened so that `OptiScaler.dll`, `OptiScaler.ini`, companion DLLs, `OptiPatcher.asi`, and the FSR4 payload directories land predictably in the cache. (`.7z` extraction shells out to `7zz`/`7z`.) Installing from a local archive path is also supported internally for offline pinning.

### CLI-first setup

The non-GUI path is `tool setup`, `tool show`, `tool doctor`, `tool diagnose`, `tool preview`, and `tool apply`. Discovery commands are read-only and all detailed inspection commands support `--json` for scripts.

```bash
# See what modde knows before changing anything.
modde tool profiles optiscaler --game stellar-blade
modde tool sources optiscaler --game stellar-blade
modde tool settings optiscaler --game stellar-blade

# Configure only. Does not download newer builds and does not apply files.
modde tool setup optiscaler --game stellar-blade --source auto

# Inspect saved/effective config, env vars, DLL overrides, and apply preview.
modde tool show optiscaler --game stellar-blade

# Get an actionable health summary and next commands.
modde tool doctor --game stellar-blade optiscaler

# Diagnose GPU tuning, source payloads, missing inputs, and active proxies.
modde tool diagnose optiscaler --game stellar-blade

# Apply once preview/diagnose are clean.
modde tool setup optiscaler --game stellar-blade --source auto --apply
```

`--source auto` is conservative: it reuses the current source if it is suitable, or a cached GOverlay build that already contains both `FSR4_INT8/` and `FSR4_LATEST/`. It does **not** upgrade or download by default. To intentionally select and install the newest matching release, pass `--upgrade`:

```bash
modde tool setup optiscaler --game stellar-blade --source goverlay-edge --upgrade --apply
```

You can also pin exact inputs without using the GUI:

```bash
modde tool setup optiscaler --game stellar-blade \
  --profile community-dxgi \
  --source goverlay-edge \
  --release-tag goverlay-edge:edge-0.9.12.0323 \
  --release-asset optiscaler-edge.7z \
  --hardware-tuning auto \
  --apply
```

### Key settings

| Key | Type | Notes |
| --- | --- | --- |
| `proxy_dll` | select | DLL OptiScaler is loaded as: `dxgi.dll` (default), `version.dll`, `dbghelp.dll`, `d3d12.dll`, `wininet.dll`, `winhttp.dll`, `winmm.dll`, `nvngx.dll`, or `OptiScaler.asi` |
| `dll_overrides` | text | Extra Wine DLL override base names (comma/space separated) |
| `copy_companion_files` | bool (default on) | Copy `fakenvapi.dll`, `nvngx-wrapper.dll`, and other DLLs found beside OptiScaler |
| `hardware_tuning` | select | `auto` lets modde choose GPU-specific FSR4 settings; `manual` preserves explicit FSR4 settings |
| `fsr4_variant` | select/read-only | `latest_fp8` ("Latest (FP8)") or `int8_402` ("4.0.2c (INT8)") — copied as `amd_fidelityfx_upscaler_dx12.dll`; read-only when `hardware_tuning=auto` |
| `emulate_fp8` | bool/read-only | Only meaningful for the FP8 variant; read-only when `hardware_tuning=auto` |
| `enable_optipatcher` | bool | Deploy `plugins/OptiPatcher.asi` to unlock DLSS/DLSS-FG inputs without whole-game spoofing |
| `spoof_dlss` | bool | Fallback DXGI spoofing path for games that still need it |

OptiScaler `.ini` overrides (`ini_overrides.*`) include the in-game menu shortcut, menu scale, FSR upscaler/frame-generation backend indices, and fakenvapi/LatencyFlex toggles. modde reads additional `.ini` keys from the selected release's `OptiScaler.ini` and exposes them as advanced overrides.

### FSR4 variants and FP8 emulation

`fsr4_variant` selects which FSR4 payload is copied as `amd_fidelityfx_upscaler_dx12.dll`: `latest_fp8` (FP8) or `int8_402` (INT8, 4.0.2c). The two payloads ship in `FSR4_LATEST/` and `FSR4_INT8/` directories inside the source.

By default `hardware_tuning=auto` owns GPU-sensitive FSR4 settings after any game profile has been applied:

| GPU | Auto tuning |
| --- | --- |
| AMD RDNA3 / RX 7000 | `fsr4_variant=int8_402`, `emulate_fp8=false` |
| AMD RDNA4 / RX 9000 | `fsr4_variant=latest_fp8`, `emulate_fp8=false` |
| AMD legacy, NVIDIA, Intel, unknown | Preserve the configured/default FSR4 setting |

Set `hardware_tuning=manual` only when intentionally testing or overriding the GPU-specific default. In manual mode modde preserves `fsr4_variant` and `emulate_fp8`, but still warns about combinations known to be unsafe, such as RDNA3 with the FP8 FSR4 model.

### OptiPatcher and companion DLLs

When `enable_optipatcher` is on, apply deploys `plugins/OptiPatcher.asi` and sets `Plugins.LoadAsiPlugins=true` in the merged `.ini`. If the patcher is not cached, `install-release` fetches the latest `OptiPatcher.asi` from `optiscaler/OptiPatcher`. With `copy_companion_files` on, any extra `.dll` next to OptiScaler (fakenvapi, nvngx wrapper, etc.) is copied alongside the proxy.

### Install scanning and backups

Before apply, OptiScaler scans the executable directory and classifies the install as `absent`, `managed`, `unmanaged`, `partially managed`, or `conflicted` (more than one recognised proxy DLL). `modde tool status` prints this as a one-line summary, e.g.:

```
OptiScaler install: unmanaged; version v0.9.1; proxy dxgi.dll
```

For an *unmanaged*, *partially managed*, or *conflicted* install, apply first backs up recognised files with a JSON manifest so you can recover the previous state. If you adopt a working install instead of reinstalling, modde records its files as managed.

### fgmod restore

OptiScaler carries the fgmod DLL-restore behaviour: fgmod deletes certain DLLs at launch (`dxgi.dll`, `winmm.dll`, `nvngx.dll`, `_nvngx.dll`, `nvngx-wrapper.dll`, `dlss-enabler.dll`, `OptiScaler.dll`). modde's launch wrapper scans the staging `mods/<mod>/bin/x64` directories and restores any of those DLLs into the executable directory so a re-deploy does not leave the game broken.

### Community profiles

For some games modde ships community-tested OptiScaler profiles (game-owned metadata). Selecting a profile via `optiscaler_profile=<id>` applies its `proxy_dll`, source, OptiPatcher flag, `.ini` overrides, and records its tested version, source URL, and notes. Profiles may carry historical FSR4 hints, but `hardware_tuning=auto` is the final authority for GPU-specific FSR4 settings. Profiles never imply OptiScaler is enabled by default.

For **Stellar Blade** the default profile is `community-dxgi`, which uses `dxgi.dll` as the proxy and enables OptiPatcher so DLSS/DLSS-FG inputs are unlocked without spoofing. You can also define your own profiles in `~/.local/share/modde/games/<game_id>.optiscaler.toml` (an `[[optiscaler.profile]]` array); user profiles merge over built-ins of the same `id`.

```bash
modde tool setup optiscaler --game stellar-blade --source auto --apply
```

See the deeper deployment notes in the supported-game page for [Stellar Blade](../games/stellar-blade.md) and [Cyberpunk 2077](../games/cyberpunk2077.md).

---

## Proton

**Category:** Performance · enabled via launch environment + DLL overrides. Does **not** install Proton or the game.

The `proton` tool stores per-game launch-compatibility settings that feed modde's existing env-var, DLL-override, and wrapper collection. It does not install Steam, the game, or a Proton runner.

### Runner selection

| Key | Type | Notes |
| --- | --- | --- |
| `version_mode` | select | `launcher_default` (use the launcher's runner), `installed_version` (use a specific installed GE-Proton), or `install_with_protonup_rs` (request an install via protonup-rs) |
| `selected_version` | select | `latest` or a specific GE-Proton tag; the option list merges the GE-Proton catalog with locally installed compatibility tools |
| `install_target` | select | Target passed to protonup-rs (`steam`) |

`version_mode = launcher_default` is the default and needs nothing else. The UI can load the upstream GE-Proton catalog from `GloriousEggroll/proton-ge-custom` and merge it with installed runners found under Steam's `compatibilitytools.d` directories. Installing a selected version still requires `protonup-rs` on `PATH`; modde invokes it non-interactively (`--tool GEProton --version <tag> --for steam`).

### Environment, prefix, and overrides

| Key | Type | Notes |
| --- | --- | --- |
| `prefix_path_override` | path | Exports `WINEPREFIX`; blank uses launcher detection |
| `extra_env` | text | Extra `KEY=VALUE` lines (one per line; `#` comments ignored) exported at launch |
| `dll_override_mode` | select | `auto`, `forced`, or `off` — how Proton contributes forced DLL overrides |
| `forced_dll_overrides` | text | Comma/space-separated DLL base names, e.g. `dxgi, winmm` |
| `wrapper_order` | select | `after-modde` or `before-tools` — where Proton integration sits in the launch chain |

When `dll_override_mode` is not `off`, the names in `forced_dll_overrides` (with any `.dll` suffix stripped) are contributed as Wine DLL overrides.

### Compatibility toggles (40+ env flags)

Proton exposes **40+ boolean toggles**, each of which exports one environment variable when set. A non-exhaustive sample:

| Setting | Exports |
| --- | --- |
| `steamdeck` | `SteamDeck=1` |
| `proton_enable_hdr` | `PROTON_ENABLE_HDR=1` |
| `enable_hdr_wsi` | `ENABLE_HDR_WSI=1` |
| `proton_enable_wayland` | `PROTON_ENABLE_WAYLAND=1` |
| `proton_log` | `PROTON_LOG=1` |
| `radv_perftest_rt` | `RADV_PERFTEST=rt,emulate_rt` |
| `proton_enable_nvapi` | `PROTON_ENABLE_NVAPI=1` |
| `mesa_loader_zink` | `MESA_LOADER_DRIVER_OVERRIDE=zink` |
| `proton_fsr4_upgrade` | `PROTON_FSR4_UPGRADE=1` |
| `proton_dlss_upgrade` | `PROTON_DLSS_UPGRADE=1` |
| `proton_xess_upgrade` | `PROTON_XESS_UPGRADE=1` |
| `proton_use_wow64` | `PROTON_USE_WOW64=1` |
| `proton_no_ntsync` | `PROTON_NO_NTSYNC=1` |
| `enable_mesa_antilag` | `ENABLE_LAYER_MESA_ANTI_LAG=1` |

```bash
# Keep the launcher's default runner
modde tool configure proton --game cyberpunk2077 -- version_mode=launcher_default

# Export extra variables and enable HDR
modde tool configure proton --game cyberpunk2077 -- \
  proton_enable_hdr=true extra_env='PROTON_LOG=1'

# Force DLL overrides a mod needs
modde tool configure proton --game cyberpunk2077 -- \
  dll_override_mode=forced forced_dll_overrides=dxgi,winmm
```

### Apply / revert

Proton writes no files into the game directory; it only contributes environment variables and DLL overrides at launch. There is nothing to `apply`/`revert`.

---

## See also

- [Executables & external tools](executables.md) — the shipped (Done) named-executable manager for xEdit, BodySlide, Nemesis, etc.
- [Playing & launching](playing.md) — how launch env vars and wrappers reach the game
- [Deployment & VFS](deployment.md) — how applied files and staging interact
- [Supported games](../games/supported-games.md) — per-game OptiScaler/Proton notes
- [Feature parity](../reference/parity.md) — why tool management is Partial
