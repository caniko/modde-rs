flake: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.modde;
  toolSchema = import ./tool-schema.nix;
  optiscalerProfilesByGame = import ./optiscaler-profiles.nix;
  releaseSupportingToolIds = import ./release-supporting-tools.nix;
  knownToolIds = [
    "mangohud"
    "vkbasalt"
    "gamemode"
    "reshade"
    "optiscaler"
    "proton"
  ];
  typedToolIds = [
    "gamemode"
    "vkbasalt"
    "reshade"
  ];
  freeformToolSettingType = lib.types.oneOf [
    lib.types.bool
    lib.types.int
    lib.types.float
    lib.types.str
    (lib.types.listOf lib.types.str)
  ];
  freeformToolSettingsType = lib.types.attrsOf freeformToolSettingType;
  releaseToolType = lib.types.submodule {
    options = {
      tag = lib.mkOption {
        type = lib.types.str;
        description = "Release tag to pin.";
      };
      asset = lib.mkOption {
        type = lib.types.str;
        description = "Release asset name.";
      };
      url = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Download URL for the pinned release asset.";
      };
      hash = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Fixed-output hash for the pinned release asset.";
      };
      path = lib.mkOption {
        type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
        default = null;
        description = "Local path to an already-downloaded release asset.";
      };
    };
  };
  nixBaseTypeFromSpec = spec:
    if spec.type == "bool" || spec.type == "tri_state_bool"
    then lib.types.bool
    else if spec.type == "int"
    then lib.types.int
    else if spec.type == "float"
    then lib.types.float
    else if spec.type == "text" || spec.type == "path"
    then lib.types.str
    else if spec.type == "enum"
    then lib.types.enum spec.values
    else throw "programs.modde: unsupported generated tool schema type '${spec.type}'";
  optiscalerProfilesForGame = game: optiscalerProfilesByGame.${game} or [];
  validateTypedSetting = toolId: key: spec: value:
    if value == null
    then null
    else if spec.type == "bool" || spec.type == "tri_state_bool"
    then
      if builtins.isBool value
      then value
      else throw "programs.modde.profiles.<name>.tools.${toolId}.settings.${key}: expected bool"
    else if spec.type == "int"
    then
      if builtins.isInt value
      then value
      else throw "programs.modde.profiles.<name>.tools.${toolId}.settings.${key}: expected int"
    else if spec.type == "float"
    then
      if builtins.isFloat value || builtins.isInt value
      then value
      else throw "programs.modde.profiles.<name>.tools.${toolId}.settings.${key}: expected float"
    else if spec.type == "text" || spec.type == "path"
    then
      if builtins.isString value
      then value
      else throw "programs.modde.profiles.<name>.tools.${toolId}.settings.${key}: expected string"
    else if spec.type == "enum"
    then
      if builtins.isString value && lib.elem value spec.values
      then value
      else throw "programs.modde.profiles.<name>.tools.${toolId}.settings.${key}: expected one of ${lib.concatStringsSep ", " spec.values}"
    else throw "programs.modde: unsupported generated tool schema type '${spec.type}'";
  toolType = toolId: {game, ...}: {
    options =
      {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether the tool should be enabled for this profile.";
        };

        applyOnActivation = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether to apply the tool after configuration during activation.";
        };

        release = lib.mkOption {
          type = lib.types.nullOr releaseToolType;
          default = null;
          description = "Pinned release asset for release-backed tools.";
        };
      }
      // (
        if lib.elem toolId typedToolIds
        then {
          settings = lib.mapAttrs (
            key: spec:
              lib.mkOption {
                type = lib.types.nullOr (nixBaseTypeFromSpec spec);
                inherit (spec) default;
                inherit (spec) description;
                apply = validateTypedSetting toolId key spec;
              }
          ) (toolSchema.${toolId} or {});
        }
        else {
          settings = lib.mkOption {
            type = freeformToolSettingsType;
            default = {};
            description = "Free-form tool settings passed through to modde.";
          };
        }
      )
      // (
        if toolId == "optiscaler"
        then {
          profile = lib.mkOption {
            type = let
              profilesForGame = optiscalerProfilesForGame game;
            in
              lib.types.nullOr (
                if profilesForGame == []
                then lib.types.str
                else lib.types.enum profilesForGame
              );
            default = null;
            description = "OptiScaler per-game profile preset.";
          };
        }
        else {}
      );
  };
  patcherType = lib.types.submodule {
    options = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Whether this patcher stage runs during modde deploy.";
      };
      type = lib.mkOption {
        type = lib.types.enum ["synthesis-cli" "command"];
        description = "Patcher stage implementation.";
      };
      order = lib.mkOption {
        type = lib.types.int;
        description = "Execution order for this stage.";
      };
      outputMod = lib.mkOption {
        type = lib.types.str;
        description = "User-facing name for the managed generated output.";
      };
      strict = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Forward-compatible failure policy knob. v1 requires true.";
      };
      settings = lib.mkOption {
        type = lib.types.submodule {
          options = {
            executable = lib.mkOption {
              type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
              default = null;
              description = "Executable path for Synthesis CLI or the command stage.";
            };
            pipelineSettings = lib.mkOption {
              type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
              default = null;
              description = "Synthesis PipelineSettings.json path.";
            };
            synthesisProfile = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "Synthesis profile nickname or GUID.";
            };
            args = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [];
              description = "Arguments for command patcher stages.";
            };
            environment = lib.mkOption {
              type = lib.types.attrsOf lib.types.str;
              default = {};
              description = "Environment variables for command patcher stages.";
            };
            workingDir = lib.mkOption {
              type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
              default = null;
              description = "Optional working directory for command patcher stages.";
            };
          };
        };
        default = {};
        description = "Type-specific patcher stage settings.";
      };
    };
  };
  profileType = lib.types.submodule ({config, ...}: {
    options = {
      game = lib.mkOption {
        type = lib.types.str;
        description = "Game identifier string (e.g. 'skyrim-se', 'fallout4').";
      };

      gameDir = lib.mkOption {
        type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
        default = null;
        description = ''
          Runtime path to the game installation. Required for Wabbajack
          modlists that reference local game files, such as Skyrim SE lists
          built from vanilla Data files.
        '';
      };

      installMode = lib.mkOption {
        type = lib.types.enum ["auto" "await-game" "disabled"];
        default = "auto";
        description = ''
          Controls Home Manager activation for this profile. "auto" installs
          and deploys when prerequisites are present, "await-game" prints the
          next setup step without installing, and "disabled" skips all
          activation work for the profile.
        '';
      };

      wabbajackList = lib.mkOption {
        type = lib.types.nullOr (lib.types.submodule {
          options = {
            url = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "URL to the .wabbajack modlist file.";
            };
            hash = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "SHA-256 hash of the modlist file.";
            };
            path = lib.mkOption {
              type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
              default = null;
              description = "Local or Nix store path to an already available .wabbajack file.";
            };
            missingArchivePolicy = lib.mkOption {
              type = lib.types.enum ["fail" "omit-files" "omit-mods"];
              default = "fail";
              description = "Behavior when optional manual/Nexus Wabbajack archives are absent.";
            };
            manualArchives = lib.mkOption {
              type = lib.types.attrsOf (lib.types.submodule {
                options = {
                  hash = lib.mkOption {
                    type = lib.types.nullOr lib.types.str;
                    default = null;
                    description = ''
                      Wabbajack xxh64 hex hash for this archive. Required
                      when the manualArchives attribute name is a readable
                      label instead of the hash itself.
                    '';
                  };
                  path = lib.mkOption {
                    type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
                    default = null;
                    description = "Path to the exact source archive for this Wabbajack archive hash.";
                  };
                  optional = lib.mkOption {
                    type = lib.types.bool;
                    default = false;
                    description = "Allow activation to omit affected outputs when this archive is absent.";
                  };
                };
              });
              default = {};
              description = ''
                User-provided manual/Nexus archives. Entries may be keyed by
                Wabbajack xxh64 hex hash, or by a readable label when hash is
                set inside the entry.
              '';
            };
          };
        });
        default = null;
        description = ''
          Wabbajack modlist source (mutually exclusive with nexusCollection).
          Set either path, or both url and hash.
        '';
      };

      nexusCollection = lib.mkOption {
        type = lib.types.nullOr (lib.types.submodule {
          options = {
            slug = lib.mkOption {
              type = lib.types.str;
              description = "Nexus Collection slug.";
            };
            version = lib.mkOption {
              type = lib.types.str;
              description = "Collection version to install.";
            };
          };
        });
        default = null;
        description = "Nexus Collection source (mutually exclusive with wabbajackList).";
      };

      tools = lib.mkOption {
        type = lib.types.submodule {
          options = lib.genAttrs knownToolIds (
            toolId: let
              toolSubmodule = lib.types.submoduleWith {
                modules = [(toolType toolId)];
                specialArgs = {
                  inherit (config) game;
                };
              };
            in
              lib.mkOption {
                type = lib.types.nullOr toolSubmodule;
                default = null;
                description = "Per-tool configuration for ${toolId}.";
              }
          );
        };
        default = {};
        description = "Per-tool configuration for this profile.";
      };

      patchers = lib.mkOption {
        type = lib.types.attrsOf patcherType;
        default = {};
        description = "Profile-scoped patcher stages that run during deployment.";
      };
    };
  });
  profileAssertions = lib.attrValues (
    lib.concatMapAttrs (
      name: profile: let
        hasWabbajack = profile.wabbajackList != null;
        hasPath = hasWabbajack && profile.wabbajackList.path != null;
        hasUrlHash =
          hasWabbajack
          && profile.wabbajackList.url != null
          && profile.wabbajackList.hash != null;
        hasPartialUrlHash =
          hasWabbajack
          && (profile.wabbajackList.url != null || profile.wabbajackList.hash != null)
          && !hasUrlHash;
        isManualArchiveHashKey = key: builtins.match "[0-9a-fA-F]{16}" key != null;
        resolveManualArchiveHash = key: archive:
          if archive.hash != null
          then archive.hash
          else key;
        manualArchiveEntries =
          if hasWabbajack
          then
            lib.mapAttrsToList (
              key: archive: {
                inherit key archive;
                resolvedHash = resolveManualArchiveHash key archive;
              }
            )
            profile.wabbajackList.manualArchives
          else [];
        resolvedManualArchiveHashes = map (entry: entry.resolvedHash) manualArchiveEntries;
        configuredTools = lib.filterAttrs (_toolId: toolCfg: toolCfg != null) profile.tools;
        patcherNames = builtins.attrNames profile.patchers;
        patcherEntries = lib.mapAttrsToList (patcherName: patcher: {inherit patcherName patcher;}) profile.patchers;
        patcherOutputMods = map (entry: entry.patcher.outputMod) patcherEntries;
        toolAssertions =
          lib.concatMapAttrs (
            toolId: toolCfg: let
              hasRelease = toolCfg.release != null;
              releasePath = hasRelease && toolCfg.release.path != null;
              releaseUrl = hasRelease && toolCfg.release.url != null;
              releaseHash = hasRelease && toolCfg.release.hash != null;
              hasUrlHash = hasRelease && releaseUrl && releaseHash;
              hasAnyUrlHash = hasRelease && (releaseUrl || releaseHash);
              hasPartialUrlHash =
                hasRelease
                && (releaseUrl != releaseHash)
                && !hasUrlHash
                && !releasePath;
              supportsRelease = lib.elem toolId releaseSupportingToolIds;
              optiscalerProfiles = optiscalerProfilesForGame profile.game;
            in
              {
                "${name}-${toolId}-release-supported" = {
                  assertion = !hasRelease || supportsRelease;
                  message = "programs.modde.profiles.${name}.tools.${toolId}.release: ${toolId} does not support release pinning.";
                };
                "${name}-${toolId}-release-source" = {
                  assertion = !hasRelease || (!(releasePath && hasAnyUrlHash) && (releasePath || hasUrlHash));
                  message = "programs.modde.profiles.${name}.tools.${toolId}.release: release.path is mutually exclusive with release.url + release.hash.";
                };
                "${name}-${toolId}-release-url-hash" = {
                  assertion = !hasPartialUrlHash;
                  message = "programs.modde.profiles.${name}.tools.${toolId}.release: release.url and release.hash must be set together.";
                };
              }
              // lib.optionalAttrs (toolId == "optiscaler" && optiscalerProfiles == []) {
                "${name}-optiscaler-profile-registered" = {
                  assertion = toolCfg.profile == null;
                  message = "programs.modde.profiles.${name}.tools.optiscaler.profile: no profiles registered for game ${profile.game}.";
                };
              }
          )
          configuredTools;
      in
        {
          "${name}-exclusive-source" = {
            assertion = !(profile.wabbajackList != null && profile.nexusCollection != null);
            message = "programs.modde.profiles.${name}: wabbajackList and nexusCollection are mutually exclusive.";
          };
          "${name}-wabbajack-source" = {
            assertion = !hasWabbajack || (hasPath != hasUrlHash);
            message = "programs.modde.profiles.${name}: wabbajackList must set exactly one source: path, or url plus hash.";
          };
          "${name}-wabbajack-url-hash" = {
            assertion = !hasPartialUrlHash;
            message = "programs.modde.profiles.${name}: wabbajackList url and hash must be set together.";
          };
          "${name}-wabbajack-required-manual-archives" = {
            assertion =
              !hasWabbajack
              || lib.all (archive: archive.optional || archive.path != null)
              (lib.attrValues profile.wabbajackList.manualArchives);
            message = "programs.modde.profiles.${name}: required manualArchives entries must set path, or mark optional = true.";
          };
          "${name}-wabbajack-manual-archive-hashes" = {
            assertion =
              !hasWabbajack
              || lib.all (
                entry:
                  entry.archive.hash != null || isManualArchiveHashKey entry.key
              )
              manualArchiveEntries;
            message = "programs.modde.profiles.${name}: readable manualArchives entries must set hash.";
          };
          "${name}-wabbajack-manual-archive-duplicate-hashes" = {
            assertion =
              !hasWabbajack
              || builtins.length resolvedManualArchiveHashes
              == builtins.length (lib.unique resolvedManualArchiveHashes);
            message = "programs.modde.profiles.${name}: manualArchives entries resolve to duplicate hashes.";
          };
          "${name}-patcher-unique-names" = {
            assertion = builtins.length patcherNames == builtins.length (lib.unique patcherNames);
            message = "programs.modde.profiles.${name}.patchers: patcher stage names must be unique.";
          };
          "${name}-patcher-unique-output-mods" = {
            assertion = builtins.length patcherOutputMods == builtins.length (lib.unique patcherOutputMods);
            message = "programs.modde.profiles.${name}.patchers: outputMod values must be unique.";
          };
        }
        // lib.listToAttrs (
          map (
            entry: {
              name = "${name}-patcher-${entry.patcherName}-synthesis-fields";
              value = {
                assertion =
                  entry.patcher.type
                  != "synthesis-cli"
                  || (
                    entry.patcher.settings.executable
                    != null
                    && entry.patcher.settings.pipelineSettings != null
                    && entry.patcher.settings.synthesisProfile != null
                    && entry.patcher.settings.args == []
                    && entry.patcher.settings.environment == {}
                    && entry.patcher.settings.workingDir == null
                  );
                message = "programs.modde.profiles.${name}.patchers.${entry.patcherName}: synthesis-cli requires settings.executable, settings.pipelineSettings, and settings.synthesisProfile only.";
              };
            }
          )
          patcherEntries
        )
        // lib.listToAttrs (
          map (
            entry: {
              name = "${name}-patcher-${entry.patcherName}-command-fields";
              value = {
                assertion =
                  entry.patcher.type
                  != "command"
                  || (
                    entry.patcher.settings.executable
                    != null
                    && entry.patcher.settings.pipelineSettings == null
                    && entry.patcher.settings.synthesisProfile == null
                  );
                message = "programs.modde.profiles.${name}.patchers.${entry.patcherName}: command patchers require settings.executable and must not set synthesis-only settings.";
              };
            }
          )
          patcherEntries
        )
        // lib.listToAttrs (
          map (
            entry: {
              name = "${name}-patcher-${entry.patcherName}-strict";
              value = {
                assertion = entry.patcher.strict;
                message = "programs.modde.profiles.${name}.patchers.${entry.patcherName}: strict must be true in v1.";
              };
            }
          )
          patcherEntries
        )
        // toolAssertions
    )
    cfg.profiles
  );
  renderToolSettingValue = value:
    if builtins.isBool value
    then
      if value
      then "true"
      else "false"
    else if builtins.isInt value
    then toString value
    else if builtins.isFloat value
    then toString value
    else if builtins.isString value
    then value
    else lib.concatStringsSep "," value;
  shellArg = value:
    if builtins.match "[A-Za-z0-9_./:=+@,% -]+" value != null && !(lib.hasInfix " " value)
    then value
    else lib.escapeShellArg value;
  renderTypedToolSettingValue = toolId: key: value: let
    spec = toolSchema.${toolId}.${key};
    mismatch = expected:
      throw "programs.modde.profiles.<name>.tools.${toolId}.settings.${key}: expected ${expected}";
  in
    if spec.type == "bool" || spec.type == "tri_state_bool"
    then
      if builtins.isBool value
      then renderToolSettingValue value
      else mismatch "bool"
    else if spec.type == "int"
    then
      if builtins.isInt value
      then renderToolSettingValue value
      else mismatch "int"
    else if spec.type == "float"
    then
      if builtins.isFloat value || builtins.isInt value
      then renderToolSettingValue value
      else mismatch "float"
    else if spec.type == "text" || spec.type == "path"
    then
      if builtins.isString value
      then renderToolSettingValue value
      else mismatch "string"
    else if spec.type == "enum"
    then
      if builtins.isString value && lib.elem value spec.values
      then renderToolSettingValue value
      else mismatch "one of ${lib.concatStringsSep ", " spec.values}"
    else renderToolSettingValue value;
  toolActivation = name: profile: let
    gameArg = lib.escapeShellArg profile.game;
    configuredTools = lib.filterAttrs (_toolId: toolCfg: toolCfg != null) profile.tools;
    renderToolActivation = toolId: toolCfg: let
      toolArg = lib.escapeShellArg toolId;
      releaseSnippet = lib.optionalString (toolCfg.release != null) (
        let
          assetSrc =
            if toolCfg.release.path != null
            then toolCfg.release.path
            else
              pkgs.fetchurl {
                inherit (toolCfg.release) url hash;
                name = toolCfg.release.asset;
              };
        in ''
          modde tool install-release-from-path ${toolArg} --game ${gameArg} --tag ${lib.escapeShellArg toolCfg.release.tag} --asset ${lib.escapeShellArg toolCfg.release.asset} ${lib.escapeShellArg (toString assetSrc)} || echo "modde: tool install-release-from-path failed for ${toolId}/${name}"
        ''
      );
      renderSettingValue =
        if lib.elem toolId typedToolIds
        then key: value: renderTypedToolSettingValue toolId key value
        else _key: renderToolSettingValue;
      filteredSettings =
        lib.filterAttrs (key: value: key != "_game_id" && key != "optiscaler_profile" && value != null) toolCfg.settings
        // lib.optionalAttrs (toolId == "optiscaler" && toolCfg.profile != null) {
          optiscaler_profile = toolCfg.profile;
        };
      settingsArgs =
        lib.concatStringsSep " "
        (lib.mapAttrsToList (
            key: value: lib.escapeShellArg "${key}=${renderSettingValue key value}"
          )
          filteredSettings);
      configureWarnings = lib.concatStringsSep "\n" (
        lib.optional (builtins.hasAttr "_game_id" toolCfg.settings) ''
          echo "modde: tool configure ignored reserved key _game_id for ${toolId}/${name}"
        ''
        ++ lib.optional (builtins.hasAttr "optiscaler_profile" toolCfg.settings) ''
          echo "modde: tool configure ignored reserved key optiscaler_profile for ${toolId}/${name}"
        ''
      );
      configureCall = lib.optionalString (settingsArgs != "") ''
        modde tool configure ${toolArg} --game ${gameArg} -- ${settingsArgs} || echo "modde: tool configure failed for ${toolId}/${name}"
      '';
      configureSnippet = lib.concatStringsSep "\n" (lib.optional (configureWarnings != "") configureWarnings ++ lib.optional (configureCall != "") configureCall);
    in
      if toolCfg.enable == false
      then ''
        modde tool disable ${toolArg} --game ${gameArg} || echo "modde: tool disable failed for ${toolId}/${name}"
      ''
      else ''
        ${releaseSnippet}
        modde tool enable ${toolArg} --game ${gameArg} || echo "modde: tool enable failed for ${toolId}/${name}"
        ${configureSnippet}
        ${lib.optionalString toolCfg.applyOnActivation ''
          modde tool apply ${toolArg} --game ${gameArg} || echo "modde: tool apply failed for ${toolId}/${name}"
        ''}
      '';
  in
    lib.concatStringsSep "\n" (lib.mapAttrsToList renderToolActivation configuredTools);
  patcherActivation = name: profile: let
    nameArg = lib.escapeShellArg name;
    gameArg = lib.escapeShellArg profile.game;
    renderPatcher = patcherName: patcher: let
      patcherNameArg = lib.escapeShellArg patcherName;
      executable = lib.escapeShellArg (toString patcher.settings.executable);
      outputMod = lib.escapeShellArg patcher.outputMod;
      orderArg = toString patcher.order;
      disableSnippet = lib.optionalString (!patcher.enable) ''
        modde patcher disable ${patcherNameArg} --profile ${nameArg} --game ${gameArg} || echo "modde: patcher disable failed for ${patcherName}/${name}"
      '';
    in
      if patcher.type == "synthesis-cli"
      then ''
        modde patcher add-synthesis ${patcherNameArg} --profile ${nameArg} --game ${gameArg} --executable ${executable} --pipeline-settings ${lib.escapeShellArg (toString patcher.settings.pipelineSettings)} --synthesis-profile ${lib.escapeShellArg patcher.settings.synthesisProfile} --output-mod ${outputMod} --order ${orderArg} || echo "modde: patcher add-synthesis failed for ${patcherName}/${name}"
        ${disableSnippet}
      ''
      else ''
        modde patcher add-command ${patcherNameArg} --profile ${nameArg} --game ${gameArg} --executable ${executable} ${lib.optionalString (patcher.settings.workingDir != null) "--working-dir ${lib.escapeShellArg (toString patcher.settings.workingDir)}"} ${lib.concatStringsSep " " (map (arg: "--arg ${shellArg arg}") patcher.settings.args)} ${lib.concatStringsSep " " (lib.mapAttrsToList (key: value: "--env ${shellArg "${key}=${value}"}") patcher.settings.environment)} --output-mod ${outputMod} --order ${orderArg} || echo "modde: patcher add-command failed for ${patcherName}/${name}"
        ${disableSnippet}
      '';
  in
    lib.concatStringsSep "\n" (lib.mapAttrsToList renderPatcher profile.patchers);
  profileActivation = name: profile: let
    nameArg = lib.escapeShellArg name;
    gameArg = lib.escapeShellArg profile.game;
    gameDirString = lib.optionalString (profile.gameDir != null) (toString profile.gameDir);
    gameDirShell = lib.escapeShellArg gameDirString;
    gameDirArg = lib.optionalString (profile.gameDir != null) " --game-dir ${gameDirShell}";
    deployArgs = "--profile ${nameArg} --game ${gameArg}";
    bethesdaGames = ["skyrim-se" "skyrim-ae" "fallout4" "fallout76" "starfield"];
    requiredModDir =
      if builtins.elem profile.game bethesdaGames
      then "Data"
      else "";
    requiredModDirShell = lib.escapeShellArg requiredModDir;
    awaitingMessage = reason: ''
      echo "modde: profile '${name}' is awaiting game install (${reason})"
      echo "modde: install '${profile.game}' with Steam/Heroic, set programs.modde.profiles.${name}.gameDir, then rebuild Home Manager"
    '';
  in
    if profile.installMode == "disabled"
    then ''
      echo "modde: profile '${name}' is disabled; skipping activation"
    ''
    else if profile.installMode == "await-game"
    then awaitingMessage "installMode = await-game"
    else if profile.wabbajackList != null
    then let
      modlist =
        if profile.wabbajackList.path != null
        then profile.wabbajackList.path
        else
          pkgs.fetchurl {
            inherit (profile.wabbajackList) url hash;
          };
      modlistArg = lib.escapeShellArg (toString modlist);
      manualArchiveImports = lib.concatStringsSep "\n" (
        lib.mapAttrsToList (
          label: archive: let
            resolvedHash =
              if archive.hash != null
              then archive.hash
              else label;
          in
            lib.optionalString (archive.path != null) ''
              echo "modde: importing Wabbajack manual archive ${resolvedHash} (${label}) for '${name}'"
              modde wabbajack import-archive ${modlistArg} ${lib.escapeShellArg (toString archive.path)}
            ''
        )
        profile.wabbajackList.manualArchives
      );
      missingPolicyArg = lib.escapeShellArg profile.wabbajackList.missingArchivePolicy;
    in
      if profile.gameDir == null
      then awaitingMessage "gameDir is not configured"
      else ''
        echo "modde: ensuring Wabbajack profile '${name}' for game '${profile.game}'"
        if [ ! -d ${gameDirShell} ]; then
          ${awaitingMessage "gameDir does not exist"}
        elif [ -n ${requiredModDirShell} ] && [ ! -d ${gameDirShell}/${requiredModDirShell} ]; then
          ${awaitingMessage "gameDir is missing ${requiredModDir}"}
        else
          ${manualArchiveImports}
          if ! modde profile lock-info ${nameArg} --game ${gameArg} >/dev/null 2>&1; then
            modde install wabbajack ${modlistArg} --profile ${nameArg}${gameDirArg} --missing-archive-policy ${missingPolicyArg}
          fi
          ${patcherActivation name profile}
          modde deploy ${deployArgs} || echo "modde: deploy failed for '${name}'"
          ${toolActivation name profile}
        fi
      ''
    else ''
      echo "modde: deploying profile '${name}' for game '${profile.game}'"
      ${patcherActivation name profile}
      modde deploy ${deployArgs} || echo "modde: deploy failed for '${name}'"
      ${toolActivation name profile}
    '';
  db = cfg.database;
  postgresSelected = db.backend == "postgres";
  hasUrl = db.url != null;
  hasDiscrete = db.name != null;
  databaseAssertions = [
    {
      assertion = !postgresSelected || hasUrl || hasDiscrete;
      message = "programs.modde.database: backend = \"postgres\" requires either `url`, or at least `name` (host/port/user optional, default to the local socket).";
    }
    {
      assertion = !postgresSelected || !(hasUrl && hasDiscrete);
      message = "programs.modde.database: set `url` OR the discrete `host`/`port`/`name`/`user` fields, not both.";
    }
    {
      assertion =
        postgresSelected
        || (db.url == null && db.host == null && db.port == null && db.name == null && db.user == null && db.passwordFile == null);
      message = "programs.modde.database: connection fields (url/host/port/name/user/passwordFile) are only valid when backend = \"postgres\".";
    }
  ];
  # The MODDE_DATABASE_* environment the binary reads at startup. modde resolves
  # its backend as env → settings.toml → sqlite default, so these vars make the
  # declarative `database` option authoritative at runtime. Computed once as an
  # attrset and consumed in two places: `home.sessionVariables` (below) so the
  # user's interactive and graphical sessions pick up the backend without any
  # manual `modde config set-database`, and the activation script so deploy-time
  # `modde` invocations hit the same database. The password is never placed in
  # the Nix store — only the path to its file is exported (MODDE_DB_PASSWORD_FILE),
  # and modde reads the secret from that file at runtime.
  databaseEnvVars = lib.optionalAttrs postgresSelected (
    {MODDE_DATABASE_BACKEND = "postgres";}
    // lib.optionalAttrs hasUrl {MODDE_DATABASE_URL = db.url;}
    // lib.optionalAttrs (db.host != null) {MODDE_DATABASE_HOST = db.host;}
    // lib.optionalAttrs (db.port != null) {MODDE_DATABASE_PORT = toString db.port;}
    // lib.optionalAttrs (db.name != null) {MODDE_DATABASE_NAME = db.name;}
    // lib.optionalAttrs (db.user != null) {MODDE_DATABASE_USER = db.user;}
    // lib.optionalAttrs (db.passwordFile != null) {MODDE_DB_PASSWORD_FILE = toString db.passwordFile;}
  );
  # Same vars rendered as shell `export`s for the activation/deploy script.
  databaseEnv =
    lib.concatStringsSep "\n"
    (lib.mapAttrsToList (name: value: "export ${name}=${lib.escapeShellArg value}") databaseEnvVars);
in {
  options.programs.modde = {
    enable = lib.mkEnableOption "modde game mod manager";

    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.stdenv.hostPlatform.system}.modde;
      description = "The modde package to use.";
    };

    profiles = lib.mkOption {
      type = lib.types.attrsOf profileType;
      default = {};
      description = "Mod profiles to manage.";
    };

    nexus = {
      apiKeyFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "Path to file containing the Nexus API key (sops-nix compatible).";
      };
    };

    database = {
      backend = lib.mkOption {
        type = lib.types.enum ["sqlite" "postgres"];
        default = "sqlite";
        description = ''
          Storage backend for modde's profile/mod/tool/save state. "sqlite"
          (the default) uses a local file; "postgres" connects to a PostgreSQL
          server configured via the options below.
        '';
      };

      url = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "postgres://modde@localhost/modde";
        description = ''
          Full PostgreSQL connection URL. Takes precedence over the discrete
          host/port/name/user options. Use this form for connection details the
          discrete fields cannot fully express, including socket directories
          such as `postgres:///modde?host=/run/postgresql` and TLS parameters
          such as `sslmode=require`. Do NOT embed the password here — use
          passwordFile instead so the secret never lands in the Nix store.
        '';
      };

      host = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          PostgreSQL host (used when url is not set). The discrete fields target
          the common TCP/socket-default case; use `url` for a socket directory
          such as `/run/postgresql` or for TLS parameters such as `sslmode`.
        '';
      };

      port = lib.mkOption {
        type = lib.types.nullOr lib.types.port;
        default = null;
        description = "PostgreSQL port (used when url is not set).";
      };

      name = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "PostgreSQL database name (used when url is not set).";
      };

      user = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "PostgreSQL user (used when url is not set).";
      };

      passwordFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = ''
          Path to a file containing the PostgreSQL password (sops-nix
          compatible). Read at modde runtime; never written to settings.toml or
          the Nix store.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = profileAssertions ++ databaseAssertions;

    home.packages = [cfg.package];

    # Make the selected backend authoritative for the user's runtime sessions
    # (terminal launches and graphical-session apps), not just activation. Empty
    # when backend = "sqlite", so it contributes nothing in the default case.
    home.sessionVariables = databaseEnvVars;

    home.activation.modde-deploy = lib.hm.dag.entryAfter ["writeBoundary"] ''
      export PATH="${cfg.package}/bin:$PATH"
      ${lib.optionalString (cfg.nexus.apiKeyFile != null) ''
        export NEXUS_API_KEY_FILE="${cfg.nexus.apiKeyFile}"
      ''}
      ${databaseEnv}
      ${lib.concatStringsSep "\n" (lib.mapAttrsToList profileActivation cfg.profiles)}
    '';
  };
}
