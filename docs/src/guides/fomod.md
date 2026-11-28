# FOMOD Installer

## Overview

[FOMOD](https://wiki.nexusmods.com/index.php/FOMOD) is the most common scripted
installer format on Nexus. Instead of a flat archive that drops into the game's
mod directory, a FOMOD ships an XML script that copies files based on your
choices: texture resolution, body type, compatibility patches, and optional
plugins.

modde supports FOMOD two ways:

- **Interactively**, via a step-by-step wizard in the GUI.
- **Declaratively**, via a config file you can generate, edit, version, and
  replay non-interactively. The file makes installs reproducible and lets a
  Home Manager profile pin known-good answers.

During analysis, modde looks for `fomod/ModuleConfig.xml` in the extracted
archive and classifies a match as [`InstallMethod::Fomod`](mod-installation.md).
The game plugin probes first, so an authoritative game-plugin match, such as
Cyberpunk's REDmod, takes precedence over a stray `fomod/` directory.

## The FOMOD format

A FOMOD installer lives in a `fomod/` directory at the archive root and is
driven by two files:

| File | Required | Purpose |
| ---- | -------- | ------- |
| `fomod/ModuleConfig.xml` | yes | The installer script: steps, groups, options, conditions, file operations. |
| `fomod/info.xml` | no | Display metadata: mod name, author, version. modde reads `version` (falling back to `name`) to stamp a revision on generated configs. |

modde locates `ModuleConfig.xml` case-insensitively, so archives that ship
`FOMOD/ModuleConfig.xml` or `fomod/moduleconfig.xml` are handled.

### Structure: steps, groups, options

`ModuleConfig.xml` is a small tree. From outermost to innermost:

- **Module** — the whole installer. Has a name and, optionally, a set of
  *required install files* (copied unconditionally regardless of answers).
- **Install steps** — ordered pages of the wizard. Each step has a name and is
  shown one at a time. A step can be gated by a *visibility condition* so it only
  appears when an earlier answer set the right flag.
- **Groups** — within a step, options are bucketed into named groups. A group has
  a **type** that controls how many options you may pick:

  | Group type | Selection rule |
  | ---------- | -------------- |
  | `SelectExactlyOne` | radio buttons — exactly one option |
  | `SelectAtMostOne` | radio buttons plus "none" |
  | `SelectAtLeastOne` | checkboxes — at least one required |
  | `SelectAny` | checkboxes — any number, including none |
  | `SelectAll` | every option is forced on |

- **Plugins (options)** — the individual choices inside a group. Each has a name,
  an optional description, an optional image, a **plugin type**
  (`Required`, `Optional`, `Recommended`, `NotUsable`, `CouldBeUsable`), the set
  of **files/folders** it copies when selected, and the **flags** it sets.

### Flags and conditions

Selecting an option can set a named **flag** to a value. Later steps, groups,
plugin type descriptors, or a final **conditional file installs** block can read
those flags to decide what to show or copy. For example, choosing 2K textures in
step 1 can install a 2K-only patch at the end. modde applies your declarative
selections, waits for the engine to resolve all flags and conditions, then
computes the file operations.

## Interactive installation

When you install a mod that contains a FOMOD installer, modde detects it
automatically. In the GUI, a step-by-step wizard walks each install step, honors
the group selection rules above, evaluates flags/conditions as you go, and stages
the resulting files. Your selections are saved to the profile (`fomod_config`)
and replayed on future deployments, so re-deploying a profile never re-prompts.

On the CLI, `modde install mod` does **not** open an interactive wizard. A FOMOD
mod installed without answers is staged as *pending user input* — analysis
succeeds but execution returns `RequiresUserInput` until a config is supplied.
For headless installs, generate a config and pass it with `--fomod-config`
(below).

## Declarative installation

For reproducible, non-interactive installs, define your FOMOD choices in a config
file. The on-disk format is a serialized `fomod_oxide::DeclarativeConfig`; modde
can emit it as TOML, JSON, or Nix.

### Generating a config template

```bash
# Generate a template with default selections (recommended/required options)
modde fomod generate /path/to/mod --format toml

# Include all plugins, not just the defaults, so you can flip any of them on
modde fomod generate /path/to/mod --all --format toml
```

The path is a mod directory that contains `fomod/ModuleConfig.xml` (for example
an extracted archive, or a mod's store directory). `generate` prints the config
to stdout — redirect it to a file you then edit:

```bash
modde fomod generate /path/to/mod --all --format toml > my-choices.toml
```

`--all` vs. defaults:

- Without `--all`, the template encodes the installer's **default** answers —
  `Required` and `Recommended` options pre-selected. Good for "I just want the
  author's intended install, pinned."
- With `--all`, **every** option is enumerated so you can toggle any of them.
  Good for "I want the 2K textures and the optional patch, not the defaults."

Output formats:

| `--format` | Notes |
| ---------- | ----- |
| `toml` | Default. The format `--fomod-config` and `fomod apply` accept directly. |
| `json` | Equivalent content; accepted by `fomod apply` when the config path ends in a non-`.toml` extension. |
| `nix` | Requires the `nix` feature on `fomod-oxide`. The stock binary returns an error for `--format nix` — build with that feature, or generate TOML/JSON and convert. |

### Applying a config to a directory

`fomod apply` resolves a config against a mod's `ModuleConfig.xml` and writes the
selected files into a destination directory, without touching any profile:

```bash
modde fomod apply /path/to/mod --config my-choices.toml --dest /path/to/output
```

Use this command in scripts or to test choices and inspect the resulting files.
It prints the file operation count and destination. modde parses TOML when the
config path ends in `.toml` and JSON otherwise.

### During mod installation

To install straight from Nexus with answers pinned, pass the same config to
`install mod`:

```bash
modde install mod https://nexusmods.com/skyrimspecialedition/mods/12345 \
  --profile my-skyrim \
  --fomod-config my-choices.toml
```

modde downloads and extracts the mod, detects the FOMOD, and applies your config
instead of opening the wizard. It records the result and config in the profile:
the same archive and config always stage the same files. Use this path from
Home Manager or other non-interactive automation.

## Inspecting FOMOD structure

See what options a mod offers without installing anything:

```bash
modde fomod inspect /path/to/mod
```

`inspect` prints:

- the **module name**;
- the count of **required install files** (copied unconditionally), if any;
- each **step** with its name;
- each **group** within a step, with its name and group type
  (e.g. `SelectExactlyOne`);
- each **plugin/option** within a group, with its index, name, plugin type,
  file count, and a one-line description preview.

Use it to understand a complex installer before you generate a config — the
index numbers and names line up with what you will edit in the generated TOML.

A typical workflow for a tricky mod:

```bash
# 1. See the shape of the installer.
modde fomod inspect ./extracted-mod

# 2. Generate an editable template with every option exposed.
modde fomod generate ./extracted-mod --all --format toml > choices.toml

# 3. Edit choices.toml to pick the options you want.

# 4. Dry-run the result into a scratch directory and check the files.
modde fomod apply ./extracted-mod --config choices.toml --dest /tmp/fomod-out

# 5. Use the same config for the real, profile-tracked install.
modde install mod <nexus-url> --profile my-skyrim --fomod-config choices.toml
```

## Troubleshooting

### `no fomod/ModuleConfig.xml found in <path>`

`generate`, `apply`, and `inspect` all require a `fomod/` directory with a
`ModuleConfig.xml` inside the path you pass. Common causes:

- You pointed at the archive instead of the extracted directory — extract first.
- The mod has a wrapper directory (`ModName-1.0/fomod/...`). Point at the inner
  directory, or let the full `install` pipeline handle it (its analyzer strips
  single-directory wrappers automatically).
- The mod is simply not a FOMOD. Run `modde fomod inspect` to confirm; if it is a
  plain archive, install it normally — no config needed.

### `failed to parse ModuleConfig.xml` / `FOMOD parse error`

The installer's XML is malformed or uses a construct modde's parser does not
accept. Open `fomod/ModuleConfig.xml` and check for unescaped `&`, mismatched
tags, or a non-UTF-8 encoding. Some old mods ship a hand-edited script; fixing
the XML in place and re-running usually resolves it. If the script is valid but
still rejected, capture it and file an issue.

### `FOMOD apply error` / `invalid config TOML`

The declarative config does not match the installer it is being applied to. This
happens when:

- The config was generated for a **different version** of the mod. Re-run
  `fomod generate` against the current archive — option names and indices change
  between versions.
- You hand-edited the TOML/JSON and introduced a typo, a renamed option, or a
  selection that violates a group rule (for example two options checked in a
  `SelectExactlyOne` group). Re-generate and re-edit minimally.

### `installer requires user input (fomod) — run the wizard first`

You installed a FOMOD mod on the CLI without `--fomod-config`. The mod is staged
but cannot finish until it has answers. Either install it from the GUI (which
runs the wizard) or supply a config:

```bash
modde fomod generate <mod-store-dir> --all --format toml > choices.toml
# edit choices.toml, then re-run install with --fomod-config choices.toml
```

### Step or group does not appear / wrong files installed

FOMOD steps and options can be gated by flags set earlier in the installer. If a
step you expected never shows, an earlier answer did not set the flag that step's
visibility condition requires. Re-run `modde fomod inspect` to review the steps,
then adjust the earlier selections in your config so the dependent step's
condition is satisfied. Because conditions are evaluated by the engine at
`apply`/`resolve` time, the surest check is a `fomod apply` into a scratch
directory followed by inspecting the staged file tree.

## See also

- [How mod installation works](mod-installation.md) — where FOMOD fits in the
  analyze → execute → record pipeline.
- [Installing from Nexus](nexus.md) — acquiring the archives FOMOD installers
  ship in.
- [Deployment](deployment.md) — how staged FOMOD output reaches the game.
- [CLI reference: `modde fomod`](../reference/cli.md) — flag-by-flag command
  reference.
