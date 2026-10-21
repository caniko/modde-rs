{
  harbor-rs,
  rustfmtPackage,
}: _: {
  imports = [
    harbor-rs.inputs.harbor-meta.treefmtModules.nix
    harbor-rs.inputs.harbor-meta.treefmtModules.toml
    harbor-rs.treefmtModules.rust
  ];
  projectRootFile = "flake.nix";

  # rustfmt comes from harbor-rs's pinned nightly profile, never a floating
  # `nightly.latest`, so local, CI and fleet formatting agree.
  programs.rustfmt = {
    edition = "2024";
    package = rustfmtPackage;
  };

  # The vendored Cryoglyph backport is checksum-verified upstream source.
  settings.global.excludes = ["vendor/**"];
}
