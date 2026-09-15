# Contributing

Development setup and broad contribution guidance live in the repository
[CONTRIBUTING.md](https://github.com/caniko/rs-modde/blob/trunk/CONTRIBUTING.md).
This page records contributor-facing implementation notes that should stay
visible in the rendered docs.

## Crash-log correlation follow-ups

The crash-log correlation implementation is intentionally conservative: it parses
Crash Logger SSE, Trainwreck, and generic logs, then correlates mentioned
plugins, DLLs, assets, and form IDs with modde's local install database. The
public framing must stay narrow: existing analyzers and MO2/Vortex integrations
already exist, while modde's edge is correlating crash evidence with the managed
profile, installed files, plugin order, versions, Nexus metadata, and experiment
state.

The parser section names were checked against upstream source instead of
invented fixtures:

- Crash Logger SSE: <https://github.com/alandtse/CrashLoggerSSE>
- Phostwood analyzer: <https://github.com/Phostwood/crash-analyzer>

The remaining missing pieces are:

| Missing piece | Why it is required | Upstream/source to provide it | Validation command |
| ------------- | ------------------ | ----------------------------- | ------------------ |
| Sanitized real Crash Logger SSE fixture logs | Parser behavior should be locked against real log shape, not synthetic examples | A contributor with permission to share redacted Crash Logger SSE logs | `nix develop . -c cargo test -p modde-core crash_logger` |
| Sanitized real Trainwreck fixture logs | Trainwreck-specific parsing needs real-world coverage before broad compatibility claims | A contributor with permission to share redacted Trainwreck logs | `nix develop . -c cargo test -p modde-core trainwreck` |
| CLI text and JSON snapshot tests for `modde doctor crash` | The command output is user-facing and should not drift accidentally | Add fixtures under the CLI test fixture tree once real logs are available | `nix develop . -c cargo test -p modde crash` |
| Correlation matrix tests | Plugin, DLL, asset, no-match, disabled-mod, and inactive-profile cases need regression coverage | Local test databases built from sanitized fixture manifests | `nix develop . -c cargo test -p modde-core crash::` |
| UI diagnostics crash-log tests | Import state and rendered suspect rows need coverage in the Iced diagnostics view | UI simulator tests using sanitized local fixture paths | `nix develop . -c cargo test -p modde-ui diagnostics` |
| Full workspace verification after CLI async recursion is fixed | Current full CLI/workspace checks are blocked by unrelated deploy/patcher async recursion | Fix the recursion between `crates/modde-cli/src/commands/deploy.rs` and `crates/modde-cli/src/commands/patcher.rs` | `nix develop . -c cargo test --workspace` |

Do not replace the fixture requirement with generated crash logs. If a fixture is
missing, report the exact fixture set needed and the validation command above.
