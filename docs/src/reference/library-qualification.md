# Library and sandbox qualification

Owned-game launching and optional sandboxing remain **Partial**. The source
implements saved per-installation launch configuration, profile/save transitions,
exact-install store hooks, durable descendant observation and paired performance
capture. Automated fixtures establish these contracts; real-game and distribution
qualification is still required before treating modde as a qualified Lutris
replacement or claiming negligible sandbox overhead.

The required implementation target is **native Linux**. Flatpak host-launch
integration is a follow-up. Performance measurements are **report-only**, as
selected by the user on 2026-10-01: retain measured deltas and evidence without a
negligible-overhead pass/fail verdict.

## Repeatable gates

From the development shell:

```sh
cargo xtask library-qualify --jobs 4
cargo xtask library-qualify --containment --jobs 4
```

The first command checks the minimal core/games/sources/CLI builds, each of the
nine individual CLI game features, save-recovery and installation boundaries,
supervision/completion receipts, performance sample gates, bisect provenance and
Library UI race guards. The second also runs real bubblewrap probes. It requires
Linux, bubblewrap on PATH and usable user namespaces; unavailable containment
fails the gate rather than skipping it. The development shell supplies bubblewrap
and SQLite. SQLite is also required by manager integration tests.

`cargo xtask check`, used by the generated project-check CI step, runs these
qualification gates after the workspace checks and requires real containment on
Linux. `checks.library-regressions` supplies deterministic regression coverage
inside the Nix build sandbox; real namespace creation is tested on the CI host.
Validate that Nix gate with:

```sh
canix cache binary build .#checks.x86_64-linux.library-regressions --include-tests --no-push --max-jobs 1 --cores 2
```

## Evidence from 2026-10-01

- Workspace/all-target compilation and strict all-feature Clippy passed.
- After GPU integration, the all-feature workspace suite passed 2,076 tests;
  the earlier default-feature suite passed 2,064 tests. Each run had two existing
  ignored tests. Fleetix's all-feature suite and strict Clippy also passed.
- Minimal builds and the nine individual game-feature builds passed.
- The production native Nix package built successfully. Its installed CLI passed
  a sandbox lifecycle fixture: detached descendants completed before the session
  ended, and exit codes 0 and 7 produced durable aggregate evidence of 0 and 1792
  respectively. This fixture does not establish rendering or game performance.
- Rust 1.93.0 passed workspace/all-target/all-feature compilation. Its invocation
  removed the development shell's nightly-only compiler flags; it did not use
  `RUSTC_BOOTSTRAP` or bypass the declared MSRV.
- Real bubblewrap 0.12.0 probes passed for read-only/writable grants, save and
  prefix writes, private HOME, inherited credential/session-bus exclusion, PID
  isolation and networking both enabled and disabled.
- A later Atlas desktop probe confirmed read/write access to the RX 7900 XTX
  render node and its Wayland socket. The `qualify_gpu` example exercised stable
  PCI selection through modde's direct and bubblewrap boundaries; both reported
  accelerated RX 7900 XTX OpenGL rendering, and Vulkan enumerated the RX 7900 XTX
  first with matching PCI IDs on both paths. Evidence is retained at
  `/data/scratch/tmp/opencode/modde-gpu-rendering-20261001.json`. This supersedes
  the earlier no-device observation and establishes a graphics diagnostic,
  not GUI-to-game or performance qualification.
- Toolbelt render-route assertions passed, covering topology defaults, application
  overrides, disabled defaults, invalid nodes and older-module compatibility.
  A source-level evaluation with modde's actual runtime Home Manager module
  verified `MODDE_GPU_RENDER_NODE` generation and override precedence. Fleetix's
  example Pkl topology exported the new render-route contract successfully.
- The scoped Fleetix GPU revision `c79c9a3902f746ec17f56de37f2c351e2dcdfbc3`
  passed hosted CI, including flake checks, both feature suites, MSRV, audit,
  documentation, strict Clippy and packaging. Toolbelt revision
  `afd6a7ced2ebea3b98705ca875d26a7acd2e5792` passed the exact pinned GPU/backend/
  browser Nix evaluations and cross-adapter override assertions. Its broader CI
  failed because published Toolbelt trunk already expects Fleetix's missing
  `publicationAddressIntents`; the same failure was reproduced on unmodified
  trunk `2faa0dd33f17386fb9123c2015bf6593d0159e40`.
  The separate browser smoke job also failed with `binary is not a Firefox
  executable`; its derivation is byte-identical on trunk and the GPU branch.
  Browser adapter, evaluation and runtime jobs passed in the PR workflow.
- Canix's normal producer refreshed its vendored Fleetix schema and topology;
  all 50 authored Pkl files validated, and the topology sidecar is current.
  Fleet-wide routes retain Nomad's independent media selection and Murph's
  oneAPI compute selection. The pinned Toolbelt adapter also passed against
  modde's actual local runtime module with host defaults and application overrides.
- The detached-process probe first exposed launcher-exit truncation: bubblewrap
  exited successfully and killed a still-running descendant. The in-namespace
  observer now retains that descendant until completion and propagates its
  nonzero exit. Both exit-0 and exit-7 probes pass.
- A read-only catalogue scan with isolated modde configuration/data found 477
  entries and 48 installed entries across Steam, GOG and Epic. This establishes
  discovery, not live launching. Heroic's GOG ownership cache was an empty
  object, so ownership remained unavailable instead of being treated as a
  complete empty library. Steam account sync and stale Epic ownership remain
  separate prerequisites.

## Outstanding qualification

| Gate | Required evidence or prerequisite |
| --- | --- |
| Nix regression gate | The source-filter issue is resolved; committed implementation `f09ce08` passed the exact gate recorded below. Diagnostic follow-ups require a new exact-source/package receipt. |
| GUI-to-Play | Atlas hardware-rendering diagnostics pass outside/inside bubblewrap. Live GUI-to-game rendering, saved configuration and completion evidence remain required. |
| Steam and Heroic | Real exact-install hooks, launcher/cloud-sync setup, native and Wine/Proton/UMU commands, first-run prefix behavior and save continuity. |
| Manager | Live manager forwarding, root lease/marker lifetime and recovery after interruption. |
| Native nested runtimes | Verify supervision, mounts and failure without an unsandboxed fallback for actual Proton/UMU commands. |
| Performance | Release/package runs and retained CSV/configuration/process evidence. Results are report-only. No real-game overhead has been measured. |
| Dependency health | Current `cryoglyph`/Iced dependency on `lru 0.16.4` still carries RUSTSEC-2026-0253. Other unmaintained/yanked warnings also remain. |
| Whole-repository documentation | Badge destinations and Simit policy are repaired; pinned generation and `docs-validate` passed for `416b6dd`. Follow-up generated CI and operational docs need current drift validation. |
| Canix integration rollout | The earlier database-input, DNS, browser and old-Modde-module blockers are resolved. `9d0f1ce46` consumes the published GPU implementation, and Atlas evaluation/package lifecycle passed below. Follow-up logging/database/manager wiring needs matching pins and evaluation receipts before rebuild. |

`event-listener 5.4.2` and `lru 0.18.2` replace the affected versions on the
dependency edges that accept those patches. Audit warnings are not a clean
security qualification merely because `cargo audit` returns zero.

For performance, use alternating off/on pairs against one installation/profile,
with identical settings and a repeatable scene. The proposed protocol is eight
pairs of approximately five-minute runs and 30 seconds of warmup. Logging
duration does not terminate the game. Capture/grading requires successful observed
exits and at least 100 usable FPS samples and 100 measured frame-time samples
after warmup; frame times are never invented from FPS. Report the observed
paired deltas and variability without assigning a negligible-overhead verdict.

Flatpak packaging and its host-runtime bridge remain unimplemented follow-up
work, outside the required native-Linux launch target.

## Repeatable GPU probe and deployment integration

Run the opt-in hardware diagnostic in a desktop session (diagnostic tools are
not required by ordinary launches or headless CI):

```sh
cargo run --locked -p modde-games --example qualify_gpu -- /absolute/path/to/glxinfo /absolute/path/to/report.json /dev/dri/by-path/pci-0000:03:00.0-render /absolute/path/to/vulkaninfo
```

The optional final argument also checks that Vulkan enumerates a hardware
device first with matching PCI IDs through both boundaries. It does not prove
which device a real Vulkan game chooses.

Fleetix's optional `gpu.render.renderNode` declares a portable 3D route. Its Pkl
schema owns stable PCI identities and GPU vocabulary; a generated contract and
conformance fixtures bind the Rust parser and Nix projections to that source.
The Pkl evaluator and Tokio are optional for Rust library consumers. modde's
live device validation, saved installation overrides, sandbox grants and process
evidence remain application-owned.

The shared Rust `fleetix::gpu::pci_selector` API awaits a crate release; published
Fleetix `0.3.0` does not provide it. modde retains its small runtime parser until
that API is available through a published Cargo dependency.

Canix declares Atlas's rendering route in host topology and forwards Fleetix's
purpose-specific `gpuRoutes`. Toolbelt resolves defaults consistently for
integrated and standalone Home Manager. Its `modde-gpu` adapter supplies the
application default; browser and mpv media adapters use the resolved media route
and respect Home Manager overrides. Rendering, media and compute remain separate
roles; mpv retains automatic presentation selection and copy-mode decoding.

Scoped GPU changes are published on upstream integration branches before their
consuming flake pins and Canix schema/topology sidecars are regenerated. Activation
also requires a modde pin containing the runtime module's GPU option. An older
modde module fails the enabled adapter's compatibility assertion instead of
silently ignoring the route. Publishing the shared contract is not live-game
qualification.

Performance configuration and durable process evidence now retain GPU routing,
PCI IDs, kernel driver, kernel version and immutable Nix graphics-driver roots.
Baselines without this provenance require recapture. Paired measurements reject
changed provenance and remain report-only.

## Publication follow-up, 2026-10-02

Publication of the implementation was authorized on 2026-10-02. The changes are
grouped into host-independent OptiScaler qualification, Rust quality cleanup,
dependency requirements, launch/save/supervision/GPU evidence, Library UI, and
Nix/Home Manager qualification commits. Implementation revision `f09ce08`
contains the GPU-capable runtime module.

The OptiScaler failure inside Nix came from a test assuming the developer's
RDNA3 GPU. Configuration now accepts an injected architecture internally, and
the integration test checks RDNA3, RDNA4, and unknown hardware deterministically.
The production entry point still detects live hardware. All-feature workspace
tests, strict all-target/all-feature Clippy, and real containment qualification
passed again. Documentation validation and pinned Simit badge generation pass;
the unsupported `ci.check_command` was replaced by a required qualification gate.

The committed-source Nix `library-regressions` gate passed:

- Derivation: `/nix/store/dl9js1argqxia67khy23jj84yb0ysgr5-modde-test-0.7.0.drv`.
- Output: `/nix/store/gk69xzklyhrl03j1zvkn5r3yxqmx2qxc-modde-test-0.7.0`.
- Command: `canix cache binary build .#checks.x86_64-linux.library-regressions --include-tests --no-push --max-jobs 1 --cores 2`.

Rechecked upstream heads are still open PRs:

- Fleetix `4465108ce8160bdb7b9510211669099ee3a83ac4`: PR CI
  [36934171743](https://github.com/caniko/fleetix/actions/runs/36934171743) passed.
- Toolbelt `9caf679440076addbde1ba9bb0786a29d59cd5f6`: PR CI
  [36935397429](https://github.com/caniko/canix-toolbelt/actions/runs/36935397429)
  and browser qualification
  [36935397380](https://github.com/caniko/canix-toolbelt/actions/runs/36935397380)
  passed, including the real Floorp smoke and exact tab preservation.

Canix commit `56954009d` consumes these revisions and producer-regenerated
cloud/publication schema and topology. Topology is current and all 50 Pkl files
validate. The Toolbelt/local-modde module evaluation resolves Atlas's host
default to `/dev/dri/by-path/pci-0000:03:00.0-render`, permits an explicit
application override, and retains independent media policy.

These gates qualify implementation contracts. Live games, store/manager
integration, real Proton/UMU, GUI-to-game save continuity, packaged paired
measurements, and actual game-renderer evidence remain outstanding. Measurements
remain report-only.

Published implementation `416b6dd1cd2f93349e0af7c32e12c93c7d2e32f6`
is available in [Modde PR #3](https://github.com/caniko/modde-rs/pull/3), with
NAR hash `sha256-vIn8VOVaaU5uwAElL+8gNPL5XB5uINiUI/p/WVofzbU=`.
Canix commit `9d0f1ce46` consumes this revision and its transitive Simit pin.
Full Atlas toplevel evaluation succeeds:
`/nix/store/yzsxwr6gpmhlyzmd1hjbmn9cpq05vchf-nixos-system-atlas-26.11.20260823.56c02bc.drv`.
The older-module GPU compatibility blocker is resolved; no activation occurred.

The updated native package also builds:
`/nix/store/lxb2y07qq4j4b1ykjba696msd4q6lg49-modde-0.7.0`.
Synthetic packaged sandbox launches retained detached descendant completion,
aggregate raw exit statuses `0` and `1792` (exit code 7), requested AMD render-node
routing and driver/inventory provenance after private launch-request removal.
Both launches cleared their completion journals. Receipt:
`/data/scratch/tmp/opencode/modde-package-gpu-0dvy2lsx/receipt.json`.
This is packaged lifecycle evidence, not an actual game-renderer or performance
measurement.

## Diagnostics readiness follow-up

The follow-up implementation adds pre-preflight correlated launch records,
private supervisor-owned output, GUI tracing, recent-run/log inspection, redacted
export, protected retention and separate boundary/inner startup evidence. It
restricts automatic raw input-device grants to controllers and requires initialized
physical prefixes and a saved Steam Cloud-disabled assertion for managed saves.

The new exact-package fixture can be run against a built native CLI:

```sh
cargo xtask library-package-qualify --binary /absolute/package/bin/modde --output /absolute/new-receipt-directory
```

It uses isolated configuration, SQLite, HOME and data directories and real
bubblewrap. It checks successful and detached-failure completion, raw signal
status, the private inner-observer channel, retained regular-file game output,
secret exclusion from exports, request consumption and cleared journals. It
does not establish actual game rendering, store-provider behavior or performance.

Local follow-up checks on 2026-10-02 passed the all-feature workspace suite,
strict all-target/all-feature Clippy, Rust 1.93.0 all-target/all-feature compilation,
minimal and nine-feature builds, containment, diagnostic retention/export and
supervision recovery. The real CLI fixture retained successful status `0`,
detached-failure status `1792`, signal status `15` and recoverable inner-exec
failure in `/data/scratch/tmp/opencode/modde-diagnostics-dev-fixture-20261002-v3/receipt.json`.
This is a development-binary fixture, not a qualified Nix package receipt.

Disposable PostgreSQL migration evidence at
`/data/scratch/tmp/opencode/modde-postgres-readiness-wdrlNk/receipt.txt` reports
`modde_fixture|f|t|25`: a non-superuser with schema creation rights created all
25 tables. Live Atlas inspection was read-only and reports `can|f|t|t|25` for
role, superuser, schema usage/create and table count. No activation occurred.

The newly generated GitHub checks cover workspace tests/Clippy, exact native
package and Nix regressions, Home Manager runtime wrappers, and real packaged
lifecycle. Pages now derives its environment and cache setup from the generator
and shared CI configuration. Simit PR #27 fixes the missing setup propagation;
Modde backports that fix and the upstream Pages environment/result-link fixes
to its pinned generator. The all-workflow `simit init ci --check --diff` gate
passes with no generated-file drift.

Hosted containment jobs configure Ubuntu's unprofiled user-namespace policy
on their disposable runner before testing bubblewrap's PID/network isolation.
The launch path retains its sandbox preflight and never retries unsandboxed.
Default public flake outputs do not consume private Apple SDK metadata. Authorized
macOS cross-build callers use `lib.mkOutputs` with explicit `macosSdkStorePath`,
`macosSdkOutputHash` and `osxSdkVersion`; without an SDK, the cross-build package
reports an unavailable prerequisite. Native Linux outputs require no SDK credentials.
