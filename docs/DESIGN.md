# sideshow — design contract

Agentic HTML slide deck compiler and toolkit. The LLM writes content and
picks design; sideshow does everything that should be deterministic.

Inspired by (clean-room, no vendored code) zarazhangrui/frontend-slides:
fixed-stage scaling, single-file output, show-don't-tell previews,
progressive-disclosure theme selection.

## Invariants

1. **Output is a single self-contained HTML file.** No sidecar assets, no
   runtime network fetches required to present. This is the distribution
   contract (srht pages, S3 presigned, Slack attachment all Just Work).
2. **Terminal-first.** Authoring and source mutation stay in the terminal. The visual feedback loop is the
   *agent's* job, not the CLI's: drive rodney (or any browser automation)
   against the built deck, screenshot slides, read them with vision. The
   deck cooperates via the `window.sideshow` runtime API (nav + audit);
   the skill pack documents the canonical rodney workflow. An optional served
   review overlay may capture visual annotations for handoff, but it never edits
   deck source. sideshow never shells out to a browser.
3. **Minimal node.** Tailwind v4 standalone binary (nixpkgs `tailwindcss_4`)
   is the only JS-ecosystem tool, provided via nix, never npm/npx.
4. **Deterministic boilerplate.** Stage CSS, nav runtime, print CSS, theme
   tokens are emitted by the compiler, never authored by the agent.

## Deck source layout

```
mydeck/
  deck.toml            # metadata + config
  theme.css            # design tokens (@theme) + theme component CSS
  slides/
    01-title.html      # per-slide fragments (bespoke layout slides)
    02-agenda.md       # markdown slides (uniform content slides, comrak)
    03-diagram.html
  assets/              # presented assets only, inlined at build
```

### deck.toml

```toml
[deck]
title = "Q3 Platform Review"
theme = "signal"            # informational; theme.css is the source of truth
# slides = [...]            # optional explicit order; default: lexicographic
                            # sort of slides/*.{html,md}

[build]
# inline_assets = true      # default; base64 everything into the output
```

Keep the schema tiny. Add keys only when a real need appears.

## Slide fragment contract (the DOM contract)

- An `.html` fragment file contains the **inner content** of one slide.
  The compiler wraps it: `<section class="slide" id="s-<stem>" data-src="<file>">`.
- Fragments are plain HTML + Tailwind utilities + theme token vars. No
  `<html>`, `<head>`, `<script>` in fragments (compiler rejects; `check`
  enforces).
- **Speaker notes:** optional `<template data-notes>…</template>` inside a
  fragment. Never rendered on stage; exposed through
  `window.sideshow.notes(n)` for the browser-driven agent workflow.
- **Incremental reveals:** elements with `data-step` (or `data-step="2"` for
  explicit ordering) are revealed stepwise by the runtime before advancing
  to the next slide.
- `.md` fragments render via comrak into
  `<section class="slide slide-md">…</section>` and are styled by theme
  typography defaults. Markdown is for content-shaped slides; anything
  bespoke should be an HTML fragment.
- Local assets are confined to `assets/` and normally become data URIs. Direct
  SVG `<img>` sources may become same-document markup only when strict XML and
  static-content checks succeed; IDs are namespaced across the full deck and
  unsupported SVGs retain passive image semantics.

## Stage model

- Fixed logical canvas **1920x1080**, uniformly scaled via
  `transform: scale()` to fit the viewport, letterboxed, never reflowed.
- Slide switching via `visibility`/`opacity`/`pointer-events` (not
  `display:none`) so per-slide layout classes behave.
- `@media print`: one slide per page at full canvas, animations disabled —
  native print is the zero-dependency PDF fallback.
- `prefers-reduced-motion` respected.

## Runtime (inlined, vanilla JS, target < ~250 lines)

- Keyboard: arrows/space/PageUp/PageDown, Home/End, `<number>`+Enter.
- URL hash deep links (`#5`) kept in sync.
- Step reveal state machine for `data-step`.
- Emits nothing, requires nothing. No web components, no framework.
- **`window.sideshow` API** — the automation contract for agents driving
  the deck via rodney:
  - `sideshow.goto(n)` / `sideshow.next()` / `sideshow.prev()`
  - `sideshow.count()` — slide count
  - `sideshow.notes(n)` — speaker notes text for a slide
  - `sideshow.audit()` — JSON report: per-slide content overflow vs the
    1920x1080 canvas, step counts, note presence, image load failures.
  Deterministic measurement lives in the deck; the browser driver stays
  dumb.

## Themes

- A theme is a single CSS file: Tailwind v4 `@theme` tokens (colors, font
  stacks, spacing scale, named type roles) + a small set of theme component
  classes (e.g. `.kicker`, `.stat`, `.divider`).
- Compiler build pipeline: generated entry CSS =
  `@import "tailwindcss"` + stage CSS + theme.css; run tailwind standalone
  with content scan over `slides/`; inline the purged output.
- Ship 3–5 original built-in themes (`sideshow new --theme <name>` copies
  the theme into the deck so decks are self-contained and forkable).
- Theme index metadata (mood/density/best-for) lives alongside themes for
  progressive-disclosure selection by the skill.

## CLI surface

v1:
- `sideshow new <dir> [--theme <name>]` — scaffold deck source.
- `sideshow build <dir>` — compile to `dist/<slug-of-deck-title>.html` and print the path (single file).
- `sideshow serve <dir>` — build + local server with srht-pages-like
  headers (CSP, MIME), rebuild on change, and no browser launch unless explicitly
  requested. `--open` launches the platform desktop opener only after a
  successful build and listener bind, using the bound URL (including `--port 0`).
  `--review` injects local-only point and logical-region annotation controls into
  served HTML; build output remains unchanged.
- `sideshow review artifact <dir>` / `list` / `export [--format json|markdown]
  [--output <path>]` / `resolve <dir> <id> --revision <n>` / `reopen <dir> <id>
  --revision <n>` / `disposition <dir> <id> --status
  pending|addressed|wont-fix|deferred [--note <text>] --revision <n>` / `clear
  <dir> --revision <n> --yes` — inspect and manage persistent review artifacts.
- `sideshow check <dir>` — static deck linter: fragment contract
  violations, broken asset refs, deck.toml validity, duplicate slide ids.
  Dynamic checks (overflow, image rendering) are `sideshow.audit()` via
  the skill's rodney workflow, not the CLI.
- `sideshow publish <dir> --target {s3,srht}` — publish an existing dist
  output via S3 upload + presigned URL or direct pages.sr.ht REST publish with a
  subdir-scoped tarball upload, no external publisher dependency.

fast-follow:
- PDF export: native `@media print` is the v1 answer (print from the
  agent-driven browser or by hand); a dedicated exporter only if fidelity
  demands it.

Screenshots/visual QA are deliberately **not** CLI subcommands — see
invariant 2.

## Review artifact contract

- Review state is a tool-neutral JSON artifact, currently `schema_version: 2`,
  stored outside deck source under `$XDG_STATE_HOME/sideshow/reviews/` or
  `$HOME/.local/state/sideshow/reviews/` when `XDG_STATE_HOME` is unset.
- Artifacts are keyed by the lowercase SHA-256 of the canonical absolute deck
  root: `<root-key>.json`; process-safe writes use a sibling `<root-key>.lock`,
  a synced temporary file, atomic rename, and directory sync.
- The artifact records deck identity, revision, optional build manifest, and
  annotations. Build manifests carry `build_id`, `built_at_ms`, slide IDs,
  source paths, source digests, and verification commands.
- Rebuilds update freshness only. Annotations are not dropped when slides change:
  `freshness` (`current`, `stale`, `orphaned`) is derived from the latest build
  manifest and remains orthogonal to workflow `state` (`todo`, `resolved`) and
  explicit `disposition` (`pending`, `addressed`, `wont_fix`, `deferred`). Served
  schema v2 clients treat that freshness as authoritative, using DOM identity
  only when the value is missing or invalid, and display non-pending dispositions
  with their optional notes.
- HTTP review mutations are JSON and revision-guarded. GET/HEAD return a quoted
  revision ETag; POST requires same-origin, the review nonce, `application/json`,
  and `If-Match` equal to the mutation `revision`. Conflicts return the latest
  snapshot for reload/retry. Successful CLI and HTTP mutations return the
  refreshed artifact; its new revision must guard the next mutation.
- `sideshow build` never emits review UI, nonce, annotations, or artifact paths.
  Review exports also refuse output paths inside the deck.
- JSON export is canonical for machines; Markdown export is a prompt-oriented
  handoff for agents.

## Dependencies (Rust)

clap, serde/toml, comrak, tiny HTTP server (repo-local or `tiny_http`),
base64. Tailwind is a runtime tool resolved from PATH (provided by the
flake devShell / package wrapper), shelled out to — not linked. rodney is
**not** a dependency: browser automation belongs to the agent workflow.

## Explicit non-goals

- No WASM/Leptos runtime in decks (revisit only for a future interactive
  slide type).
- No markdown-only authoring mode; markdown is a per-slide convenience.
- No browser-based source editor or inline WYSIWYG editing. Served review mode
  is annotation and handoff only.
- No reveal.js — but the runtime is intentionally small enough that
  swapping a different runtime in later is a build-step change, not a
  rewrite.
