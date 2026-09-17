# Contributing to modde

Thank you for your interest in contributing to modde!

## Getting Started

### Prerequisites

- [Nix](https://nixos.org/) with flakes enabled (recommended)
- Alternatively: Rust 2024 edition toolchain, SQLite, and system dependencies for Iced

### Development Setup

```sh
# Clone the repository
git clone https://github.com/caniko/modde-rs.git
cd modde-rs

# Enter the dev shell (provides all dependencies)
nix develop

# Project-local cargo alias: `cargo xtask ...` runs `crates/modde-xtask`.
# It depends on the flake-pinned `harbor-xtask` git crate, so a fresh
# checkout may fetch harbor-rs.

# Build
cargo build --workspace

# Run tests
cargo xtask test

# Run clippy
cargo xtask lint

# Format check
cargo xtask check

# Regenerate the committed Nix tool schema
just export-tool-schema

# Validate mdBook output, local docs links, and command examples
cargo xtask docs-validate

# Run the GUI directly with development logging
cargo xtask gui

# Override GUI logging when needed
RUST_LOG=modde_ui=trace,modde_core=debug cargo xtask gui
```

### Project Structure

```
crates/
  modde-core/     Core types, database, VFS, profiles, collision detection
  modde-sources/  Download backends (Nexus, Wabbajack, GitHub, MEGA, etc.)
  modde-games/    Game plugins, launcher detection, mod scanners
  modde-cli/      CLI interface (24 commands, 60+ subcommands)
  modde-ui/       GUI built with Iced
```

## Making Changes

1. Fork the repository on Codeberg
2. Create a feature branch from `trunk`
3. Make your changes
4. Ensure `cargo xtask check` passes
5. Submit a pull request

### Code Style

- Follow existing patterns in the codebase
- Workspace-wide clippy pedantic warnings are enforced
- Use `thiserror` for error types, `anyhow` for CLI/application errors
- Prefer small, focused commits with clear messages
- `nix/tool-schema.nix` is generated via `just export-tool-schema`; do not hand-edit it

### Adding Game Support

Game plugins implement traits in `crates/modde-games/src/traits.rs`:

- `GamePlugin` for game detection, paths, and configuration
- `ModScanner` for filesystem-based mod discovery
- `SaveTracker` for save file management

See `crates/modde-games/src/cyberpunk/` for a complete example.

### Where things are

When adding tests, benches, or coverage to the workspace, model new work on the
existing scaffolding rather than re-deriving it:

| Area | Entry point |
| ---- | ----------- |
| CLI test fixtures (isolates `MODDE_DATA_DIR` / `HOME` / XDG) | `crates/modde-cli/tests/common/mod.rs` |
| Wiremock HTTP-mocking pattern | `crates/modde-cli/tests/cli_nexus_status.rs`, `crates/modde-cli/tests/cli_install_mod.rs` |
| Criterion bench scaffolding | `crates/modde-core/benches/vfs_deploy.rs` |
| Proptest scaffolding | `crates/modde-core/tests/resolver_proptest.rs` |
| Concurrency / stress tests | `crates/modde-core/tests/concurrency_tests.rs` |
| Nexus base-URL override (point a client at a mock server) | `crates/modde-sources/src/nexus/mod.rs` — `base_url()` / `graphql_url()` |

The Nexus client honors `MODDE_NEXUS_BASE_URL` and `MODDE_NEXUS_GRAPHQL_URL`
environment variables so integration tests can point its REST v1 and GraphQL v2
calls at a wiremock instance instead of the live Nexus API.

#### Coverage

Coverage runs through the xtask wrapper (`cargo-llvm-cov` is in the devShell):

```sh
# Terminal summary
cargo xtask coverage

# HTML report at target/llvm-cov/html/
cargo xtask coverage --html

# lcov.info at target/llvm-cov/lcov.info (for badge/upload tooling)
cargo xtask coverage --lcov

# CI mode: gather once, write lcov + summary, fail under a threshold
cargo xtask coverage --ci --fail-under <N>
```

`--ci` is what Forgejo Actions runs; `--lcov` writes
`target/llvm-cov/lcov.info`, which is the input for any coverage badge or upload
step.

#### Fuzzing

`cargo-fuzz` requires nightly Rust, while `flake.nix` pins a stable toolchain.
Property-style fuzzing that must run on stable should use `arbitrary`-based
proptest targets (model after the proptest scaffolding above) rather than
`cargo-fuzz`.

## Reporting Issues

Please file issues on the [GitHub issue tracker](https://github.com/caniko/modde-rs/issues).

Include:

- modde version (`modde --version`)
- Operating system and distribution
- Steps to reproduce
- Expected vs actual behavior

## Releases

Release tags are bare semver names such as `0.2.0` or `1.0.0-rc.1`.
Fedora COPR wiring lives in the release workflow and the distribution-channel
table below; there is no separate COPR release document in this repository.

### Release tooling

- Run releases through `cargo xtask release {patch|minor|major|prerelease} -m "<message>"`.
- The xtask wrapper delegates to `simit release` from the devShell. For a new minor release candidate, use `cargo xtask release minor --pre rc.1 -m "Release 1.0.0-rc.1"`; for the next RC on an already-prerelease version, use `cargo xtask release prerelease --pre rc.2 -m "Release 1.0.0-rc.2"`.
- Tags are bare semver with no `v` prefix; `.forgejo/workflows/release.yml` triggers on `[0-9]*`.
- Stable tags must match `X.Y.Z`. Prerelease tags must match `X.Y.Z-rc.N`, `X.Y.Z-beta.N`, or `X.Y.Z-alpha.N`.
- Every tag must have a matching `## [X.Y.Z] - YYYY-MM-DD` or `## [X.Y.Z-rc.N] - YYYY-MM-DD` heading in `CHANGELOG.md` before CI will build.
- Prerelease tags run the full build, signing, smoke, Codeberg release, Attic push, and COPR upload, but Codeberg marks them as prereleases. Stable-only channels are skipped: crates.io, Homebrew, AUR `modde-bin`, winget, Scoop, and Flathub.
- COPR prereleases publish to `caniko/modde-rs-testing`; create that COPR project before the first RC tag.
- Keep the `## [Unreleased]` heading in `CHANGELOG.md` exactly as-is so simit can update it.
- modde-rs does not run `simit init-ci --check` or `simit init-flake --check`.
- Those checks would treat this repo's bespoke `atlas` workflows and harbor-rs-driven flake as drift.
- The rationale, revisit conditions, and other non-obvious choices live in the [Architecture reference](https://modde.tartanoglu.com/docs/reference/architecture.html) (see its "Release, packaging, and tooling decisions" section).

### Distribution channels

A stable tag fans out to several downstream channels from the single
`.forgejo/workflows/release.yml` run; prerelease tags publish only the
prerelease-safe subset. Each external channel is gated on its credential being
present and **skips with a warning when the secret is unset**, so pull requests
and dry-run tags never fail on a missing publish credential. A production
release should treat such a skip as a release blocker for that channel.

| Channel | Artifact / target | Publish gate | State |
| ------- | ----------------- | ------------ | ----- |
| Codeberg release | tarballs, AppImages, SRPM, SBOMs, signatures | always (stable + prerelease) | Live |
| Nix flake / home-manager | flake outputs | n/a (consumed directly from the repo) | Live |
| Attic cache | `https://attic.candee.baby/canix` | always | Live |
| Fedora COPR | SRPM upload | always; prereleases land in `caniko/modde-rs-testing` | Wired, not publicly discoverable |
| Debian/Ubuntu APT | `.deb` via Simit's APT publisher to `caniko/apt-modde` | stable only; `modde_apt_repo_ssh_key` | Bootstrapped; deploy key pending |
| Arch AUR | `modde`, `modde-bin`, `modde-git` PKGBUILDs | stable only; `AUR_SSH_KEY` | Staged, not pushed |
| Flathub | `com.tartanoglu.modde` manifest PR | stable only; `FLATHUB_TOKEN` | Staged, submission not accepted |
| crates.io | per-crate `cargo publish` | stable only | Stable-only |
| Homebrew tap | `caniko/homebrew-modde` formula | stable only | Staged |
| winget | `Caniko.Modde` PR to `microsoft/winget-pkgs` | stable only; `WINGET_PAT` | Staged |
| Scoop | `caniko/scoop-modde` bucket | stable only; `CODEBERG_TOKEN` | Wired; first release pending |

Only the Nix flake, home-manager module, and the Attic cache are live for end
users today; APT and Scoop are bootstrapped and await the first successful
release publication. The honest per-channel status that ships to users lives in
[docs/src/getting-started/installation.md](docs/src/getting-started/installation.md);
keep that page and this table consistent when a channel goes live.

Channel notes worth remembering across releases:

- **Prerelease gating.** Tags matching `X.Y.Z-rc.N` / `-beta.N` / `-alpha.N`
  run the full build, sign, smoke, Codeberg release, and Attic push, are marked
  `prerelease: true` on Codeberg, and skip every stable-only channel
  (crates.io, Homebrew, AUR, winget, Scoop, Flathub, APT). COPR still builds
  but lands in `caniko/modde-rs-testing`.
- **Windows binaries are repackaged as `.zip`** alongside the `.tar.gz` for
  winget and Scoop, which do not accept `.tar.gz`. The Scoop `hash` matches the
  downloaded `.zip`, not the binaries inside it.
- **APT publishes over SSH**, not an HTTPS token: Simit's `dist apt publish`
  command builds the history-preserving `reprepro` tree and pushes the `pages`
  branch of `caniko/apt-modde` using the `modde_apt_repo_ssh_key` deploy key
  (write access), with temporary host-key and GPG handling. The public Pages
  endpoint is `https://apt.modde.tartanoglu.com/`; the repository signing-key
  fingerprint and key-rotation procedure are documented in [SECURITY.md](SECURITY.md).
- **Scoop uses the shared Codeberg credential**: Simit's release template maps
  the `CODEBERG_TOKEN` Actions secret to its bucket-publish environment, and
  updates `caniko/scoop-modde/bucket/modde.json` from the tagged Codeberg
  release ZIP.
- **macOS/Windows artifacts are experimental.** They build in CI but there is no
  published Codeberg release asset for them yet; do not advertise them as
  shipped.

### Website and Codeberg Pages

The modde website is published at `https://modde.tartanoglu.com/` from the
generated `site` flake output. The presentation site lives in `website/`, the
mdBook docs live in `docs/`, and the combined output puts docs under `/docs/`.

When creating or changing any website for this project family, follow the current
Codeberg Pages split:

- For a `*.codeberg.page` URL, use the new git-pages flow or Forgejo Actions
  publishing for that repository.
- For a custom domain, use Codeberg's legacy Pages flow until git-pages supports
  custom domains: publish static output to a `pages` branch and include a plain
  `.domains` file listing the custom domain.
- For `modde.tartanoglu.com`, `nix build .#site` must produce `.domains` with
  exactly `modde.tartanoglu.com`, and `.forgejo/workflows/pages.yml` publishes
  that output with `nix run .#deploy-pages`.
- Declare DNS in canix, not by hand in the Cloudflare UI. Codeberg Pages custom
  domains use an unproxied CNAME, currently `modde -> caniko.github.io`
  in `/data/nvme0/can/canix/lib/topology/Domains.pkl`.
- After DNS changes, run the canix DNS checks and plan/apply from
  `/data/nvme0/can/Projects/canix`:
  `nix build --no-link .#checks.x86_64-linux.dns-cloudflare-ddns-proxied-expression`,
  `nix build --no-link .#checks.x86_64-linux.dns-ddns-covered-by-excludes`,
  `nix build --no-link .#checks.x86_64-linux.dns-excludes-trace-to-ddns`,
  then `XDG_RUNTIME_DIR=/run/user/$(id -u) nix run .#dns-plan-local -- --doit`.

Validate a Pages deployment with:

```sh
nix build .#site
grep -qx 'modde.tartanoglu.com' result/.domains
nix run .#deploy-pages
git ls-remote --heads origin pages
curl -fsSI https://modde.tartanoglu.com/
curl -fsSI https://modde.tartanoglu.com/docs/
```

### Hotfix release

Hotfixes ship from the last good release tag, not from `trunk`, when `trunk`
contains unrelated or unfinished work.

```sh
git fetch --tags origin
git switch -c hotfix/0.2.0 0.2.0
git cherry-pick <fix-commit>
cargo xtask check
cargo xtask release patch -m "Release 0.2.1"
git push origin hotfix/0.2.0 0.2.1
```

For a hotfix on a prerelease line, use `cargo xtask release prerelease --pre
rc.2 -m "Release 0.3.0-rc.2"`. No back-merge from `trunk` is required to
publish a hotfix. After the release is out, either cherry-pick the fix back to
`trunk` or open a follow-up PR that explains why the fix is hotfix-only.

### Release announcements

Stable tags announce after the Codeberg release is created. Prerelease tags do
not announce. Configure these Forgejo secrets to enable announcements:

- `MASTODON_TOKEN` and `MASTODON_BASE_URL`, for example `https://fosstodon.org`
- `MATRIX_TOKEN`, `MATRIX_HOMESERVER`, and `MATRIX_ROOM`

When any of those secrets are absent, CI logs a skip and keeps the release
running. The announcement body includes the Codeberg release URL and the first
five lines from the matching `CHANGELOG.md` section.

### Yank / withdraw a release

Use this drill when a published stable release must be pulled. Announce the
withdrawal first if users may already have downloaded artifacts.

```sh
VERSION=0.2.1
```

- crates.io: `cargo yank --version "$VERSION" -p modde`; repeat for every published crate at that version.
- Codeberg release: delete or mark the release draft-only from the Codeberg UI, then delete the tag only if the tag itself is invalid: `git push origin ":refs/tags/${VERSION}"`.
- Homebrew tap: in `caniko/homebrew-modde`, revert the formula bump commit with `git revert <commit> && git push`.
- Debian / Ubuntu apt (once the channel is live): publish a corrected stable release to `https://modde.rs/apt/`, or temporarily remove the affected `.deb` entries from the served APT tree and force-push the rebuilt `pages` branch.
- AUR: revert the affected package repo commit and push. Use `git push --force-with-lease` only if the bad commit must disappear from the AUR history.
- winget: comment `Withdrawn: modde ${VERSION}` on the generated PR and close it. If merged, open a removal/revert PR in `microsoft/winget-pkgs`.
- Scoop: revert the bucket manifest bump in `caniko/scoop-modde` and push.
- Flathub: close the release PR if unmerged. If merged, open a `revert/${VERSION}` PR against `flathub/com.tartanoglu.modde`.
- COPR: find the build id with `copr-cli list-builds caniko/modde-rs --output-format json`, then run `copr-cli delete-build <build-id>`. Use `caniko/modde-rs-testing` for prerelease builds.

After rollback, add a `CHANGELOG.md` note under `Unreleased` that names the
withdrawn version and points to the fixed follow-up release.

### Supply-chain checks

The release path carries two supply-chain gates that contributors should expect
to interact with:

- **`cargo deny` runs on every PR and on release.** CI runs
  `cargo deny check` (advisories, bans, sources, licenses) as a parallel CI job,
  and the release workflow runs
  `cargo deny check -W unmaintained advisories bans sources licenses`
  before building. A known-vulnerable dependency fails the release; the only
  escape is an explicit, commented `[advisories].ignore` entry in `deny.toml`
  documenting the temporary exception. Keep vulnerability advisories denied and
  `unmaintained = "warn"` so routine unmaintained-transitive warnings do not
  block PRs.
- **SBOMs ship with every release.** The release generates both a CycloneDX JSON
  SBOM (`modde-<version>.cdx.json`) and an SPDX 2.3 JSON SBOM
  (`modde-<version>.spdx.json`) from the workspace `Cargo.lock`, uploads both to
  the Codeberg release, lists them in `SHA256SUMS.txt`, and signs/attests them
  with the same Sigstore step as the binaries. The human-readable
  `THIRD_PARTY_LICENSES.html` continues to ship alongside them. Consuming and
  scanning the SBOMs (grype / osv-scanner) is documented in
  [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed under GPL-3.0-only.
