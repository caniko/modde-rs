# Library and sandbox qualification

Owned-game launching and optional sandboxing remain **Partial**. The source
implements saved per-installation launch configuration, profile/save transitions,
exact-install store hooks, durable descendant observation and paired performance
capture. Automated fixtures establish these contracts; real-game and distribution
qualification is still required before treating modde as a qualified Lutris
replacement. Real-game sandbox performance remains unmeasured.

The required implementation target is **native Linux**. Flatpak host-launch
integration is a follow-up. Performance measurements are **report-only**, as
selected by the user on 2026-10-01: retain measured deltas and evidence without a
negligible-overhead pass/fail verdict.

## Current qualification, 2026-10-05

The approved producer revision is
`0d3ce666e2362555b90d4d37b084cc8d86450eb9`, published on
`integration/native-library-gpu` in [PR #3](https://github.com/caniko/modde-rs/pull/3).
Its [producer CI](https://github.com/caniko/modde-rs/actions/runs/37031697945)
and [native package CI](https://github.com/caniko/modde-rs/actions/runs/37031698041)
passed. Independent verification retained the hosted checkout/tree identities
and reports 2,092 passed tests, zero failures and two existing ignored tests:
`/data/scratch/tmp/opencode/durability-modde-0d3-independent-producer-verification.json`.
These results qualify that revision; later documentation or implementation
changes require their own checks before a replacement pin is selected.

The historical Canix consumer
`8240764b480d0fff6aab77ef50f8767b5941676e` realized and privately published
the actual Home Manager wrapper and native package:

- Wrapper: `/nix/store/zdyx23lxxyzbj4z2gnzy5h4h8amwfxjj-modde-desktop-runtime`.
- Native package: `/nix/store/8vbqbqbdp86qvrwc0yscicsz6ll2yfr2-modde-0.7.0`.
- Binding: `/data/scratch/tmp/opencode/modde-atlas-repaired-committed-runtime-binding-20261002.json`.

The actual-wrapper fixture used isolated SQLite/configuration/data directories
and real bubblewrap. It passed successful completion, detached descendant
failure, signal handling, failed inner-exec recovery, private/redacted diagnostics
and journal cleanup. Durable raw statuses were `0`, `1792`, `15` and `256`
respectively. The binding records the desktop entry, both wrapper defaults,
compiled producer revision, NAR/deriver identities, launch receipts, retained
output root and verified private publication. Defaults are PostgreSQL through
`/run/postgresql`, render node `/dev/dri/by-path/pci-0000:03:00.0-render`, log level
`info` and retention of 50 launches. The fixture's SQLite override does not
establish live PostgreSQL or real-game save continuity.

On 2026-10-04, read-only reverification confirmed those retained artifacts and
receipts:
`/data/scratch/tmp/opencode/modde-native-closeout-historical-verification-20261004T194904215074Z/receipt.json`
(SHA-256 `4c272affec938bba414c72857199992683aa4a0a677a285ee664d4c0134ea81d`).
This remains historical component evidence. A successor Canix source requires
measured package, wrapper, defaults and dependency-context equality before
execution evidence transfers. Equal source files alone do not establish it.

Canix qualification runs locally on Atlas; its hosted Actions are
**skipped-by-operator**. The coordinator selected
`3b29bb33d125381f9d34b4f18657755cbe4a0379` for scoped non-activating qualification
after reviewing the complete successor delta. The predecessor online VM failed
because its stdout-only capture missed the intentional stderr stop; the corrected
fixture preserves all 13 lifecycle assertions. That failure and earlier failed
or cancelled attempts remain retained. Selection authorizes component checks,
not production adoption or activation. Final composition, full local registry
qualification and the production writer window remain coordinator-owned.

Live game testing starts after those recovery/adoption/activation gates. At
runtime entry, reconfirm Modde inactivity and the actual foreground marker/pause
policy. Native direct, Steam, Heroic, Proton/UMU and manager paths need observed
rendering, interruption recovery and save-continuity receipts. Eight alternating
off/on measurement pairs require about 80 minutes of gameplay plus setup;
retain paired deltas and variability without an overhead acceptance verdict.
PR review/merge readiness remains separate from successful component CI.

### Selected Canix binding and component acceptance

The selected `3b29bb33…` combined binding and coordinator post-checks passed.
Independent verification covers source NARs, ten standalone/embedded homes,
41 recovery-record declarations, separate configured and final-service PostgreSQL
packages, opaque derivation graphs, verified private `canix-fleet` publication
and physical/registered managed roots. The retained inventory contains 72 exact
derivations: 70 explicit selections plus the measured manager package/config
output contexts. Evidence:

- Sealed worker handoff:
  `/data/scratch/tmp/opencode/durability-3b29-green-binding-worker-handoff-20261004T220520080406Z.json`
  (SHA-256 `27fbdd313cb10fe9edaa55b5b276d6dc65a49a92e04b8bf53c560e165d9bdfa6`).
- Combined binding:
  `/data/scratch/tmp/opencode/durability-3b29-coordinator-combined-context-v2-20261004T213013602808Z/binding.json`
  (SHA-256 `81e06c9737a36177488b9b2254231933c07ac1c5f10ff07b7ccfe68aa53fa0b6`).
- Independent binding/root verification:
  `/data/scratch/tmp/opencode/durability-3b29-bound-selection-context-v5-independent-20261004T220352406667Z/receipt.json`
  (SHA-256 `f0972b6f996b28b023975277f63b3b64985aed2ad1cf84d6700bb5d56b0ff49f`).
- Actual rendered production shell: unsuppressed ShellCheck and all twelve cleanup
  cases passed in
  `/data/scratch/tmp/opencode/durability-3b29-authoritative-full-script-native-20261004T215948931439Z/receipt.json`
  (SHA-256 `f6bab51b5ac28f6b95f8bf3f9b01a11d204f91a99a3b353c05b8cb71e9172933`).

Measured equality transferred the historical Modde wrapper/package and remote-policy
results to this selection. The coordinator independently accepted artifact/NAR,
deriver/reference, five-input cohort, defaults, database ownership, four launch
statuses, remote opaque graph, private publication and root equality:
`/data/scratch/tmp/opencode/durability-3b29-modde-remote-transfer-independent-20261004T221931871448Z/receipt.json`
(SHA-256 `940b25629498d6d1220392bbb945bceb654f312dba4b24b19a382b6eefbd99f0`).
The launch evidence retains its disposable SQLite/real-bubblewrap scope.

Modde's storage cohort uses final-service PostgreSQL
`/nix/store/rnyi9ykl60rghq1z2nbvl29jljjchak7-postgresql-and-plugins-18.6`.
The separately verified configured PostgreSQL package has a different output
(`3hc…`); it must not substitute for the final-service binding.

### Corrected online VM result

The sole actual corrected `pg-online-backup-only` retry passed on 2026-10-04.
The reviewed v2 runner realized the selected opaque graph with normal capacity
admission, one job/two cores and no further flake evaluation:

- Derivation: `/nix/store/cn472qzq1g4ng8d9fm9pskpccll4jgap-vm-test-run-canix-pg-online-backup-only.drv`.
- Output: `/nix/store/s219r8labpnic76scwfz4y9w6p8a53gk-vm-test-run-canix-pg-online-backup-only`.
- Actual terminal:
  `/data/scratch/tmp/opencode/modde-atlas-3b29-bound-pg-online-backup-only-immutable-executor/terminal.json`
  (SHA-256 `ff27fce292c58b09e49605533bae733f508b09e2fa332050ac1bc90fe88ff05f`).
- Worker post-terminal verification:
  `/data/scratch/tmp/opencode/modde-3b29-online-terminal-verification-20261004T223136116229Z/receipt-v2.json`
  (SHA-256 `d08e7ea2ff6a53f3ec4a240bd400476286a7054dff0f249161e545eb3529333f`).

JUnit reports one test, zero failures/errors/skips. The successful complete
rendered script retains all 13 lifecycle assertions, including intentional
pre-adoption stops, INT/TERM interruption cleanup, receiver restoration, retained
partial/acknowledged-row preservation, `LAST_SUCCESS` and four receipt hashes.
The complete 1,207-line VM log and exact private publication/physical managed root
were retained and rechecked. Captured intentional-stop stderr is checked by the
script's assertions; the driver does not echo that captured text into its log.
The initial post-terminal log-substring assumption failure is preserved alongside
the corrected verification, as are the predecessor VM failure and admission-only
nonexecution. This is VM component acceptance, not production record acceptance,
adoption, activation or actual-game continuity.

### Changed-hook Nomad VM and independent acceptance

The user explicitly reassigned the sole changed-hook Nomad VM to the coordinator.
That exact selected graph also passed through one opaque managed realization,
one job/two cores, no source evaluation and no capacity override:

- Derivation: `/nix/store/5s94bikx9yickgh6vrk0n28ymlwc233v-vm-test-run-canix-managed-pg-nomad-recovery.drv`.
- Output: `/nix/store/5k26h6clpais856ajachcprgwl3jk00j-vm-test-run-canix-managed-pg-nomad-recovery`.
- Actual terminal:
  `/data/scratch/tmp/opencode/durability-3b29-coordinator-pg-recovery-nomad-only-executor/terminal.json`
  (SHA-256 `2e9c96bd085eee707be88a9df8b7ee3234357df5783891fad41cd04c7aad2db2`).
- Independent acceptance of both online and Nomad VM gates:
  `/data/scratch/tmp/opencode/durability-3b29-vm-terminal-independent-v3-20261004T224505764092Z/receipt.json`
  (SHA-256 `9d7c8a498c7109dacf0f49399d15c298d435d8d50874fa9ae00ab1691b4bd823`).

Both gates report JUnit one test, zero failures/errors/skips and exact private
`canix-fleet` publication/physical roots. Nomad's actual bound production hook
checks export v2: exactly 2,000 files and 35 directories, all sizes/hashes and no
extras. The unchanged 41 production SQL checks plus an additive saved-reviews
fixture produce 42 equal records across source, local and off-host restores.
Backup, epoch, system, metadata, manifest, snapshot, WAL and contract identities
are bound. Cancel, wrong-export, stale-live-check and corrupt-metadata rejection
paths reach successful whole-script completion. Verification-adapter failures
remain retained: verbose live/ANSI logs differ from JUnit's representation, and
the snapshot has no self-hash; its actual bytes/export and both restore hashes
provide the binding. No fixture/assertion changes or second VM attempt were used.

### Recovered six-stage literal provenance

The exact `3b29bb33…` top-level derivation closure selects all six Raven ETL unit
derivations. Read-only inspection recovered each unit's literal rendered text
from `structuredAttrs.text`, establishing `User=pink_raven`, literal
`PGUSER=pink_raven` and source revision
`a96a22ab6f554631e72068acdf22baab4bdbe622`. These fields are byte-equal to
historical `224…`, and every command context is represented by the selected
unit derivation inputs. Evidence:
`/data/scratch/tmp/opencode/durability-3b29-six-stage-literal-provenance-20261004T225802091632Z/receipt.json`
(SHA-256 `d73f0b6fa9a96e91a62345c2fb5af55f97cd0bacb05129b89294e6482d59ecc4`).

This closes the literal environment-projection evidence gap for `3b29…` without
another flake evaluation or realization. Complete stage commands and contexts
still differ: selected Canix lease-launcher output/derivation `wq42…`/`dsr3…`
replace historical `17z…`/`b2i9…`. Literal-field equality does not transfer
complete stage execution; the changed launcher and final composition still need
their own qualification.

### Remaining closeout and production gates

Scoped local qualification is green. Physical production Nomad restore, the last
acknowledged-row boundary, writer window and final cohort remain unadmitted.
Production ETL writer authority remains unassigned/unconfirmed; VM reassignment
does not schedule or fence production. Final committed composition, packaged CLI
and full runtime/stage qualification, writer window and activation remain pending.
In particular, selected Raven stage commands use different Canix lease-launcher
artifacts from the historical accepted stage. Historical component acceptance
does not establish full-stage context identity across that change.
The recovered literal fields above are accepted for `3b29…`; the final source
must bind its own stage fields, commands, CLI artifacts and contexts.

The coordinator must select an exact committed 40-character final Canix revision
and enumerate its production recovery contract. The accepted 41 production SQL
checks and additive VM fixture are specific to `3b29…`. Observed Forgejo source
edits do not qualify a possible 49-record final contract. Each added or changed
record needs an explicit owner, source-backed assertion and qualification of the
changed scope.

Evidence transfers artifact by artifact. Record one of three dispositions:

| Disposition | Required evidence |
| --- | --- |
| Identical and transferable | Measured derivation/output, NAR, relevant inputs/defaults and execution-context equality, with retained publication/root evidence. |
| Changed and requiring qualification | Explicit delta and admitted qualification of the changed package, launcher, hook, recovery contract or runtime behavior. |
| Pending | The exact missing evidence and its owner; no acceptance inferred from source similarity or a previous pass. |

A changed recovery hook or SQL contract cannot inherit the prior scope's VM
acceptance. Any required final-source attempt is separately coordinator-admitted;
the sole corrected online and changed-hook Nomad attempts for `3b29…` remain
closed green. Qualification uses normal bounded admission and retains failed,
cancelled and intermediate evidence.

Production admission must bind the exact final artifacts, confirmed writer owners
and protected workloads, UTC start/end/deadline, last-acknowledged boundary,
acceptance criteria and the failure/deadline recovery action. Physical restore
acceptance precedes adoption/activation; explicit writer release follows required
post-activation verification. Modde gameplay and measurements require their own
explicit runtime admission and cannot inherit an expired recovery window.

The coordinator's 2026-10-05 storage recheck also leaves an actual Atlas Btrfs
blocker unresolved. The original scrub reported 60 uncorrectable errors and zero
corrected. No newer matching journal errors were observed through 05:31 UTC,
but no fresh scrub/affected-extent verification or corruption-clearance proof
was obtained. Cumulative corruption counters do not establish post-scrub growth.
Evidence:
`/data/scratch/tmp/opencode/durability-atlas-btrfs-persistence-20261005T053121748122Z/receipt.json`
(SHA-256 `6b27f32a8eb2b7f33a02dac173dba02c7d2b0062098f21d4dc14de7863f6aab8`).
The coordinator/storage owner must provide accepted fresh storage verification
before production admission; successful historical VM gates do not clear it.

[PR #2](https://github.com/caniko/modde-rs/pull/2), exact head
`2cadcd139368620184fffd587df961684f90c60b`, remains open: its nine per-crate CI
workflows and release workflow are absent from approved PR #3. Digest/manager
fix coverage alone does not prove full supersession. Independent disposition:
`/data/scratch/tmp/opencode/durability-modde-pr2-closeout-independent-20261004T222654796699Z/receipt.json`
(SHA-256 `50c8d44f324a3b661cee9e156a12bf7dedabf25b3ec105e29654a82d458bcfa5`).
CI/release work needs an explicit preservation or retirement decision before closure.

PR #3 default-branch integration requires completed current-head substantive review
and acceptable CI through the guarded review/merge interfaces. The installed Canix
interfaces were unavailable at this snapshot; the Greptile credit-limit notice
does not provide substantive review. Local documentation commits require their
own qualification and do not replace approved producer `0d3ce666…` or inherit its
hosted results. Generated docs, active/history roots, runners, queries and failed
receipts remain retained while their closeout gates are pending.

The dated sections below retain earlier implementation and failure evidence.

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
| Nix regression gate | Approved producer `0d3ce666…` passed native package CI, including Nix regressions and packaged lifecycle. A replacement producer revision requires fresh exact-source qualification. |
| GUI-to-Play | Atlas hardware-rendering diagnostics pass outside/inside bubblewrap. Live GUI-to-game rendering, saved configuration and completion evidence remain required. |
| Steam and Heroic | Real exact-install hooks, launcher/cloud-sync setup, native and Wine/Proton/UMU commands, first-run prefix behavior and save continuity. |
| Manager | Live manager forwarding, root lease/marker lifetime and recovery after interruption. |
| Native nested runtimes | Verify supervision, mounts and failure without an unsandboxed fallback for actual Proton/UMU commands. |
| Performance | Release/package runs and retained CSV/configuration/process evidence. Results are report-only. No real-game overhead has been measured. |
| Dependency health | Current `cryoglyph`/Iced dependency on `lru 0.16.4` still carries RUSTSEC-2026-0253. Other unmaintained/yanked warnings also remain. |
| Whole-repository documentation | Badge destinations and Simit policy are repaired. Closeout docs must retain exact-source claims and pass current link/command/mdBook validation. |
| Canix integration rollout | Historical consumer `8240764b…` binds the approved producer, actual wrapper, logging/database/manager defaults and packaged lifecycle. Selected successor `3b29bb33…` needs coordinator-owned binding and affected checks before final composition, recovery/adoption and activation. |

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
