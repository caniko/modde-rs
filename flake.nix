{
  description = "modde — cross-platform game mod manager";

  inputs = {
    harbor-rs.url = "git+https://github.com/caniko/harbor-rs.git?ref=trunk&rev=05cc4f162b55fa904b687db1821e2463fa813e50";

    harbor-macos-sdk-pin.url = "git+https://github.com/caniko/harbor-macos-sdk-pin.git";


    nixpkgs.follows = "harbor-rs/nixpkgs";
    rust-overlay.follows = "harbor-rs/rust-overlay";
    crane.follows = "harbor-rs/crane";
    flake-utils.url = "github:numtide/flake-utils";

    nix-appimage = {
      url = "github:ralismark/nix-appimage";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    simit = {
      url = "git+https://github.com/caniko/simit?ref=refs/tags/0.17.10";
      inputs.harbor-rs.follows = "harbor-rs";
      inputs.nixpkgs.follows = "harbor-rs/nixpkgs";
      inputs.rust-overlay.follows = "harbor-rs/rust-overlay";
      inputs.crane.follows = "harbor-rs/crane";
      inputs.flake-utils.url = "github:numtide/flake-utils";
      inputs.plinth.follows = "plinth";
    };

    plinth = {
      url = "git+https://github.com/caniko/plinth.git";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-overlay.follows = "rust-overlay";
      inputs.crane.follows = "crane";
      inputs.flake-utils.follows = "flake-utils";
    };

    visual-rubric = {
      url = "git+https://github.com/caniko/visual-rubric.git";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-overlay.follows = "rust-overlay";
      inputs.crane.follows = "crane";
      inputs.flake-utils.follows = "flake-utils";
    };

    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-manager-core = {
      url = "git+https://github.com/caniko/nix-manager-core.git?ref=trunk";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.harbor-rs.follows = "harbor-rs";
      inputs.rust-overlay.follows = "rust-overlay";
      inputs.crane.follows = "crane";
    };
  };

  outputs = {
    self,
    nixpkgs,
    harbor-rs,
    harbor-macos-sdk-pin,
    simit,
    plinth,
    visual-rubric,
    rust-overlay,
    flake-utils,
    nix-appimage,
    nix-manager-core,
    ...
  }: let
    mkOutputs = {
      macosSdkStorePath ? harbor-macos-sdk-pin.storePath,
      macosSdkOutputHash ? harbor-macos-sdk-pin.outputHash,
      osxSdkVersion ? harbor-macos-sdk-pin.sdkVersion,
    }:
      flake-utils.lib.eachDefaultSystem (system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
          config.allowUnfreePredicate = pkg:
            builtins.elem (nixpkgs.lib.getName pkg) [
              "unrar"
            ];
        };
        lib = nixpkgs.lib;

        toolchain = harbor-rs.lib.mkToolchain {inherit pkgs; toolchainProfile = "nightly";};
        inherit (toolchain) craneLib;
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        moddeVersion = cargoToml.workspace.package.version or cargoToml.package.version;
        simitPackage = simit.packages.${system}.default.overrideAttrs (old: {
          patches = (old.patches or []) ++ [./nix/patches/simit-rs-modde-workflow.patch];
        });
        plinthProject = plinth.packages.${system}.plinth-project;
        visualRubric = visual-rubric.packages.${system}.default;
        simitCli = pkgs.writeShellApplication {
          name = "simit";
          text = ''
            if [ "''${1-}" = "--version" ]; then
              echo "simit ${simitPackage.version}"
              exit 0
            fi

            exec ${lib.getExe' simitPackage "simit"} "$@"
          '';
        };
        coprPython = pkgs.python3Packages.buildPythonPackage rec {
          pname = "copr";
          version = "2.6";
          format = "setuptools";
          src = pkgs.fetchPypi {
            inherit pname version;
            hash = "sha256-w8tEbdLPyqIc01wD/qKETw8aDs0h+dxZ/NnbwcuZuY8=";
          };
          propagatedBuildInputs = with pkgs.python3Packages; [
            filelock
            munch
            requests
            requests-toolbelt
            setuptools
          ];
          pythonImportsCheck = ["copr"];
          doCheck = false;
        };
        coprCli = pkgs.python3Packages.buildPythonApplication rec {
          pname = "copr-cli";
          version = "2.5";
          format = "setuptools";
          src = pkgs.fetchPypi {
            pname = "copr_cli";
            inherit version;
            hash = "sha256-4dKB03SQivnl3RX2l6+v/wcdJVJWKZF5GN52TOrnf3k=";
          };
          propagatedBuildInputs = with pkgs.python3Packages; [
            coprPython
            humanize
            jinja2
            rich
            setuptools
          ];
          doCheck = false;
          meta.mainProgram = "copr-cli";
        };
        cross = harbor-rs.lib.mkCross ({
            inherit pkgs system osxSdkVersion;
          }
          // lib.optionalAttrs (macosSdkStorePath != null) {
            inherit macosSdkStorePath;
            # Pass the FOD outputHash so harbor-rs can reconstruct a
            # context-carrying SDK reference; this makes the pinned SDK a real
            # build input that the sandbox bind-mounts (otherwise osxcross-clang
            # cannot find the SDK inside the sandboxed darwin cross build).
            inherit macosSdkOutputHash;
          });

        # Linux-specific dependencies for the native build
        linuxBuildInputs = lib.optionals pkgs.stdenv.hostPlatform.isLinux (with pkgs; [
          dbus
          wayland
          libxkbcommon
          vulkan-loader
        ]);

        linuxLdPath = lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
          pkgs.lib.makeLibraryPath (with pkgs; [
            wayland
            libxkbcommon
            vulkan-loader
          ])
        );

        # Documentation site built with mdBook (docs/book.toml + docs/src)
        docs = pkgs.stdenv.mkDerivation {
          pname = "modde-docs";
          version = moddeVersion;
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./docs/book.toml
              ./docs/src
            ];
          };
          nativeBuildInputs = [pkgs.mdbook];
          phases = ["buildPhase" "installPhase"];
          buildPhase = ''
            cp -r --no-preserve=mode $src/docs docs
            mdbook build docs
          '';
          installPhase = ''
            cp -r docs/book $out
          '';
        };

        # Presentation website built by the typed Rust/Leptos static generator.
        website = pkgs.stdenv.mkDerivation {
          pname = "modde-website";
          version = moddeVersion;
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./website/static
              ./website/plinth-project.toml
              ./docs/capability-matrix.toml
            ];
          };
          nativeBuildInputs = [plinthProject];
          phases = ["buildPhase" "installPhase"];
          buildPhase = ''
            plinth-project render \
              --config $src/website/plinth-project.toml \
              --out public
          '';
          installPhase = ''
            cp -r public $out
            cp $out/style.css $out/style-20260604.css
          '';
        };

        # Combined site: website at root, docs at /docs/
        site = pkgs.runCommand "modde-project-site" {} ''
          mkdir -p $out
          cp -r ${website}/* $out/
          mkdir -p $out/docs
          cp -r ${docs}/* $out/docs/
          printf '%s\n' modde.tartanoglu.com > $out/.domains
        '';

        nativeBuildInputs = with pkgs; [
          cmake
          makeWrapper
          pkg-config
          # Test-only: a handful of decompress unit tests build 7z
          # fixtures at runtime via Command::new("7zz"). cacert is
          # also referenced explicitly so the closure carries it for
          # the SSL_CERT_FILE that commonArgs sets.
          _7zz
          cacert
        ];

        buildInputs = with pkgs;
          [
            openssl
          ]
          ++ linuxBuildInputs;

        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.lock
            ./Cargo.toml
            ./README.md
            ./crates
            ./crates/modde-manager
            ./docs/capability-matrix.toml
            ./docs/src/reference/parity.md
            ./docs/src/games/supported-games.md
            ./dist/com.tartanoglu.modde.metainfo.xml
            ./dist/com.tartanoglu.modde.png
            ./dist/modde-ui.desktop
            ./dist/assets/logo/logo.svg
            ./website/static
            ./website/plinth-project.toml
          ];
        };

        unrarNgSysWindowsCrossPatch = ./nix/patches/unrar-ng-sys-target-windows-cross.patch;
        cargoVendorDir = craneLib.vendorCargoDeps {
          inherit src;
          overrideVendorCargoPackage = p: drv:
            if p.name == "unrar-ng-sys" && p.version == "0.7.7"
            then
              drv.overrideAttrs (old: {
                patches =
                  (old.patches or [])
                  ++ [unrarNgSysWindowsCrossPatch];
              })
            else drv;
        };

        commonArgs = {
          pname = "modde";
          version = moddeVersion;
          inherit src nativeBuildInputs buildInputs cargoVendorDir;
          strictDeps = true;
          SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
          NIX_SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        };

        nativePackageArgs =
          commonArgs
          // {
            # Switch-time package builds should compile only the shipped
            # binaries. Full target/test coverage lives in checks.
            cargoExtraArgs = "--locked --package modde --package modde-ui --bins";
            doCheck = false;
          };
        managerPackageArgs =
          commonArgs
          // {
            cargoExtraArgs = "--locked --package modde-manager --bin modde-manager";
            doCheck = false;
          };
        oraclePackageArgs =
          commonArgs
          // {
            cargoExtraArgs = "--locked --package modde-oracle --bins";
            doCheck = false;
          };

        cargoArtifacts = craneLib.buildDepsOnly nativePackageArgs;
        managerCargoArtifacts = craneLib.buildDepsOnly managerPackageArgs;
        oracleCargoArtifacts = craneLib.buildDepsOnly oraclePackageArgs;

        modde = craneLib.buildPackage (nativePackageArgs
          // {
            inherit cargoArtifacts;

            postInstall = ''
              for bin in "$out"/bin/*; do
                wrapProgram "$bin" \
                  ${lib.optionalString pkgs.stdenv.hostPlatform.isLinux "--prefix LD_LIBRARY_PATH : ${linuxLdPath} \\"}
                  --set-default SSL_CERT_FILE "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" \
                  --set-default NIX_SSL_CERT_FILE "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              done
              install -Dm0644 ${./dist/modde-ui.desktop} "$out/share/applications/com.tartanoglu.modde.desktop"
              install -Dm0644 ${./dist/com.tartanoglu.modde.png} "$out/share/icons/hicolor/512x512/apps/com.tartanoglu.modde.png"
              install -Dm0644 ${./dist/assets/logo/logo.svg} "$out/share/icons/hicolor/scalable/apps/com.tartanoglu.modde.svg"
              install -Dm0644 ${./dist/com.tartanoglu.modde.metainfo.xml} "$out/share/metainfo/com.tartanoglu.modde.metainfo.xml"
            '';

            meta = with pkgs.lib; {
              description = "Cross-platform game mod manager";
              license = with licenses; [gpl3Only];
              platforms = platforms.linux ++ platforms.darwin;
            };
          });
        modde-manager = craneLib.buildPackage (managerPackageArgs
          // {
            cargoArtifacts = managerCargoArtifacts;
            meta = with pkgs.lib; {
              description = "Declarative post-setup game-client manager";
              license = licenses.gpl3Only;
              platforms = platforms.linux ++ platforms.darwin;
              mainProgram = "modde-manager";
            };
          });
        modde-oracle = craneLib.buildPackage (oraclePackageArgs
          // {
            cargoArtifacts = oracleCargoArtifacts;

            meta = with pkgs.lib; {
              description = "Opt-in empirical mod compatibility oracle service for modde";
              license = with licenses; [gpl3Only];
              platforms = platforms.linux;
            };
          });

        windowsTarget = "x86_64-pc-windows-gnu";
        windowsTargetSuffix =
          lib.strings.replaceStrings ["-"] ["_"] windowsTarget;
        buildPlatformSuffix =
          lib.strings.toLower pkgs.pkgsBuildHost.stdenv.hostPlatform.rust.cargoEnvVarTarget;
        aarch64LinuxTarget = "aarch64-unknown-linux-gnu";
        aarch64LinuxTargetSuffix =
          lib.strings.replaceStrings ["-"] ["_"] aarch64LinuxTarget;
        pkgsAarch64Linux = pkgs.pkgsCross.aarch64-multiplatform;
        toolchainAarch64 = harbor-rs.lib.mkToolchain {pkgs = pkgsAarch64Linux; toolchainProfile = "nightly";};
        craneLibAarch64 = toolchainAarch64.craneLib;
        darwinSigtool = pkgs.darwin.sigtool;
        # Ad-hoc sign the cross-built Mach-O binaries. sigtool's `codesign`
        # spawns `codesign_allocate`, which osxcross provides unprefixed on PATH
        # (osxcross is in the darwin nativeBuildInputs via mkCrossBuilder), so no
        # CODESIGN_ALLOCATE wiring is needed here.
        signDarwinBinaries = ''
          for bin in "$out"/bin/*; do
            if [ -f "$bin" ]; then
              ${lib.getExe' darwinSigtool "codesign"} --sign - --force "$bin"
            fi
          done
        '';
        mkDarwinUnavailable = name:
          pkgs.runCommand name {} ''
            echo "ERROR: ${name} requires osxcross on x86_64-linux with a realized macOS SDK." >&2
            exit 1
          '';
        darwinNativeBuildInputs = nativeBuildInputs ++ [darwinSigtool];
        darwinArgs = pname:
          commonArgs
          // lib.optionalAttrs (cross.osxcrossRustHelpers != null) cross.osxcrossRustHelpers.commonEnv
          // {
            inherit pname;
            buildInputs = [];
            nativeBuildInputs = darwinNativeBuildInputs;
            PKG_CONFIG_ALLOW_CROSS = "1";
            cargoBuildExtraArgs = "--workspace";
            doCheck = false;
            postInstall = signDarwinBinaries;
          };
        darwinCrossBuilderArm =
          if cross.osxcrossRustHelpers != null
          then
            cross.osxcrossRustHelpers.mkCrossBuilder {
              inherit craneLib;
              target = "aarch64-apple-darwin";
            }
          else null;
        darwinArmArgs = darwinArgs "modde-darwin-aarch64";
        aarch64LinuxBuildInputs = with pkgsAarch64Linux; [
          openssl
          dbus
          wayland
          libxkbcommon
          vulkan-loader
        ];
        aarch64LinuxNativeBuildInputs =
          nativeBuildInputs
          ++ [
            pkgsAarch64Linux.stdenv.cc
          ];
        aarch64LinuxArgs =
          commonArgs
          // {
            pname = "modde-aarch64-linux";
            buildInputs = aarch64LinuxBuildInputs;
            nativeBuildInputs = aarch64LinuxNativeBuildInputs;
            CARGO_BUILD_TARGET = aarch64LinuxTarget;
            CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER = "${pkgsAarch64Linux.stdenv.cc}/bin/${pkgsAarch64Linux.stdenv.cc.targetPrefix}cc";
            "CC_${aarch64LinuxTargetSuffix}" = "${pkgsAarch64Linux.stdenv.cc}/bin/${pkgsAarch64Linux.stdenv.cc.targetPrefix}cc";
            "CXX_${aarch64LinuxTargetSuffix}" = "${pkgsAarch64Linux.stdenv.cc}/bin/${pkgsAarch64Linux.stdenv.cc.targetPrefix}c++";
            PKG_CONFIG_ALLOW_CROSS = "1";
            depsBuildBuild = [pkgsAarch64Linux.stdenv.cc];
            cargoBuildExtraArgs = "--workspace";
            doCheck = false;
          };
        windowsBuildInputs = with pkgs.pkgsCross.mingwW64; [
          openssl
          windows.mcfgthreads
          windows.pthreads
        ];
        windowsNativeBuildInputs = with pkgs; [
          cmake
          pkg-config
          pkgsCross.mingwW64.pkg-config
          cross.mingwCC
          cross.mingwBinutils
        ];
        windowsArgs =
          commonArgs
          // cross.windowsEnv
          // {
            pname = "modde-windows";
            buildInputs = windowsBuildInputs;
            nativeBuildInputs = windowsNativeBuildInputs;
            CARGO_BUILD_TARGET = windowsTarget;
            CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = "-C link-arg=-L${pkgs.pkgsCross.mingwW64.windows.mcfgthreads}/lib -C link-arg=-l:libmcfgthread.dll.a";
            PKG_CONFIG_ALLOW_CROSS = "1";
            "CC_${buildPlatformSuffix}" = "cc";
            "CXX_${buildPlatformSuffix}" = "c++";
            preBuild = ''
              mkdir -p .mingw-case-headers
              ln -sf ${pkgs.pkgsCross.mingwW64.windows.mingw_w64_headers}/include/powrprof.h .mingw-case-headers/PowrProf.h
              ln -sf ${pkgs.pkgsCross.mingwW64.windows.mingw_w64_headers}/include/sddl.h .mingw-case-headers/Sddl.h
              ln -sf ${pkgs.pkgsCross.mingwW64.windows.mingw_w64_headers}/include/wbemidl.h .mingw-case-headers/Wbemidl.h
              export CFLAGS_${windowsTargetSuffix}="-I$PWD/.mingw-case-headers ''${CFLAGS_${windowsTargetSuffix}:-}"
              export CXXFLAGS_${windowsTargetSuffix}="-I$PWD/.mingw-case-headers ''${CXXFLAGS_${windowsTargetSuffix}:-}"
            '';
            cargoBuildExtraArgs = "--workspace --features windows-integrations";
            doCheck = false;
          };
        windowsCargoArtifacts = craneLib.buildDepsOnly windowsArgs;
        modde-windows = craneLib.buildPackage (windowsArgs
          // {
            cargoArtifacts = windowsCargoArtifacts;
            postInstall = ''
              cp ${pkgs.pkgsCross.mingwW64.windows.mcfgthreads}/bin/libmcfgthread-2.dll "$out/bin/"
            '';
          });
        aarch64LinuxCargoArtifacts = craneLibAarch64.buildDepsOnly aarch64LinuxArgs;
        modde-aarch64-linux = craneLibAarch64.buildPackage (aarch64LinuxArgs
          // {
            cargoArtifacts = aarch64LinuxCargoArtifacts;
          });
        darwinArmCargoArtifacts =
          if darwinCrossBuilderArm != null
          then darwinCrossBuilderArm.buildDepsOnly darwinArmArgs
          else null;
        modde-darwin-aarch64 =
          if darwinCrossBuilderArm != null
          then
            darwinCrossBuilderArm.buildPackage (darwinArmArgs
              // {
                cargoArtifacts = darwinArmCargoArtifacts;
              })
          else mkDarwinUnavailable "modde-darwin-aarch64";
      in {
        packages =
          {
            inherit modde modde-manager modde-oracle docs website site;
            copr-cli = coprCli;
            default = modde;
            harbor-rs = harbor-rs.packages.${system}.harbor-rs;

            flatpak-cargo-generator = let
              flatpakCargoGeneratorPy = pkgs.fetchurl {
                url = "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/96e2fe8bf7d2e5791ca1bdce2dba373f1e27c425/cargo/flatpak-cargo-generator.py";
                hash = "sha256-s3PIqxoFN47F2O0GRcexJ7zsfS96F5hpT7xifVcNhWw=";
              };
              python = pkgs.python3.withPackages (ps: [
                ps.aiohttp
                ps.pyyaml
                ps.tomlkit
              ]);
            in
              pkgs.writeShellApplication {
                name = "flatpak-cargo-generator";
                runtimeInputs = [python];
                text = ''
                  exec python3 ${flatpakCargoGeneratorPy} "$@"
                '';
              };

            flatpak-manifest = let
              flatpakAppId = "com.tartanoglu.modde";
              releaseSourceUrl = "https://github.com/caniko/rs-modde/releases/download/${moddeVersion}/rs-modde-${moddeVersion}.tar.gz";
              flatpakManifest = {
                "app-id" = flatpakAppId;
                runtime = "org.freedesktop.Platform";
                "runtime-version" = "25.08";
                sdk = "org.freedesktop.Sdk";
                "sdk-extensions" = ["org.freedesktop.Sdk.Extension.rust-stable"];
                command = "modde-ui";
                "finish-args" = [
                  "--socket=wayland"
                  "--socket=fallback-x11"
                  "--share=network"
                  "--filesystem=home"
                  "--device=dri"
                  "--talk-name=org.freedesktop.secrets"
                ];
                modules = [
                  {
                    name = "modde";
                    buildsystem = "simple";
                    "build-options" = {
                      "append-path" = "/usr/lib/sdk/rust-stable/bin";
                      env = {
                        CARGO_HOME = "/run/build/modde/cargo";
                        CARGO_NET_OFFLINE = "true";
                      };
                    };
                    "build-commands" = [
                      "install -Dm0644 cargo/config .cargo/config.toml"
                      "cargo --offline fetch --locked --manifest-path Cargo.toml --verbose"
                      "cargo build --offline --release --locked --bin modde-ui --verbose"
                      "install -Dm0755 target/release/modde-ui \${FLATPAK_DEST}/bin/modde-ui"
                      "install -Dm0644 dist/modde-ui.desktop \${FLATPAK_DEST}/share/applications/\${FLATPAK_ID}.desktop"
                      "install -Dm0644 dist/com.tartanoglu.modde.png \${FLATPAK_DEST}/share/icons/hicolor/512x512/apps/\${FLATPAK_ID}.png"
                      "install -Dm0644 dist/assets/logo/logo.svg \${FLATPAK_DEST}/share/icons/hicolor/scalable/apps/\${FLATPAK_ID}.svg"
                      "install -Dm0644 dist/com.tartanoglu.modde.metainfo.xml \${FLATPAK_DEST}/share/metainfo/\${FLATPAK_ID}.metainfo.xml"
                    ];
                    sources = [
                      {
                        type = "archive";
                        url = releaseSourceUrl;
                        sha256 = "@SOURCE_TARBALL_SHA256@";
                      }
                      "cargo-sources.json"
                    ];
                  }
                ];
              };
            in
              pkgs.writeText "com.tartanoglu.modde.json" (builtins.toJSON flatpakManifest);

            homebrew-formula = let
              versionField = moddeVersion;
              baseUrl = "https://github.com/caniko/rs-modde/releases/download";
              archiveUrl = arch: os: "${baseUrl}/${versionField}/modde-${versionField}-${arch}-${os}.tar.gz";
              formula = harbor-rs.lib.mkHomebrewFormula {
                inherit pkgs;
                name = "modde";
                version = versionField;
                description = "Cross-platform game mod manager";
                homepage = "https://modde.tartanoglu.com";
                license = "GPL-3.0-only";
                platforms = {
                  darwin_arm = {
                    url = archiveUrl "aarch64" "darwin";
                    sha256 = ":no_check";
                  };
                  linux_arm = {
                    url = archiveUrl "aarch64" "linux";
                    sha256 = ":no_check";
                  };
                  linux_intel = {
                    url = archiveUrl "x86_64" "linux";
                    sha256 = ":no_check";
                  };
                };
                binaries = ["modde" "modde-ui"];
                testBlock = ''
                  system "#{bin}/modde", "--version"
                '';
              };
            in
              formula.formulaPath;
          }
          // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            appimage-cli = harbor-rs.lib.mkAppImage {
              inherit system nix-appimage;
              program = "${modde}/bin/modde";
              pname = "modde";
            };
            appimage-ui = harbor-rs.lib.mkAppImage {
              inherit system nix-appimage;
              program = "${modde}/bin/modde-ui";
              pname = "modde-ui";
            };
            inherit modde-aarch64-linux;
            inherit modde-darwin-aarch64;
            inherit modde-windows;
          };

        formatter = pkgs.writeShellApplication {
          name = "modde-format";
          runtimeInputs = [pkgs.alejandra];
          text = ''
            args=("$@")
            has_path=false
            for arg in "''${args[@]}"; do
              case "$arg" in
                -*) ;;
                *) has_path=true ;;
              esac
            done

            if [ "''${has_path}" = false ]; then
              args=(flake.nix nix)
              if [ "$#" -gt 0 ]; then
                args=("$@" "''${args[@]}")
              fi
            fi

            exec alejandra "''${args[@]}"
          '';
        };

        checks = let
          hmLib = lib.extend (_final: _prev: {
            hm.dag.entryAfter = _deps: text: text;
          });
          evalHmConfig = moddeConfig:
            (hmLib.evalModules {
              specialArgs = {
                inherit pkgs;
              };
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
                    home.activation = lib.mkOption {
                      type = lib.types.attrsOf lib.types.str;
                      default = {};
                    };
                    home.sessionVariables = lib.mkOption {
                      type = lib.types.attrsOf lib.types.str;
                      default = {};
                    };
                  };
                })
                (import ./nix/hm-module.nix self)
                {
                  programs.modde =
                    {
                      enable = true;
                      package = pkgs.writeShellScriptBin "modde" "exit 0";
                      profiles = {};
                    }
                    // moddeConfig;
                }
              ];
            })
          .config;
          evalHm = profiles: evalHmConfig {inherit profiles;};
          hmModuleEvalExpr = moddeConfigFile: ''
            let
              pkgs = import "${toString nixpkgs}" {
                system = "x86_64-linux";
                overlays = [ (import "${toString rust-overlay}") ];
              };
              lib = pkgs.lib;
              hmLib = lib.extend (_final: _prev: {
                hm.dag.entryAfter = _deps: text: text;
              });
              evalHmConfig = moddeConfig:
                (hmLib.evalModules {
                  specialArgs = { inherit pkgs; };
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
                        home.activation = lib.mkOption {
                          type = lib.types.attrsOf lib.types.str;
                          default = {};
                        };
                        home.sessionVariables = lib.mkOption {
                          type = lib.types.attrsOf lib.types.str;
                          default = {};
                        };
                      };
                    })
                    (import ./nix/hm-module.nix {
                      packages."x86_64-linux".modde = pkgs.hello;
                    })
                    {
                      programs.modde =
                        {
                          enable = true;
                          package = pkgs.hello;
                          profiles = {};
                        }
                        // moddeConfig;
                    }
                  ];
                }).config;
              evalHm = profiles: evalHmConfig { inherit profiles; };
            in let
              result = evalHmConfig (builtins.fromJSON (builtins.readFile ${moddeConfigFile}));
              failedAssertions = builtins.filter (assertion: !assertion.assertion) result.assertions;
            in
              if failedAssertions != []
              then throw (builtins.head failedAssertions).message
              else
                builtins.deepSeq result.assertions (
                  builtins.deepSeq result.home.sessionVariables (
                    builtins.deepSeq result.home.activation.modde-deploy true
                  )
                )
          '';
          mkHmModuleFailureCheck = {
            name,
            expected,
            profiles ? {},
            database ? {},
          }: let
            moddeConfigFile = pkgs.writeText "modde-hm-module-${name}.json" (builtins.toJSON {inherit profiles database;});
          in
            pkgs.runCommand "modde-hm-module-${name}" {nativeBuildInputs = [pkgs.nix pkgs.gnugrep pkgs.coreutils];} ''
              set -euo pipefail
              export HOME="$TMPDIR/home"
              mkdir -p "$HOME"
              mkdir -p nix
              cp ${./nix/hm-module.nix} nix/hm-module.nix
              cp ${./nix/tool-schema.nix} nix/tool-schema.nix
              cp ${./nix/optiscaler-profiles.nix} nix/optiscaler-profiles.nix
              cp ${./nix/release-supporting-tools.nix} nix/release-supporting-tools.nix
              cat > expr.nix <<'EOF'
              ${hmModuleEvalExpr moddeConfigFile}
              EOF
              if nix-instantiate --eval --show-trace expr.nix >stdout 2>stderr; then
                echo "expected fixture ${name} to fail"
                cat stdout
                cat stderr
                exit 1
              fi
              grep -Fq ${lib.escapeShellArg expected} stderr
              touch "$out"
            '';
          activationReady =
            (evalHm {
              lotf = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                };
              };
              manual = {
                game = "skyrim-se";
              };
            })
          .home
          .activation
          .modde-deploy;
          activationNoGameDir =
            (evalHm {
              lotf = {
                game = "skyrim-se";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationMissingPath =
            (evalHm {
              lotf = {
                game = "skyrim-se";
                gameDir = "/missing/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationAwaitMode =
            (evalHm {
              lotf = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                installMode = "await-game";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationDisabled =
            (evalHm {
              lotf = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                installMode = "disabled";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationManualArchives =
            (evalHm {
              lotf = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  missingArchivePolicy = "omit-mods";
                  manualArchives = {
                    "0123456789abcdef" = {
                      path = "${./LICENSE}";
                    };
                    "Readable Archive Name.7z" = {
                      hash = "abcdef0123456789";
                      path = "${./LICENSE}";
                    };
                    "ffffffffffffffff" = {
                      optional = true;
                    };
                    "Readable Optional Archive.7z" = {
                      hash = "fedcba9876543210";
                      optional = true;
                    };
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationTools =
            (evalHm {
              tooling = {
                game = "test game";
                tools = {
                  mangohud = {
                    enable = true;
                    settings = {
                      enable_vsync = false;
                      fps_limit = 60;
                    };
                  };
                  reshade = {
                    enable = true;
                    applyOnActivation = true;
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationDisabledTool =
            (evalHm {
              tooling = {
                game = "test game";
                tools.vkbasalt.enable = false;
              };
            })
          .home
          .activation
          .modde-deploy;
          activationTypedTools =
            (evalHm {
              tooling = {
                game = "test game";
                tools = {
                  vkbasalt = {
                    enable = true;
                    settings = {
                      enableOnLaunch = true;
                      casSharpness = 0.4;
                    };
                  };
                  reshade = {
                    enable = true;
                    applyOnActivation = true;
                    settings = {
                      dll_name = "d3d11.dll";
                    };
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationOptiscalerRelease =
            (evalHm {
              blade = {
                game = "stellar-blade";
                tools.optiscaler = {
                  enable = true;
                  release = {
                    tag = "v1.0";
                    asset = "OptiScaler.7z";
                    url = "file://${./LICENSE}";
                    hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationOptiscalerProfile =
            (evalHm {
              blade = {
                game = "stellar-blade";
                tools.optiscaler = {
                  enable = true;
                  profile = "community-dxgi";
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationPatchers =
            (evalHm {
              main = {
                game = "skyrim-se";
                patchers = {
                  synthesis = {
                    type = "synthesis-cli";
                    order = 10;
                    outputMod = "generated-synthesis";
                    settings = {
                      executable = "/tools/Synthesis.CLI.exe";
                      pipelineSettings = "/configs/PipelineSettings.json";
                      synthesisProfile = "default";
                    };
                  };
                  command = {
                    type = "command";
                    enable = false;
                    order = 20;
                    outputMod = "generated-command";
                    settings = {
                      executable = "/tools/custom-patcher";
                      args = ["--fast" "--headless"];
                      environment = {
                        MODE = "ci";
                      };
                      workingDir = "/work";
                    };
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          patcherStrictAssertions =
            (evalHm {
              invalid = {
                game = "skyrim-se";
                patchers.bad = {
                  type = "command";
                  order = 1;
                  outputMod = "generated";
                  strict = false;
                  settings.executable = "/tools/custom-patcher";
                };
              };
            })
          .assertions;
          patcherStrictFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && assertion.message
                  == "programs.modde.profiles.invalid.patchers.bad: strict must be true in v1."
              )
              patcherStrictAssertions
            then "false"
            else "true";
          mangohudReleaseAssertions =
            (evalHm {
              invalid = {
                game = "test game";
                tools.mangohud.release = {
                  tag = "v1.0";
                  asset = "MangoHud.7z";
                  url = "https://example.test/mangohud.7z";
                  hash = lib.fakeHash;
                };
              };
            })
          .assertions;
          mangohudReleaseFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && assertion.message
                  == "programs.modde.profiles.invalid.tools.mangohud.release: mangohud does not support release pinning."
              )
              mangohudReleaseAssertions
            then "false"
            else "true";
          releasePathAndUrlAssertions =
            (evalHm {
              invalid = {
                game = "test game";
                tools.optiscaler.release = {
                  tag = "v1.0";
                  asset = "OptiScaler.7z";
                  url = "https://example.test/o.7z";
                  path = "${./LICENSE}";
                };
              };
            })
          .assertions;
          releasePathAndUrlFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && lib.hasInfix "mutually exclusive" assertion.message
              )
              releasePathAndUrlAssertions
            then "false"
            else "true";
          optiscalerBadProfileEval = builtins.tryEval (
            builtins.deepSeq
            ((evalHm {
                invalid = {
                  game = "stellar-blade";
                  tools.optiscaler = {
                    enable = true;
                    profile = "nonexistent";
                  };
                };
              })
                .home
                .activation
                .modde-deploy)
            true
          );
          typedUnknownKeyEval = builtins.tryEval (
            builtins.deepSeq
            ((evalHm {
                invalid = {
                  game = "test game";
                  tools.vkbasalt = {
                    enable = true;
                    settings.cas_sharpness = 0.4;
                  };
                };
              })
                .home
                .activation
                .modde-deploy)
            true
          );
          typedWrongTypeEval = builtins.tryEval (
            builtins.deepSeq
            ((evalHm {
                invalid = {
                  game = "test game";
                  tools.vkbasalt = {
                    enable = true;
                    settings.casSharpness = "fast";
                  };
                };
              })
                .home
                .activation
                .modde-deploy)
            true
          );
          badAssertions =
            (evalHm {
              invalid = {
                game = "skyrim-se";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                };
                nexusCollection = {
                  slug = "collection";
                  version = "1";
                };
              };
            })
          .assertions;
          mutualExclusionFails =
            if (builtins.elemAt badAssertions 0).assertion
            then "true"
            else "false";
          unknownToolEval = builtins.tryEval (
            builtins.deepSeq
            ((evalHm {
                invalid = {
                  game = "skyrim-se";
                  tools.notatool.enable = true;
                };
              })
                .home
                .activation
                .modde-deploy)
            true
          );
          readableManualWithoutHashAssertions =
            (evalHm {
              invalid = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  manualArchives."Readable Archive.7z" = {
                    path = "${./LICENSE}";
                  };
                };
              };
            })
          .assertions;
          readableManualWithoutHashFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && assertion.message
                  == "programs.modde.profiles.invalid: readable manualArchives entries must set hash."
              )
              readableManualWithoutHashAssertions
            then "false"
            else "true";
          duplicateManualHashAssertions =
            (evalHm {
              invalid = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  manualArchives = {
                    "0123456789abcdef".path = "${./LICENSE}";
                    "Readable Archive.7z" = {
                      hash = "0123456789abcdef";
                      path = "${./LICENSE}";
                    };
                  };
                };
              };
            })
          .assertions;
          duplicateManualHashFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && assertion.message
                  == "programs.modde.profiles.invalid: manualArchives entries resolve to duplicate hashes."
              )
              duplicateManualHashAssertions
            then "false"
            else "true";
          databaseUrlOnly = evalHmConfig {
            database = {
              backend = "postgres";
              url = "postgres:///x";
            };
          };
          databaseDiscrete = evalHmConfig {
            database = {
              backend = "postgres";
              name = "modde";
              host = "h";
              port = 5432;
              user = "u";
            };
          };
          databaseNameOnly = evalHmConfig {
            database = {
              backend = "postgres";
              name = "modde";
            };
          };
          databaseSqliteDefault = evalHmConfig {};
        in {
          tool-schema-fresh = pkgs.runCommand "modde-tool-schema-fresh" {} ''
            ${modde}/bin/modde dev export-tool-schema --out "$TMPDIR/tool-schema.nix"
            diff -u ${./nix/tool-schema.nix} "$TMPDIR/tool-schema.nix"
            diff -u ${./nix/optiscaler-profiles.nix} "$TMPDIR/optiscaler-profiles.nix"
            diff -u ${./nix/release-supporting-tools.nix} "$TMPDIR/release-supporting-tools.nix"
            touch "$out"
          '';
          hm-module = pkgs.runCommand "modde-hm-module-check" {} ''
            cat > ready <<'EOF'
            ${activationReady}
            EOF
            grep -q "modde install wabbajack" ready
            grep -q -- "--game-dir '/games/Skyrim Special Edition'" ready
            grep -q "modde deploy --profile lotf --game skyrim-se" ready
            grep -q "modde deploy --profile manual --game skyrim-se" ready

            cat > no-game-dir <<'EOF'
            ${activationNoGameDir}
            EOF
            grep -q "awaiting game install" no-game-dir
            grep -q "gameDir is not configured" no-game-dir
            ! grep -q "modde install wabbajack" no-game-dir

            cat > missing-path <<'EOF'
            ${activationMissingPath}
            EOF
            grep -q "gameDir does not exist" missing-path
            grep -q "modde install wabbajack" missing-path
            grep -q "if \\[ ! -d '/missing/Skyrim Special Edition' \\]" missing-path

            cat > await-mode <<'EOF'
            ${activationAwaitMode}
            EOF
            grep -q "installMode = await-game" await-mode
            ! grep -q "modde install wabbajack" await-mode
            ! grep -q "modde deploy --profile lotf" await-mode

            cat > disabled <<'EOF'
            ${activationDisabled}
            EOF
            grep -q "is disabled; skipping activation" disabled
            ! grep -q "modde install wabbajack" disabled
            ! grep -q "modde deploy --profile lotf" disabled

            cat > manual-archives <<'EOF'
            ${activationManualArchives}
            EOF
            grep -q "modde wabbajack import-archive" manual-archives
            grep -q "0123456789abcdef (0123456789abcdef)" manual-archives
            grep -q "abcdef0123456789 (Readable Archive Name.7z)" manual-archives
            grep -q -- "--missing-archive-policy omit-mods" manual-archives

            cat > tools <<'EOF'
            ${activationTools}
            EOF
            grep -q "modde deploy --profile tooling --game 'test game'" tools
            grep -q "modde tool enable mangohud --game 'test game'" tools
            grep -q "modde tool configure mangohud --game 'test game' --" tools
            grep -q "enable_vsync=false" tools
            grep -q "fps_limit=60" tools
            ! grep -q "modde tool apply mangohud --game 'test game'" tools
            grep -q "modde tool apply reshade --game 'test game'" tools

            cat > disabled-tool <<'EOF'
            ${activationDisabledTool}
            EOF
            grep -q "modde tool disable vkbasalt --game 'test game'" disabled-tool
            ! grep -q "modde tool configure vkbasalt" disabled-tool
            ! grep -q "modde tool apply vkbasalt" disabled-tool

            cat > typed-tools <<'EOF'
            ${activationTypedTools}
            EOF
            grep -q "modde tool enable vkbasalt --game 'test game'" typed-tools
            grep -q "enableOnLaunch=true" typed-tools
            grep -q "casSharpness=0.4" typed-tools
            ! grep -q "toggleKey=" typed-tools
            grep -q "modde tool configure reshade --game 'test game' -- 'dll_name=d3d11.dll'" typed-tools
            grep -q "modde tool apply reshade --game 'test game'" typed-tools

            cat > optiscaler-release <<'EOF'
            ${activationOptiscalerRelease}
            EOF
            grep -q "modde tool install-release-from-path optiscaler --game stellar-blade --tag v1.0 --asset OptiScaler.7z" optiscaler-release
            grep -q "/nix/store/" optiscaler-release
            grep -q "modde tool enable optiscaler --game stellar-blade" optiscaler-release

            cat > optiscaler-profile <<'EOF'
            ${activationOptiscalerProfile}
            EOF
            grep -q "modde tool configure optiscaler --game stellar-blade -- 'optiscaler_profile=community-dxgi'" optiscaler-profile

            cat > patchers <<'EOF'
            ${activationPatchers}
            EOF
            grep -q "modde patcher add-synthesis synthesis --profile main --game skyrim-se" patchers
            grep -q -- "--pipeline-settings /configs/PipelineSettings.json" patchers
            grep -q -- "--synthesis-profile default" patchers
            grep -q -- "--order 10" patchers
            grep -q "modde patcher add-command command --profile main --game skyrim-se" patchers
            grep -q -- "--working-dir /work" patchers
            grep -q -- "--arg --fast" patchers
            grep -q -- "--env MODE=ci" patchers
            grep -q "modde patcher disable command --profile main --game skyrim-se" patchers

            test "${mutualExclusionFails}" = "false"
            test "${
              if unknownToolEval.success
              then "true"
              else "false"
            }" = "false"
            test "${readableManualWithoutHashFails}" = "false"
            test "${duplicateManualHashFails}" = "false"
            test "${patcherStrictFails}" = "false"
            test "${mangohudReleaseFails}" = "false"
            test "${releasePathAndUrlFails}" = "false"
            test "${
              if optiscalerBadProfileEval.success
              then "true"
              else "false"
            }" = "false"
            test "${
              if typedUnknownKeyEval.success
              then "true"
              else "false"
            }" = "false"
            test "${
              if typedWrongTypeEval.success
              then "true"
              else "false"
            }" = "false"
            touch "$out"
          '';
          hm-module-database = pkgs.runCommand "modde-hm-module-database-check" {} ''
            cat > url-session.json <<'EOF'
            ${builtins.toJSON databaseUrlOnly.home.sessionVariables}
            EOF
            grep -q '"MODDE_DATABASE_BACKEND":"postgres"' url-session.json
            grep -q '"MODDE_DATABASE_URL":"postgres:///x"' url-session.json
            ! grep -q 'MODDE_DATABASE_NAME' url-session.json
            ! grep -q 'MODDE_DATABASE_HOST' url-session.json
            ! grep -q 'MODDE_DATABASE_PORT' url-session.json
            ! grep -q 'MODDE_DATABASE_USER' url-session.json

            cat > url-activation <<'EOF'
            ${databaseUrlOnly.home.activation.modde-deploy}
            EOF
            grep -q "export MODDE_DATABASE_BACKEND=postgres" url-activation
            grep -q "export MODDE_DATABASE_URL=postgres:///x" url-activation
            ! grep -q 'MODDE_DATABASE_NAME' url-activation
            ! grep -q 'MODDE_DATABASE_HOST' url-activation
            ! grep -q 'MODDE_DATABASE_PORT' url-activation
            ! grep -q 'MODDE_DATABASE_USER' url-activation

            cat > discrete-session.json <<'EOF'
            ${builtins.toJSON databaseDiscrete.home.sessionVariables}
            EOF
            grep -q '"MODDE_DATABASE_BACKEND":"postgres"' discrete-session.json
            grep -q '"MODDE_DATABASE_NAME":"modde"' discrete-session.json
            grep -q '"MODDE_DATABASE_HOST":"h"' discrete-session.json
            grep -q '"MODDE_DATABASE_PORT":"5432"' discrete-session.json
            grep -q '"MODDE_DATABASE_USER":"u"' discrete-session.json

            cat > discrete-activation <<'EOF'
            ${databaseDiscrete.home.activation.modde-deploy}
            EOF
            grep -q "export MODDE_DATABASE_BACKEND=postgres" discrete-activation
            grep -q "export MODDE_DATABASE_NAME=modde" discrete-activation
            grep -q "export MODDE_DATABASE_HOST=h" discrete-activation
            grep -q "export MODDE_DATABASE_PORT=5432" discrete-activation
            grep -q "export MODDE_DATABASE_USER=u" discrete-activation

            cat > name-only-session.json <<'EOF'
            ${builtins.toJSON databaseNameOnly.home.sessionVariables}
            EOF
            grep -q '"MODDE_DATABASE_BACKEND":"postgres"' name-only-session.json
            grep -q '"MODDE_DATABASE_NAME":"modde"' name-only-session.json
            ! grep -q 'MODDE_DATABASE_HOST' name-only-session.json
            ! grep -q 'MODDE_DATABASE_PORT' name-only-session.json
            ! grep -q 'MODDE_DATABASE_USER' name-only-session.json

            cat > sqlite-session.json <<'EOF'
            ${builtins.toJSON databaseSqliteDefault.home.sessionVariables}
            EOF
            test "$(cat sqlite-session.json)" = "{}"

            cat > sqlite-activation <<'EOF'
            ${databaseSqliteDefault.home.activation.modde-deploy}
            EOF
            ! grep -q 'MODDE_DATABASE_' sqlite-activation
            touch "$out"
          '';
          hm-module-database-postgres-missing-name = mkHmModuleFailureCheck {
            name = "database-postgres-missing-name";
            expected = "backend = \"postgres\" requires either `url`, or at least `name`";
            database.backend = "postgres";
          };
          hm-module-database-url-and-discrete = mkHmModuleFailureCheck {
            name = "database-url-and-discrete";
            expected = "set `url` OR the discrete `host`/`port`/`name`/`user` fields, not both";
            database = {
              backend = "postgres";
              url = "postgres:///x";
              name = "modde";
              host = "h";
              port = 5432;
              user = "u";
            };
          };
          hm-module-database-sqlite-connection-field = mkHmModuleFailureCheck {
            name = "database-sqlite-connection-field";
            expected = "connection fields (url/host/port/name/user/passwordFile) are only valid when backend = \"postgres\"";
            database = {
              backend = "sqlite";
              name = "modde";
            };
          };
          hm-module-tools = pkgs.runCommand "modde-hm-module-tools-check" {} ''
            cat > tools <<'EOF'
            ${(evalHm {
                skyrim = {
                  game = "skyrim-se";
                  gameDir = "/games/Skyrim Special Edition";
                  wabbajackList = {
                    url = "file://${./LICENSE}";
                    hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  };
                  tools = {
                    vkbasalt = {
                      enable = true;
                      settings = {
                        enableOnLaunch = true;
                        casSharpness = 0.4;
                      };
                    };
                    gamemode.enable = true;
                  };
                };
                blade = {
                  game = "stellar-blade";
                  tools.optiscaler = {
                    enable = true;
                    applyOnActivation = true;
                    profile = "community-dxgi";
                    release = {
                      tag = "v1.0";
                      asset = "OptiScaler.7z";
                      path = "${pkgs.writeText "OptiScaler.7z" "fake OptiScaler release archive"}";
                    };
                  };
                };
              })
            .home
            .activation
            .modde-deploy}
            EOF
            grep -q "modde deploy --profile skyrim --game skyrim-se" tools
            grep -q "modde tool enable vkbasalt --game skyrim-se" tools
            grep -q "modde tool configure vkbasalt --game skyrim-se --" tools
            grep -q "enableOnLaunch=true" tools
            grep -q "casSharpness=0.4" tools
            grep -q "modde tool enable gamemode --game skyrim-se" tools
            grep -q "modde tool install-release-from-path optiscaler --game stellar-blade --tag v1.0 --asset OptiScaler.7z" tools
            grep -q "/nix/store/" tools
            grep -q "modde tool configure optiscaler --game stellar-blade --" tools
            grep -q "optiscaler_profile=community-dxgi" tools
            grep -q "modde tool apply optiscaler --game stellar-blade" tools
            touch "$out"
          '';
          hm-module-tools-unknown-tool = mkHmModuleFailureCheck {
            name = "unknown-tool";
            expected = "does not exist";
            profiles = {
              invalid = {
                game = "skyrim-se";
                tools.notatool.enable = true;
              };
            };
          };
          hm-module-tools-unknown-setting = mkHmModuleFailureCheck {
            name = "unknown-setting";
            expected = "does not exist";
            profiles = {
              invalid = {
                game = "test game";
                tools.vkbasalt = {
                  enable = true;
                  settings.cas_sharpness = true;
                };
              };
            };
          };
          hm-module-tools-wrong-type = mkHmModuleFailureCheck {
            name = "wrong-type";
            expected = "not of type";
            profiles = {
              invalid = {
                game = "test game";
                tools.vkbasalt = {
                  enable = true;
                  settings.casSharpness = "fast";
                };
              };
            };
          };
          hm-module-tools-unsupported-release = pkgs.runCommand "modde-hm-module-unsupported-release" {} ''
            cat > assertions.json <<'EOF'
            ${builtins.toJSON
              (evalHm {
                invalid = {
                  game = "test game";
                  tools.mangohud.release = {
                    tag = "v1.0";
                    asset = "MangoHud.7z";
                    path = "${pkgs.writeText "MangoHud.7z" "fake MangoHud release archive"}";
                  };
                };
              })
                .assertions}
            EOF
            grep -q "does not support release pinning" assertions.json
            touch "$out"
          '';
          manager = craneLib.cargoTest (managerPackageArgs
            // {
              cargoArtifacts = managerCargoArtifacts;
              doCheck = true;
              nativeBuildInputs =
                managerPackageArgs.nativeBuildInputs ++ [pkgs.procps pkgs.git pkgs.sqlite];
            });
          hm-module-tools-bad-profile = mkHmModuleFailureCheck {
            name = "bad-profile";
            expected = "singular enum";
            profiles = {
              invalid = {
                game = "stellar-blade";
                tools.optiscaler = {
                  enable = true;
                  profile = "nonexistent";
                };
              };
            };
          };
        };

        devShells =
          (harbor-rs.lib.mkDevShells {
            inherit pkgs cross;
            inherit (toolchain) craneLib;
            pkgConfigDeps = buildInputs;

            packages = with pkgs;
              [
                appstream
                cargo-about
                cargo-audit
                cargo-cyclonedx
                cargo-deb
                cargo-deny
                cargo-llvm-cov
                cargo-nextest
                cargo-sbom
                coprCli
                cosign
                curl
                debootstrap
                dnf5
                dpkg
                file
                findutils
                flatpak
                flatpak-builder
                forgejo-cli
                git
                gnugrep
                gnupg
                gnutar
                grype
                gzip
                jq
                minisign
                nodejs
                openssh
                osslsigncode
                pacman
                podman
                pre-commit
                qemu
                reprepro
                rpm
                rust-analyzer
                stdenv.cc
                toolchain.rustToolchain
                simitCli
                plinthProject
                visualRubric
                alejandra
                just
                _7zz
                unrar
                mdbook
                prettier
                taplo
                unzip
                util-linux
                wineWow64Packages.stable
                wget
                zip
              ]
              ++ nativeBuildInputs
              ++ buildInputs;

            extraEnv = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
              LD_LIBRARY_PATH = linuxLdPath;
            };
          })
          // {
            docs = pkgs.mkShell {
              packages = with pkgs;
                [
                  cargo-deny
                  cargo-nextest
                  git
                  mdbook
                  plinthProject
                  pre-commit
                  rust-analyzer
                  stdenv.cc
                  toolchain.rustToolchain
                  visualRubric
                  jq
                  taplo
                ]
                ++ nativeBuildInputs
                ++ buildInputs;
              shellHook = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                export LD_LIBRARY_PATH="${linuxLdPath}"
              '';
            };
          };

        apps.deploy-pages = {
          type = "app";
          program = let
            script = pkgs.writeShellApplication {
              name = "deploy-pages";
              runtimeInputs = with pkgs; [git nix coreutils findutils];
              text = builtins.readFile ./scripts/deploy-pages.sh;
            };
          in "${script}/bin/deploy-pages";
        };

        apps.release-smoke = {
          type = "app";
          program = let
            script = pkgs.writeShellApplication {
              name = "release-smoke";
              runtimeInputs = with pkgs; [
                appstream
                coreutils
                cosign
                debootstrap
                dnf5
                dpkg
                file
                findutils
                flatpak
                flatpak-builder
                git
                gnugrep
                gnutar
                grype
                gzip
                jq
                minisign
                osslsigncode
                podman
                qemu
                rpm
                unzip
                wineWow64Packages.stable
              ];
              text = ''
                repo="''${MODDE_SOURCE_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
                exec "$repo/scripts/smoke/run-smoke.sh" "$@"
              '';
            };
          in "${script}/bin/release-smoke";
        };

        apps.copr-cli = {
          type = "app";
          program = "${lib.getExe coprCli}";
        };

        apps.local-check-fast = {
          type = "app";
          program = let
            script = pkgs.writeShellApplication {
              name = "local-check-fast";
              runtimeInputs = with pkgs; [
                cargo-deny
                coreutils
                git
                gnugrep
                jq
                nix
                simitCli
                toolchain.rustToolchain
              ];
              text = ''
                repo="''${MODDE_SOURCE_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
                cd "$repo"
                if [ -z "''${MODDE_LOCAL_CHECK_IN_DEVSHELL:-}" ]; then
                  exec nix develop "$repo" -c env MODDE_LOCAL_CHECK_IN_DEVSHELL=1 "$0" "$@"
                fi

                simit init release --check
                simit init ci --ci-provider crow --platform forgejo --runtime nix --runner codefloe-global --workspace --check
                nix flake check --keep-going
                cargo test --workspace --all-features
                cargo clippy --workspace --all-targets --all-features -- --deny warnings
                cargo deny check -W unmaintained advisories bans sources licenses

                for package in modde-core modde-sources modde-games modde-ui modde; do
                  cargo package -p "$package" --allow-dirty --list >/dev/null
                done
              '';
            };
          in "${script}/bin/local-check-fast";
        };

        apps.release-local-check = {
          type = "app";
          program = let
            script = pkgs.writeShellApplication {
              name = "release-local-check";
              runtimeInputs = with pkgs; [
                appstream
                cargo-about
                cargo-cyclonedx
                cargo-deb
                cargo-deny
                cargo-sbom
                coprCli
                coreutils
                cosign
                curl
                debootstrap
                dnf5
                dpkg
                file
                findutils
                forgejo-cli
                git
                gnugrep
                gnupg
                gnutar
                gzip
                jq
                minisign
                toolchain.rustToolchain
                nix
                nodejs
                openssh
                reprepro
                rpm
                util-linux
              ];
              text = ''
                repo="''${MODDE_SOURCE_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
                exec bash "$repo/scripts/release-local-check.sh" "$@"
              '';
            };
          in "${script}/bin/release-local-check";
        };

        apps.local-check-release = {
          type = "app";
          program = let
            script = pkgs.writeShellApplication {
              name = "local-check-release";
              runtimeInputs = with pkgs; [
                coreutils
                git
              ];
              text = ''
                version="''${1:-}"
                if [ -z "$version" ]; then
                  echo "usage: local-check-release <version>" >&2
                  exit 2
                fi

                ${self.apps.${system}.local-check-fast.program}
                exec ${self.apps.${system}.release-local-check.program} "$version"
              '';
            };
          in "${script}/bin/local-check-release";
        };

        apps.build-deb = {
          type = "app";
          program = let
            script = pkgs.writeShellApplication {
              name = "build-deb";
              runtimeInputs = with pkgs; [
                cargo-deb
                coreutils
                dpkg
                git
                gnugrep
                nix
                toolchain.rustToolchain
              ];
              text = ''
                repo="''${MODDE_SOURCE_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
                cd "$repo"
                if [ -z "''${MODDE_BUILD_DEB_IN_DEVSHELL:-}" ]; then
                  exec nix develop "$repo" -c env MODDE_BUILD_DEB_IN_DEVSHELL=1 "$0" "$@"
                fi

                exec bash "$repo/scripts/build-deb.sh" "$@"
              '';
            };
          in "${script}/bin/build-deb";
        };

        apps.local-release-deploy = {
          type = "app";
          program = let
            script = pkgs.writeShellApplication {
              name = "local-release-deploy";
              runtimeInputs = with pkgs; [
                appstream
                cargo-about
                cargo-cyclonedx
                cargo-deb
                cargo-deny
                cargo-sbom
                coprCli
                coreutils
                cosign
                curl
                debootstrap
                dnf5
                dpkg
                file
                findutils
                flatpak
                flatpak-builder
                forgejo-cli
                git
                gnugrep
                gnupg
                gnutar
                gzip
                jq
                minisign
                nix
                nodejs
                openssh
                osslsigncode
                pacman
                qemu
                reprepro
                rpm
                toolchain.rustToolchain
                unzip
                util-linux
                wineWow64Packages.stable
                wget
                zip
              ];
              text = ''
                repo="''${MODDE_SOURCE_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
                cd "$repo"
                if [ -z "''${MODDE_LOCAL_DEPLOY_IN_DEVSHELL:-}" ]; then
                  exec nix develop "$repo" -c env MODDE_LOCAL_DEPLOY_IN_DEVSHELL=1 "$0" "$@"
                fi

                exec bash "$repo/scripts/local-release-deploy.sh" "$@"
              '';
            };
          in "${script}/bin/local-release-deploy";
        };

        # Release artifact signing/verification via the harbor-rs binding.
        # `minisign -S/-V` over target/modde-release/root-artifacts/release/SHA256SUMS.txt against keys/minisign.pub —
        # the same operation scripts/smoke/smoke-signatures.sh and the
        # simit-generated release.yml perform, exposed as reusable apps.
        #   MINISIGN_SECRET_KEY=… MINISIGN_PASSWORD=… nix run .#sign-release
        #   nix run .#verify-release
        apps.sign-release = harbor-rs.lib.mkMinisignSign {
          inherit pkgs;
          files = ["target/modde-release/root-artifacts/release/SHA256SUMS.txt"];
        };

        apps.verify-release = harbor-rs.lib.mkMinisignVerify {
          inherit pkgs;
          files = ["target/modde-release/root-artifacts/release/SHA256SUMS.txt"];
          publicKeyFile = "keys/minisign.pub";
        };
      });
  in
    {
      lib.managerSchemaVersion = 3;
      lib.mkManager = {
        pkgs,
        package,
        config,
      }:
        nix-manager-core.lib.mkDeclarativeManager {
          inherit pkgs config package;
          managerPackage = package;
          managerBinary = "modde-manager";
          extraRuntimePackages = [pkgs.git pkgs.procps pkgs.sqlite];
        };

      linuxDistributionSupport = {
        policy = "major-distro-families";
        cargo_features = {
          default = ["rar" "postgres"];
          lean_linux = [];
        };
        channels = {
          apt = {
            enabled = true;
            families = ["debian" "ubuntu" "linux-mint" "pop-os"];
            architectures = ["amd64"];
            artifacts = ["*.deb"];
            smoke = "smoke-deb";
            publish_gate = "stable-tags-only";
          };
          copr = {
            enabled = true;
            families = ["fedora" "rhel" "rocky" "alma" "bazzite" "nobara"];
            architectures = ["x86_64"];
            artifacts = ["*.src.rpm"];
            smoke = "smoke-srpm";
            publish_gate = "stable-tags-only";
          };
          aur = {
            enabled = true;
            families = ["arch" "manjaro" "endeavouros" "cachyos"];
            architectures = ["x86_64"];
            artifacts = ["modde-{version}-x86_64-linux.tar.gz" "rs-modde-{version}.tar.gz"];
            publish_gate = "stable-tags-only";
          };
          nix = {
            enabled = true;
            families = ["nix" "nixos"];
            architectures = ["x86_64-linux" "aarch64-linux"];
            artifacts = ["nix flake package" "home-manager module"];
            smoke = "nix flake check";
            publish_gate = "flake-evaluation";
          };
          flatpak = {
            enabled = true;
            families = ["freedesktop" "immutable-linux" "gaming-linux"];
            architectures = ["x86_64"];
            artifacts = ["com.tartanoglu.modde.json" "cargo-sources.json"];
            smoke = "smoke-flatpak";
            publish_gate = "stable-tags-only";
          };
          appimage = {
            enabled = true;
            families = ["portable-linux"];
            architectures = ["x86_64"];
            artifacts = ["modde-{version}-x86_64.AppImage" "modde-ui-{version}-x86_64.AppImage"];
            smoke = "smoke-appimage";
            publish_gate = "release-artifact";
          };
          tarball = {
            enabled = true;
            families = ["generic-linux"];
            architectures = ["x86_64-linux" "aarch64-linux"];
            artifacts = ["modde-{version}-x86_64-linux.tar.gz" "modde-{version}-aarch64-linux.tar.gz"];
            smoke = "smoke-linux-tarball";
            publish_gate = "release-artifact";
          };
        };
        out_of_scope = ["opensuse-obs" "snap" "alpine-musl"];
      };
      homeManagerModules.modde = import ./nix/hm-runtime-module.nix self;
      lib = {
        inherit mkOutputs;
      };
      simitConfig = {
        ci.check_command = "cargo run -p modde-xtask -- check";
        release.publish.enforcement = "activated-remote";
        release.publish.channels = {
          apt = "required";
          scoop = "required";
          homebrew = "staged";
        };
        release.smoke.command = "nix run .#release-smoke --";
        release.codeberg = {
          repo = "caniko/rs-modde";
          target_branch = "trunk";
          token_secret = "CODEBERG_TOKEN";
        };
        release.artifacts = {
          version_attr = "modde";
          substituters = ["https://attic.candee.baby/canix" "https://cache.nixos.org"];
          trusted_public_keys = ["canix:lPzPzKrmYqW5Rxa5r0uQWvCqD3S5nx0h2eCy7XD5JM8=" "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="];
          supply_chain_command = "nix shell nixpkgs#cargo nixpkgs#rustc nixpkgs#cargo-deny -c cargo deny check -W unmaintained advisories bans sources licenses";
          checksum_globs = ["*.tar.gz" "*.zip" "*.AppImage" "*.deb" "*.src.rpm" "*.cdx.json" "*.spdx.json"];
          sbom_commands = [
            ''
              nix develop -c bash <<'SBOM'
              set -euo pipefail
              mkdir -p target/modde-release/root-artifacts/release
              cargo about generate --config dist/licenses/about.toml --output-file target/modde-release/root-artifacts/release/THIRD_PARTY_LICENSES.html dist/licenses/about-template.hbs
              cargo sbom --output-format cyclone_dx_json_1_5 > "target/modde-release/root-artifacts/release/modde-''${VERSION}.cdx.json"
              cargo sbom --output-format spdx_json_2_3 > "target/modde-release/root-artifacts/release/modde-''${VERSION}.spdx.json"
              SBOM
            ''
          ];
          build_commands = [
            ''
              copy_nix_binary() {
                result_dir="$1"; binary="$2"; destination="$3"
                if [ -f "''${result_dir}/bin/.''${binary}-wrapped" ]; then cp "''${result_dir}/bin/.''${binary}-wrapped" "$destination"; else cp "''${result_dir}/bin/''${binary}" "$destination"; fi
              }

              nix build .#modde --out-link target/modde-release/root-artifacts/linux-result
              mkdir -p target/modde-release/root-artifacts/release/linux-x86_64
              copy_nix_binary target/modde-release/root-artifacts/linux-result modde target/modde-release/root-artifacts/release/linux-x86_64/modde
              copy_nix_binary target/modde-release/root-artifacts/linux-result modde-ui target/modde-release/root-artifacts/release/linux-x86_64/modde-ui
              tar czf "target/modde-release/root-artifacts/release/modde-''${VERSION}-x86_64-linux.tar.gz" -C target/modde-release/root-artifacts/release/linux-x86_64 modde modde-ui
              cp target/modde-release/root-artifacts/release/linux-x86_64/modde "target/modde-release/root-artifacts/release/modde-''${VERSION}-x86_64-linux"
              cp target/modde-release/root-artifacts/release/linux-x86_64/modde-ui "target/modde-release/root-artifacts/release/modde-ui-''${VERSION}-x86_64-linux"

              nix build .#modde-aarch64-linux --out-link target/modde-release/root-artifacts/aarch64-linux-result
              mkdir -p target/modde-release/root-artifacts/release/linux-aarch64
              copy_nix_binary target/modde-release/root-artifacts/aarch64-linux-result modde target/modde-release/root-artifacts/release/linux-aarch64/modde
              copy_nix_binary target/modde-release/root-artifacts/aarch64-linux-result modde-ui target/modde-release/root-artifacts/release/linux-aarch64/modde-ui
              tar czf "target/modde-release/root-artifacts/release/modde-''${VERSION}-aarch64-linux.tar.gz" -C target/modde-release/root-artifacts/release/linux-aarch64 modde modde-ui
              cp target/modde-release/root-artifacts/release/linux-aarch64/modde "target/modde-release/root-artifacts/release/modde-''${VERSION}-aarch64-linux"
              cp target/modde-release/root-artifacts/release/linux-aarch64/modde-ui "target/modde-release/root-artifacts/release/modde-ui-''${VERSION}-aarch64-linux"

              nix build .#modde-windows --out-link target/modde-release/root-artifacts/windows-result
              mkdir -p target/modde-release/root-artifacts/release/windows-x86_64
              cp target/modde-release/root-artifacts/windows-result/bin/modde.exe target/modde-release/root-artifacts/windows-result/bin/modde-ui.exe target/modde-release/root-artifacts/release/windows-x86_64/
              if [ -f target/modde-release/root-artifacts/windows-result/bin/libmcfgthread-2.dll ]; then cp target/modde-release/root-artifacts/windows-result/bin/libmcfgthread-2.dll target/modde-release/root-artifacts/release/windows-x86_64/; fi
              # Windows tar.gz/zip and individual signed .exe assets are produced
              # by the Authenticode signing step, after the .exe files are signed.

              nix build .#modde-darwin-aarch64 --out-link target/modde-release/root-artifacts/darwin-arm-result
              mkdir -p target/modde-release/root-artifacts/release/darwin-aarch64
              cp target/modde-release/root-artifacts/darwin-arm-result/bin/modde target/modde-release/root-artifacts/darwin-arm-result/bin/modde-ui target/modde-release/root-artifacts/release/darwin-aarch64/
              tar czf "target/modde-release/root-artifacts/release/modde-''${VERSION}-aarch64-darwin.tar.gz" -C target/modde-release/root-artifacts/release/darwin-aarch64 modde modde-ui

              nix build .#appimage-ui --out-link target/modde-release/root-artifacts/appimage-ui-result
              cp target/modde-release/root-artifacts/appimage-ui-result "target/modde-release/root-artifacts/release/modde-ui-''${VERSION}-x86_64.AppImage"
              nix build .#appimage-cli --out-link target/modde-release/root-artifacts/appimage-cli-result
              cp target/modde-release/root-artifacts/appimage-cli-result "target/modde-release/root-artifacts/release/modde-''${VERSION}-x86_64.AppImage"

              git archive --format=tar.gz --prefix=rs-modde/ -o "target/modde-release/root-artifacts/release/rs-modde-''${VERSION}.tar.gz" HEAD
              source_sha256="$(sha256sum "target/modde-release/root-artifacts/release/rs-modde-''${VERSION}.tar.gz" | awk '{print $1}')"
              nix build .#flatpak-manifest --out-link target/modde-release/root-artifacts/flatpak-result
              cp target/modde-release/root-artifacts/flatpak-result target/modde-release/root-artifacts/release/com.tartanoglu.modde.json
              sed -i "s/@SOURCE_TARBALL_SHA256@/''${source_sha256}/" target/modde-release/root-artifacts/release/com.tartanoglu.modde.json
              nix run .#flatpak-cargo-generator -- Cargo.lock -o target/modde-release/root-artifacts/release/cargo-sources.json
              cp target/modde-release/root-artifacts/srpms/*.src.rpm target/modde-release/root-artifacts/release/ 2>/dev/null || true
            ''
          ];
        };
        release.attic = {
          cache = "canix";
          url = "https://attic.candee.baby";
          token_name = "rs-modde";
          result_links = ["target/modde-release/root-artifacts/linux-result" "target/modde-release/root-artifacts/aarch64-linux-result" "target/modde-release/root-artifacts/windows-result" "target/modde-release/root-artifacts/darwin-arm-result" "target/modde-release/root-artifacts/appimage-ui-result" "target/modde-release/root-artifacts/appimage-cli-result" "target/modde-release/root-artifacts/flatpak-result"];
        };
        release.announce = {};
        release.windows_signing = {
          binaries = ["modde" "modde-ui"];
          sign_name = "modde";
          sign_url = "https://modde.tartanoglu.com";
          tar_archive = "modde-{version}-x86_64-windows.tar.gz";
          zip_archive = "modde-{version}-x86_64-windows.zip";
        };
        flatpak = {
          repo = "flathub/com.tartanoglu.modde";
          app_id = "com.tartanoglu.modde";
          manifest_files = ["com.tartanoglu.modde.json" "cargo-sources.json"];
        };
        winget = {
          package_id = "Caniko.Modde";
          download_repo = "caniko/rs-modde";
          zip_archive = "modde-{version}-x86_64-windows.zip";
        };
        homebrew = {
          name = "modde";
          tap_url = "https://codeberg.org/caniko/homebrew-modde.git";
          download_repo = "caniko/rs-modde";
          binaries = ["modde" "modde-ui"];
          description = "Cross-platform game mod manager";
          homepage = "https://modde.tartanoglu.com";
          license = "GPL-3.0-only";
          archive_pattern = "modde-{version}-{arch}-{os}.tar.gz";
          platforms.darwin_intel = false;
        };
        chocolatey = {
          name = "modde";
          download_repo = "caniko/rs-modde";
          description = "Cross-platform game mod manager";
          project_url = "https://modde.tartanoglu.com";
          authors = "Can H. Tartanoglu";
          license_url = "https://raw.githubusercontent.com/caniko/rs-modde/trunk/LICENSE";
          archive_pattern = "modde-{version}-{arch}-windows.zip";
          # Interim: pull choco from the fork that ships the chocolatey package
          # (caniko/nixpkgs add-chocolatey-scoop). Drop the nixpkgs ref to
          # plain `nixpkgs#chocolatey` once it lands upstream.
          nix_tool = "github:caniko/nixpkgs/add-chocolatey-scoop#chocolatey";
          api_key_secret = "CHOCOLATEY_API_KEY";
          api_key_env = "CHOCOLATEY_API_KEY";
        };
        scoop = {
          name = "modde";
          bucket_url = "https://github.com/caniko/scoop-modde.git";
          bucket_token_secret = "CODEBERG_TOKEN";
          download_repo = "caniko/rs-modde";
          description = "Cross-platform game mod manager";
          homepage = "https://modde.tartanoglu.com";
          license = "GPL-3.0-only";
          archive_pattern = "modde-{version}-{arch}-windows.zip";
          binaries = ["modde" "modde-ui"];
          architectures.arm64 = false;
        };
        aur = {
          name = "modde";
          description = "Cross-platform game mod manager";
          license = "GPL-3.0-only";
          maintainer = "Can H. Tartanoglu <caniko@codeberg.org>";
          maintainer_gpg = "818D507F1E62139F8A17EAA64623DEA06FDACFE1";
          depends = ["dbus" "gcc-libs" "glibc" "libxkbcommon" "openssl" "sqlite" "vulkan-icd-loader" "wayland"];
          makedepends = ["cargo" "cmake" "pkgconf" "rust"];
          bin_glibc_min = "2.38";
          binaries = ["modde" "modde-ui"];
          assets = [
            {
              source = "dist/modde-ui.desktop";
              dest = "usr/share/applications/com.tartanoglu.modde.desktop";
            }
            {
              source = "dist/com.tartanoglu.modde.png";
              dest = "usr/share/icons/hicolor/512x512/apps/com.tartanoglu.modde.png";
            }
            {
              source = "dist/assets/logo/logo.svg";
              dest = "usr/share/icons/hicolor/scalable/apps/com.tartanoglu.modde.svg";
            }
            {
              source = "dist/com.tartanoglu.modde.metainfo.xml";
              dest = "usr/share/metainfo/com.tartanoglu.modde.metainfo.xml";
            }
          ];
          download_repo = "caniko/rs-modde";
        };
        copr = {
          name = "modde";
          spec_path = "dist/rpm/modde.spec";
          summary = "Cross-platform game mod manager";
          description = "modde is a cross-platform game mod manager with CLI and GUI interfaces.\nIt supports Nexus Mods, Wabbajack modlists, FOMOD installers, and BAIN\npackages for games like Skyrim, Fallout, Starfield, and Cyberpunk 2077.";
          license = "GPL-3.0-only";
          download_repo = "caniko/rs-modde";
          nix_tool = ".#copr-cli";
          build_requires = ["rust >= 1.93" "cargo" "gcc" "pkg-config" "openssl-devel" "dbus-devel" "wayland-devel" "libxkbcommon-devel" "vulkan-loader-devel"];
          binaries = ["modde" "modde-ui"];
          project = "caniko/rs-modde";
          login_secret = "COPR_LOGIN";
          username_secret = "COPR_USERNAME";
          token_secret = "COPR_TOKEN";
        };
        apt = {
          repo_url = "ssh://git@github.com/caniko/apt-modde.git";
          public_url = "https://apt.modde.tartanoglu.com/";
          pages = {
            provider = "codeberg-git-pages";
            runner = "atlas-nix-trusted";
          };
          label = "modde";
          # cargo-target=deb-name (cargo-deb names the file after [metadata.deb].name)
          packages = ["modde=modde"];
          build_deps = ["ca-certificates" "gcc" "libdbus-1-dev" "libsqlite3-dev" "libssl-dev" "libvulkan-dev" "libwayland-dev" "libxkbcommon-dev" "pkg-config"];
          gpg_key_secret = "MODDE_APT_REPO_GPG_KEY";
          gpg_key_id_secret = "MODDE_APT_REPO_GPG_KEY_ID";
          gpg_passphrase_secret = "MODDE_APT_REPO_GPG_PASSPHRASE";
          ssh_key_secret = "MODDE_APT_REPO_SSH_KEY";
        };
      };
    }
    // mkOutputs {};
}
