# Project Site Design

Plinth's `plinth-project` generates the static Leptos site at the root from
`website/plinth-project.toml`. It supplies the shared section model, TOML loader,
static HTML renderer, development server, and UX diagnostics.

## Iteration record

1. The original Zola page mixed Markdown, raw HTML, a route chooser, and command
   cards in one install section, making first-time installation ambiguous and
   hard to test.
2. A data-only Zola wrapper would still leave the key UX decisions in templates
   and Markdown conventions, so it was rejected.
3. The accepted design is a typed Rust site definition with semantic install
   routes and CI-failing diagnostics. The renderer emits static HTML, so the
   deployed Pages site stays simple.

Tests fail when a layout recreates the old choice overload; the guided install
layout passes.

## Person links

Define a person once and reference them across rendered pages:

```toml
[site]
primary_person = "caniko"

[[people]]
id = "caniko"
name = "Caniko"
url = "https://codeberg.org/caniko"
role = "Maintainer"

[[people.links]]
label = "Codeberg"
href = "https://codeberg.org/caniko"
kind = "person"

[[pages.sections]]
type = "hero"
title = "modde"
tagline = "A cross-platform game mod manager"
subtitle = "Install Wabbajack modlists, Nexus Collections, and individual mods."
person = "caniko"

[[pages.sections]]
type = "person_mention"
id = "maintainer"
heading = "Maintainer"
intro = "modde is maintained as an open-source project by Caniko."
person = "caniko"
```

`site.primary_person` renders footer attribution and JSON-LD author metadata.
Hero `person` renders a byline. `person_mention` renders an about/maintainer
section with the person's typed links. Unknown fields in project-site TOML fail
the build.

## Local visual audit

Run the local development server with:

```sh
just website-serve
plinth-project serve --config website/plinth-project.toml --out website/public --watch --no-open
```

`plinth-project` serves and reloads the site; modde-rs supplies only site data.
`--watch` rerenders and reloads browser tabs when the capability matrix or
static assets change. Rust generator or renderer edits still require restarting
the command.

Audit the site locally:

```sh
just website-audit
```

The audit renders `website/public`, serves it on a local ephemeral port, takes
desktop and mobile screenshots in a headless browser, and writes a
machine-readable report to `target/site-audit/report.json`. `just website-audit`
fails if a configured person link is missing from the rendered HTML.

By default the audit runs the advisory AI rubric through the shared
`visual-rubric` crate. Set `PLINTH_PROJECT_BROWSER` to override the browser binary,
or pass `--browser PATH`. Use `--fake-ai` for plumbing tests without model
credentials, or `--skip-ai` when only deterministic report data and screenshots
are needed.

The AI rubric runs locally, not as a Forgejo CI gate. Browser-capture failures
and deterministic diagnostic failures fail the local audit. Model failures are
recorded in the report and do not fail the audit, so design work can continue
without model credentials on the runner.

## Reusable contract

Future project sites should define install options in `plinth-project` TOML
rather than Markdown snippets. `plinth-project` then provides:

- static rendering for the site;
- deterministic diagnostics for structural UX issues;
- `install_ux_report` for route counts, recommendation state, command coverage,
  anchor coverage, and route-level findings;
- screenshot audit integration through `plinth-project audit install`.
