{
  pkgs,
  flake,
}: let
  inherit (pkgs) lib;
  raw = pkgs.runCommand "modde-runtime-probe" {} ''
    mkdir -p "$out/bin" "$out/share/applications"
    for program in modde modde-ui; do
      cat > "$out/bin/$program" <<'SCRIPT'
    #!${pkgs.runtimeShell}
    for key in MODDE_BIN MODDE_DATABASE_BACKEND MODDE_DATABASE_URL MODDE_GPU_RENDER_NODE MODDE_MANAGER_BIN MODDE_MANAGER_CONFIG RUST_LOG MODDE_LOG_KEEP_LAUNCHES; do
      printf '%s=%s\n' "$key" "''${!key}"
    done
    SCRIPT
      chmod +x "$out/bin/$program"
    done
    printf '%s\n' '[Desktop Entry]' 'Exec=modde-ui' > "$out/share/applications/com.tartanoglu.modde.desktop"
  '';
  manager = flake.lib.mkManager {
    inherit pkgs;
    package = pkgs.writeShellScriptBin "modde-manager" "exit 0";
    config = {
      schema_version = 5;
      instances = {};
    };
  };
  evaluated =
    (lib.evalModules {
      specialArgs = {inherit pkgs;};
      modules = [
        ({lib, ...}: {
          options = {
            assertions = lib.mkOption {
              type = lib.types.listOf lib.types.attrs;
              default = [];
            };
            home.packages = lib.mkOption {
              type = lib.types.listOf lib.types.package;
              default = [];
            };
            home.sessionVariables = lib.mkOption {
              type = lib.types.attrsOf lib.types.str;
              default = {};
            };
          };
        })
        (import ./hm-runtime-module.nix flake)
        {
          programs.modde = {
            enable = true;
            package = raw;
            database = {
              backend = "postgres";
              url = "postgres:///modde?host=/run/postgresql";
            };
            gpu.renderNode = "/dev/dri/by-path/pci-0000:03:00.0-render";
            logging = {
              level = "debug";
              keepLaunches = 7;
            };
            manager = {
              package = manager.unwrappedPackage;
              inherit (manager) configFile;
            };
          };
        }
      ];
    }).config;
  runtime = builtins.head evaluated.home.packages;
in
  assert lib.all (a: a.assertion) evaluated.assertions;
    pkgs.runCommand "modde-hm-runtime-check" {nativeBuildInputs = [pkgs.gnugrep];} ''
      set -euo pipefail
      for program in modde modde-ui; do
        env -i ${runtime}/bin/$program > "$program.defaults"
        grep -Fx 'MODDE_BIN=${runtime}/bin/modde' "$program.defaults"
        grep -Fx 'MODDE_DATABASE_BACKEND=postgres' "$program.defaults"
        grep -Fx 'MODDE_DATABASE_URL=postgres:///modde?host=/run/postgresql' "$program.defaults"
        grep -Fx 'MODDE_MANAGER_BIN=${manager.unwrappedPackage}/bin/modde-manager' "$program.defaults"
        grep -Fx 'MODDE_MANAGER_CONFIG=${manager.configFile}' "$program.defaults"
        grep -Fx 'RUST_LOG=debug' "$program.defaults"
        grep -Fx 'MODDE_LOG_KEEP_LAUNCHES=7' "$program.defaults"
        env -i RUST_LOG=trace MODDE_GPU_RENDER_NODE=/explicit MODDE_DATABASE_BACKEND=sqlite ${runtime}/bin/$program > "$program.overrides"
        grep -Fx 'RUST_LOG=trace' "$program.overrides"
        grep -Fx 'MODDE_GPU_RENDER_NODE=/explicit' "$program.overrides"
        grep -Fx 'MODDE_DATABASE_BACKEND=sqlite' "$program.overrides"
      done
      grep -Fx 'Exec=${runtime}/bin/modde-ui' ${runtime}/share/applications/com.tartanoglu.modde.desktop
      cmp ${manager.configFile} ${pkgs.writeText "expected-config.json" (builtins.toJSON {
        schema_version = 5;
        instances = {};
      })}
      touch "$out"
    ''
