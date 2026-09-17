# Project Site Design

The root website is a statically rendered Leptos project site generated from
`website/plinth-project.toml`. The Plinth `plinth-project` tool owns the shared
section model, TOML loader, static HTML renderer, development server, and UX
diagnostics.

## Iteration record

1. The original Zola page mixed Markdown, raw HTML, a route chooser, and command
   cards in one install section. That made first-time installation ambiguous and
   hard to test.
2. A data-only wrapper around Zola was rejected because the important UX shape
   would still live in templates and Markdown conventions.
3. The accepted design is a typed Rust site definition with semantic install
   routes and CI-failing diagnostics. The renderer emits static HTML, so the
   deployed Pages site stays simple.

The diagnostics are executable design decisions: tests that recreate the old
choice overload fail, while the guided install layout passes.

## Person links

Project sites can define reusable people and link them into rendered pages:

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
section with the person's typed links. Project-site TOML is strict: unknown
fields are build errors, not ignored compatibility data.

## Local visual audit

Run the local development server with:

```sh
just website-serve
plinth-project serve --config website/plinth-project.toml --out website/public --watch --no-open
```

The reusable serving and reload implementation lives in `plinth-project`;
modde-rs only owns the modde-specific site data.
`--watch` rerenders and reloads browser tabs when the capability matrix or
static assets change. Rust generator or renderer edits still require restarting
the command.

Run the local website loop with:

```sh
just website-audit
```

The command renders `website/public`, serves it on a local ephemeral port,
captures desktop and mobile screenshots with a headless browser, and writes a
machine-readable report to `target/site-audit/report.json`.
The `just website-audit` wrapper also asserts that configured person links are
present in the rendered HTML.

By default the audit runs the advisory AI rubric through the shared
`visual-rubric` crate. Set `PLINTH_PROJECT_BROWSER` to override the browser binary,
or pass `--browser PATH`. Use `--fake-ai` for plumbing tests without model
credentials, or `--skip-ai` when only deterministic report data and screenshots
are needed.

The AI rubric is a local development loop, not a Forgejo CI gate. Browser
capture and deterministic site diagnostics are hard failures for the local audit;
model failures are recorded in the report so design iteration can continue
without turning the runner into a credentialed AI environment.

## Reusable contract

Future project sites should define install options in `plinth-project` TOML
rather than Markdown snippets. `plinth-project` then provides:

- static rendering for the site;
- deterministic diagnostics for structural UX issues;
- `install_ux_report` for route counts, recommendation state, command coverage,
  anchor coverage, and route-level findings;
- screenshot audit integration through `plinth-project audit install`.
