# Cryoglyph compatibility backport

This directory contains the published `cryoglyph 0.1.0` crate, with its `lru`
dependency changed from `0.16` to `0.18.2` in both Cargo manifests. Rust and WGSL
sources, examples, benchmarks and licenses are unchanged.

Upstream fixed the dependency in
[iced-rs/cryoglyph#5](https://github.com/iced-rs/cryoglyph/pull/5), commit
`a7a4b4b5a9fe9bf621c5c858489520fe0661ae5f`. That revision uses WGPU 29 and a
newer Cosmic Text fork. Modde's Iced 0.14 renderer uses WGPU 27 and Cosmic Text
0.15, so this backport retains the published graphics API while consuming the
patched `lru` API. It addresses
[RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html).

Source archive:
`https://static.crates.io/crates/cryoglyph/cryoglyph-0.1.0.crate`

Archive SHA-256:
`08bc795bdbccdbd461736fb163930a009da6597b226d6f6fce33e7a8eb6ec519`

Reproduce with `python3 scripts/vendor-cryoglyph.py`; verify without writing with
`python3 scripts/vendor-cryoglyph.py --check`. An existing crates.io archive can
be supplied with `--archive /absolute/path/cryoglyph-0.1.0.crate` for offline
verification. The generator checks the archive digest and applies exactly the
two manifest substitutions. `.cargo_vcs_info.json` retains the upstream source
revision and the original authors/licenses remain intact.

The Nix source includes this directory. Native, cross and test derivations use
`cargoArtifacts = null` and compile real sources: Crane's dependency-only build
replaces local path patches with dummy APIs, which Harbor correctly rejects for
registry dependencies. Shared sandbox compiler caching remains available.

Remove this Cargo patch and directory when a compatible upstream release contains
the fixed dependency, or when Modde adopts a newer Iced graphics stack.
