flake: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.modde;
  db = cfg.database;
  postgresSelected = db.backend == "postgres";
  hasUrl = db.url != null;
  hasDiscrete = db.name != null;
  databaseEnvVars = lib.optionalAttrs postgresSelected (
    {MODDE_DATABASE_BACKEND = "postgres";}
    // lib.optionalAttrs hasUrl {MODDE_DATABASE_URL = db.url;}
    // lib.optionalAttrs (db.host != null) {MODDE_DATABASE_HOST = db.host;}
    // lib.optionalAttrs (db.port != null) {MODDE_DATABASE_PORT = toString db.port;}
    // lib.optionalAttrs (db.name != null) {MODDE_DATABASE_NAME = db.name;}
    // lib.optionalAttrs (db.user != null) {MODDE_DATABASE_USER = db.user;}
    // lib.optionalAttrs (db.passwordFile != null) {MODDE_DB_PASSWORD_FILE = toString db.passwordFile;}
  );
  databaseAssertions = [
    {
      assertion = !postgresSelected || hasUrl || hasDiscrete;
      message = "programs.modde.database: postgres requires url or discrete connection fields";
    }
    {
      assertion = !postgresSelected || !(hasUrl && hasDiscrete);
      message = "programs.modde.database: set url OR discrete connection fields, not both";
    }
    {
      assertion = postgresSelected || (db.url == null && db.host == null && db.port == null && db.name == null && db.user == null && db.passwordFile == null);
      message = "programs.modde.database: connection fields require backend = postgres";
    }
  ];
  runtimeEnv =
    databaseEnvVars
    // {
      RUST_LOG = cfg.logging.level;
      MODDE_LOG_KEEP_LAUNCHES = toString cfg.logging.keepLaunches;
    }
    // lib.optionalAttrs (cfg.gpu.renderNode != null) {MODDE_GPU_RENDER_NODE = cfg.gpu.renderNode;}
    // lib.optionalAttrs (cfg.nexus.apiKeyFile != null) {NEXUS_API_KEY_FILE = toString cfg.nexus.apiKeyFile;}
    // lib.optionalAttrs (cfg.manager.package != null) {MODDE_MANAGER_BIN = "${cfg.manager.package}/bin/modde-manager";}
    // lib.optionalAttrs (cfg.manager.configFile != null) {MODDE_MANAGER_CONFIG = toString cfg.manager.configFile;};
  runtimePackage = pkgs.symlinkJoin {
    name = "modde-desktop-runtime";
    paths = [cfg.package];
    nativeBuildInputs = [pkgs.makeWrapper];
    postBuild = ''
      for program in modde modde-ui; do
        rm "$out/bin/$program"
        makeWrapper ${cfg.package}/bin/"$program" "$out/bin/$program" \
          --set MODDE_BIN "$out/bin/modde" \
          ${lib.concatStringsSep " " (lib.mapAttrsToList (key: value: "--set-default ${lib.escapeShellArg key} ${lib.escapeShellArg value}") runtimeEnv)}
      done
      if [ -f "$out/share/applications/com.tartanoglu.modde.desktop" ]; then
        rm "$out/share/applications/com.tartanoglu.modde.desktop"
        substitute ${cfg.package}/share/applications/com.tartanoglu.modde.desktop "$out/share/applications/com.tartanoglu.modde.desktop" \
          --replace-fail 'Exec=modde-ui' "Exec=$out/bin/modde-ui"
      fi
    '';
  };
in {
  options.programs.modde = {
    enable = lib.mkEnableOption "modde game mod manager";
    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.stdenv.hostPlatform.system}.modde;
      description = "The modde package to install.";
    };
    nexus.apiKeyFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to the Nexus API key file.";
    };
    gpu.renderNode = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Stable PCI DRM render node for Mesa game launches. Saved per-installation choices take precedence; this does not select the modde GUI renderer.";
    };
    logging = {
      level = lib.mkOption {
        type = lib.types.enum ["error" "warn" "info" "debug" "trace"];
        default = "info";
        description = "Default tracing level for desktop/CLI entry points; RUST_LOG overrides it.";
      };
      keepLaunches = lib.mkOption {
        type = lib.types.ints.positive;
        default = 50;
        description = "Completed launch bundles to retain; pending sessions and experiments are protected.";
      };
    };
    manager = {
      package = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = null;
        description = "Unwrapped modde-manager package for Library entries.";
      };
      configFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "Declarative manager configuration for Library discovery/launch.";
      };
    };
    database = {
      backend = lib.mkOption {
        type = lib.types.enum ["sqlite" "postgres"];
        default = "sqlite";
      };
      url = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
      };
      host = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
      };
      port = lib.mkOption {
        type = lib.types.nullOr lib.types.port;
        default = null;
      };
      name = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
      };
      user = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
      };
      passwordFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
      };
    };
  };
  config = lib.mkIf cfg.enable {
    assertions =
      databaseAssertions
      ++ [
        {
          assertion = (cfg.manager.package == null) == (cfg.manager.configFile == null);
          message = "programs.modde.manager requires both package and configFile";
        }
        {
          assertion = cfg.gpu.renderNode == null || builtins.match "/dev/dri/by-path/pci-[0-9a-fA-F]{4}:[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\\.[0-7]-render" cfg.gpu.renderNode != null;
          message = "programs.modde.gpu.renderNode must be a stable PCI render-node alias";
        }
      ];
    home.packages = [runtimePackage] ++ lib.optional (cfg.manager.package != null) cfg.manager.package;
    home.sessionVariables = runtimeEnv;
  };
}
