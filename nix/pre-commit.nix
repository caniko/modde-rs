{
  pkgs,
  treefmtWrapper,
  rustToolchain ? null,
}: {
  treefmt = {
    enable = true;
    name = "treefmt";
    package = treefmtWrapper;
    entry = "${treefmtWrapper}/bin/treefmt --fail-on-change";
    pass_filenames = false;
    stages = ["manual"];
  };

  cargo-fmt = {
    enable = true;
    name = "cargo fmt";
    entry = "cargo fmt --all -- --check";
    extraPackages = pkgs.lib.optional (rustToolchain != null) rustToolchain;
    pass_filenames = false;
  };

  cargo-clippy = {
    enable = true;
    name = "cargo clippy";
    entry = "cargo clippy --all-targets --all-features -- --deny warnings";
    extraPackages = pkgs.lib.optional (rustToolchain != null) rustToolchain;
    pass_filenames = false;
    stages = ["pre-push" "manual"];
  };

  cargo-check = {
    enable = true;
    name = "cargo check";
    entry = "cargo check --workspace --all-features";
    extraPackages = pkgs.lib.optional (rustToolchain != null) rustToolchain;
    pass_filenames = false;
    stages = ["pre-push" "manual"];
  };

  cargo-audit = {
    enable = true;
    name = "cargo audit";
    entry = "cargo audit";
    extraPackages = pkgs.lib.optional (rustToolchain != null) rustToolchain ++ [pkgs.cargo-audit];
    pass_filenames = false;
    stages = ["manual"];
  };

  cargo-deny = {
    enable = true;
    name = "cargo deny";
    entry = "cargo deny check bans licenses sources";
    extraPackages = pkgs.lib.optional (rustToolchain != null) rustToolchain ++ [pkgs.cargo-deny];
    pass_filenames = false;
  };

  nix-flake-check = {
    enable = true;
    name = "nix flake check";
    entry = "nix --extra-experimental-features 'nix-command flakes' flake check --cores 0 --max-jobs auto --no-update-lock-file";
    extraPackages = [pkgs.nix];
    pass_filenames = false;
    stages = ["pre-push" "manual"];
  };
}
