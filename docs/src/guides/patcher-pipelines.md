# Patcher pipelines

Use profile-scoped patcher stages in `modde deploy` when generated patches must
match the deployed files and active plugin load order. The first stage type
shipped was Synthesis CLI. A second type runs your command with environment
variables set by modde.

Enabled patchers are required deployment artifacts. If an executable, settings
file, Synthesis profile, runtime prerequisite, or command output is missing,
deployment fails and prints the missing artifact, why it matters, how to
regenerate it, and a validation command.

## Synthesis

Install the .NET SDK, unpack the entire Synthesis release zip into a dedicated
tools directory, and use Synthesis itself to create `PipelineSettings.json`.
Synthesis runs as an external tool.

```bash
modde patcher add-synthesis synthesis \
  --profile my-skyrim \
  --game skyrim-se \
  --executable /tools/Synthesis/Synthesis.CLI.exe \
  --pipeline-settings /tools/Synthesis/PipelineSettings.json \
  --synthesis-profile my-skyrim \
  --output-mod synthesis-output \
  --order 10
```

During deploy, modde invokes:

```bash
Synthesis.CLI.exe run-pipeline \
  --OutputDirectory <deployed-data-dir> \
  --PipelineSettingsPath <PipelineSettings.json> \
  --ProfileIdentifier <profile> \
  --DataFolderPath <deployed-data-dir> \
  --LoadOrderFilePath <generated-load-order>
```

modde snapshots the deployed game mod root before and after the run, accepts
only brand-new files plus rewrites of that stage's own previous outputs, moves
those files into `~/.local/share/modde/generated/<game>/<profile>/<stage>/`,
and then re-projects the managed output back into the game before the next
stage runs. A stage that modifies base-deployed files or another stage's output
fails closed.

## Command stages

Command stages are for local scripts or compiled patchers. modde sets these
environment variables:

| Variable | Meaning |
| --- | --- |
| `MODDE_PATCHER_DATA_DIR` | Deployed game data directory |
| `MODDE_PATCHER_LOAD_ORDER_FILE` | Generated active plugin load order |
| `MODDE_PATCHER_PROFILE` | modde profile name |
| `MODDE_PATCHER_GAME` | modde game id |
| `MODDE_PATCHER_STAGE_NAME` | Current patcher stage name |

```bash
modde patcher add-command my-patcher \
  --profile my-skyrim \
  --game skyrim-se \
  --executable /home/me/bin/build-my-patch \
  --arg --strict \
  --env LOG_LEVEL=info \
  --output-mod my-patcher-output \
  --order 20
```

## Managing stages

```bash
modde patcher list --profile my-skyrim --game skyrim-se
modde patcher validate --profile my-skyrim --game skyrim-se
modde patcher run --profile my-skyrim --game skyrim-se
modde patcher run synthesis --profile my-skyrim --game skyrim-se
modde patcher reorder synthesis my-patcher --profile my-skyrim --game skyrim-se
modde patcher disable synthesis --profile my-skyrim --game skyrim-se
modde patcher enable synthesis --profile my-skyrim --game skyrim-se
modde patcher remove my-patcher --profile my-skyrim --game skyrim-se
```

`run` runs the enabled pipeline against the deployed profile without a fresh
base deploy. Pass a stage name to run only that stage. `validate` checks
executables, settings files, and Synthesis profiles without launching any
configured executable or stage.
`reorder` sets the stage order to the supplied names.

## Timeouts, caching, and failure recovery

Every stage has a hard timeout (default 1800 seconds; set per stage with
`--timeout-seconds` on `add-synthesis`/`add-command`). A stage that exceeds it
is killed and the run fails. modde captures stage stdout/stderr to log files
so you can diagnose failures and timeouts afterward.

Stage results are cached: when a stage's settings, output mod, and the
profile's active load order are unchanged since its last successful run, the
stage is skipped with `Skipped patcher stage '<name>' (cache hit)`. Changing
any input — including the load order — invalidates the cache.

Before a run, modde backs up previously generated stage outputs and restores
them if the pipeline fails, so the profile isn't left with half-regenerated
patches.

## Home Manager

```nix
programs.modde.profiles.my-skyrim = {
  game = "skyrim-se";
  patchers = {
    synthesis = {
      type = "synthesis-cli";
      enable = true;
      order = 10;
      outputMod = "synthesis-output";
      settings = {
        executable = "/tools/Synthesis/Synthesis.CLI.exe";
        pipelineSettings = "/tools/Synthesis/PipelineSettings.json";
        synthesisProfile = "my-skyrim";
      };
    };
  };
};
```

Home Manager configures patcher stages before it calls `modde deploy`. Deploy
owns execution order and failure behavior.
