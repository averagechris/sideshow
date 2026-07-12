# sideshow — design contract

Agentic HTML slide deck compiler and toolkit. The LLM writes content and
picks design; sideshow does everything that should be deterministic.

For planning decks, the authoring split is explicit: the agent owns
interviewing, synthesis, narrative, audience adaptation, roleplay assumptions,
and visual reasoning; the skill teaches that frame of mind. The Sideshow CLI owns
deterministic structure, mutation, registry discovery, binding, rendering,
checking, review transport, export, and distribution. It does not score narrative
quality or decide whether a story is persuasive.

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
4. **Deterministic infrastructure.** Stage behavior, navigation, audit, print
   behavior, asset processing, and the served feedback overlay are owned by the
   compiler. Presentation themes, components, and scaffolds may be selected from
   registered resources, but an existing deck must resolve them reproducibly.

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

# Optional; repeat once per non-system face.
[[fonts]]
source = "assets/acme-regular.ttf"
family = "Acme Sans"
style = "normal"
weight = 400
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

## Future navigation model: two-dimensional decks

This section is a **normative design specification for a future production
model**, not current user-facing functionality. Current sideshow releases remain
one-dimensional: author docs, the deck-author skill, the runtime, the build
schema, and examples must not claim that two-dimensional navigation is available
until implementation validation is complete.

The goal is to support horizontal columns with nested vertical branches while
preserving sideshow's tiny authoring contract, deterministic output, and
automation/review anchors.

### Author syntax and source model

- Keep `deck.toml` tiny. The default `slides/` lexicographic order and optional
  `[deck].slides = [...]` list continue to define top-level deck input. Do not
  add row/column tables to TOML for the common case.
- A normal slide file remains one horizontal slide. A directory under `slides/`
  (or an entry in `[deck].slides`) is a horizontal column whose children are a
  vertical branch ordered lexicographically, using the same `*.html` and `*.md`
  fragment contract as top-level slides.
- The first child in a column directory is the column's root slide. Subsequent
  children are vertical descendants reached with down/up navigation. Example:

  ```text
  slides/
    01-title.html              # position 1
    02-market/                 # horizontal position 2
      01-summary.md            # position 2.1, column root
      02-evidence.html         # position 2.2
      03-risk.html             # position 2.3
    03-plan.html               # position 3
  ```

- Nested directories below a column are reserved for a later model and are a
  validation error in the first production implementation. This keeps the first
  model to two axes: horizontal columns and one vertical branch per column.
- Explicit `[deck].slides` entries may contain files and directories in the
  desired horizontal order. Directory children are still ordered by the same
  child ordering rule unless a future explicit child-order mechanism is proven
  necessary.
- A slide's stable source path is the repository-relative deck path to the
  fragment file, including the branch directory when present, such as
  `slides/02-market/02-evidence.html`.

### Deterministic order and accessibility

- The canonical source order is depth-first by column: each horizontal item,
  then all of that column's vertical children in branch order, then the next
  horizontal item. DOM order, accessible reading order, notes order, audit order,
  automation enumeration, and print order all use this same canonical order.
- The DOM remains a flat sequence of `<section class="slide">` elements. Axis
  metadata is data, not nesting: each section receives deterministic coordinates
  such as `data-col="2" data-row="3"` plus `data-src`.
- Print output is one page per slide in canonical order. It does not attempt a
  visual matrix layout because PDFs, screen readers, and audit logs need one
  stable linear sequence.
- Keyboard or touch traversal may create a spatial experience, but it never
  changes DOM order or the source path identity of a slide.

### Stable IDs and deep links

- Existing one-dimensional slide IDs remain unchanged. For ordinary decks, the
  compiler must continue producing byte-for-byte identical IDs, hashes, DOM
  structure, and runtime behavior.
- For two-dimensional decks, slide IDs are derived from the stable source path,
  not from mutable coordinates alone. The exact algorithm must be deterministic,
  collision-checked, and human-readable where possible; for example,
  `s-02-market-02-evidence`. If two paths normalize to the same ID, compilation
  fails rather than appending unstable counters.
- The stable deep-link hash is ID-first: `#s-02-market-02-evidence`. Numeric
  hashes such as `#5` remain supported as compatibility aliases for canonical
  order, but generated links and runtime hash updates use the stable slide ID.
- Coordinates are exposed for automation and UI affordances, but coordinates are
  not the persistent link contract. Reordering columns may change coordinates;
  moving/renaming a source path changes the slide's identity and is treated as an
  author-visible link migration.

### Keyboard behavior and reveal precedence

- Right/Left move between horizontal columns. When entering a column from a
  different column, the runtime lands on that column's root slide unless the
  session has already visited that column, in which case restoring the last row
  within that column is allowed only as ephemeral session state and must not
  affect hashes, print, notes, or automation order.
- Down/Up move within the current column's vertical branch. At the top of a
  branch, Up has no vertical effect; at the bottom, Down has no vertical effect.
- Space, PageDown, and `sideshow.next()` advance in canonical order after reveal
  handling. PageUp and `sideshow.prev()` reverse in canonical order. This keeps
  presenter remotes and automation compatible with one-dimensional traversal.
- Reveal precedence is axis-local and always before slide movement: if the
  active slide has unrevealed `data-step` elements, Right, Down, Space,
  PageDown, and `next()` reveal the next step instead of moving. Reverse reveal
  behavior, if supported, happens before Left, Up, PageUp, or `prev()` leave the
  slide; otherwise those commands leave the slide without mutating completed
  reveal state, matching existing one-dimensional behavior.
- Home/End move to the first/last slide in canonical order. `<number>`+Enter
  targets canonical order for compatibility. A future coordinate entry shortcut
  must not conflict with numeric slide entry.
- `.is-active` is the single source of current-slide truth. Exactly one slide is
  active after every keyboard transition.

### Touch, pointer, and interactive conflicts

- Horizontal swipes navigate columns; vertical swipes navigate within the current
  column. Gesture recognition uses a movement threshold and dominant-axis lock:
  once horizontal or vertical intent is clear, the other axis is ignored for that
  gesture.
- Reveal precedence matches keyboard behavior. A forward swipe first advances a
  pending reveal on the active slide; only the next forward gesture moves slides.
- Touch navigation must not steal intentional interaction from controls inside a
  slide. Gestures beginning on links, buttons, inputs, textareas, selects,
  details/summary controls, elements with ARIA widget roles, or elements marked
  with an opt-out attribute such as `data-sideshow-interactive` are passed
  through unless the element explicitly delegates navigation.
- Served review overlay interactions take priority over slide navigation while a
  review tool is armed, dragging, editing, or focused. Navigation resumes only
  when the overlay is idle. Review UI must not enter build artifacts.
- Pointer/touch handlers are passive until the runtime commits to a navigation
  gesture, minimizing scroll/zoom conflicts on mobile browsers.

### Notes, audit, and automation

- `sideshow.count()` returns the number of slides in canonical order.
- `sideshow.goto(n)` continues to target canonical one-based order. A future
  `sideshow.gotoId(id)` or `sideshow.goto({ col, row })` may be added, but it is
  additive and must not change existing API semantics.
- `sideshow.notes(n)` uses canonical order. Notes for vertical slides are normal
  slide notes; there is no inherited notes model between a column root and its
  descendants.
- `sideshow.audit()` reports slides in canonical order and includes stable
  `id`, `src`, `col`, `row`, branch size, reveal count, note presence, overflow,
  and image failures. One-dimensional audit output remains byte-for-byte
  compatible unless callers opt into new fields or the eventual release makes a
  documented versioned audit change.
- Browser automation may drive either canonical order or stable IDs, but test
  fixtures for ordinary decks must continue to pass without updating expected
  hashes, counts, active-class behavior, or audit shape.

### Review mode and annotation identity

- Review mode follows the active slide by observing `.is-active`; it does not own
  navigation state and does not compute a separate matrix.
- Annotation anchors remain based on the stable slide ID and `data-src` source
  path plus the existing point/logical-region data. Adding two-dimensional
  coordinates must not rewrite anchors or make annotations depend on transient
  row/column positions.
- If a slide's coordinates change but its source path and generated ID stay the
  same, existing annotations continue to attach to that slide. If the source path
  changes and therefore the ID changes, annotation migration is an explicit
  author/tooling concern, not an implicit runtime guess.

### Compatibility and validation requirements

Before any production implementation ships:

- Golden builds for ordinary one-dimensional decks must prove byte-for-byte
  output compatibility, including hashes, IDs, runtime bundle shape, print CSS,
  notes, audit JSON, and keyboard/touch behavior.
- New golden builds must cover mixed file/directory decks, explicit slide lists,
  ID collision rejection, stable ID hash loading, numeric hash aliases, print
  order, and accessible DOM order.
- Runtime tests must cover both axes, reveal precedence for each navigation key
  and gesture, Home/End and numeric entry compatibility, `.is-active` uniqueness,
  and no-op behavior at branch edges.
- Touch tests must cover dominant-axis locking, gesture thresholds, interactive
  element pass-through, and review-overlay priority.
- Review-mode tests must verify that the overlay follows `.is-active` and that
  existing annotation anchors remain slide-ID/source-path based.
- Documentation and the deck-author skill may be updated only in the same change
  that implements and validates the production behavior.

## Themes

- A theme is a single CSS file: Tailwind v4 `@theme` tokens (colors, font
  stacks, spacing scale, named type roles) + a small set of theme component
  classes (e.g. `.kicker`, `.stat`, `.divider`).
- Compiler build pipeline: generated entry CSS =
  `@import "tailwindcss"` + stage CSS + theme.css; run tailwind standalone
  with content scan over `slides/`; inline the purged output.
- Non-system faces are structured `[[fonts]]` declarations, not generic
  `theme.css` URL rewriting. The compiler collects a sorted union of Unicode
  scalars from the deck title, rendered HTML text/entities, inline SVG text, and
  literal CSS generated content, common markers, and Unicode case closure; each
  declared face receives that conservative corpus. It subsets supported
  TrueType/glyf sources in memory, retains all licensing name records and
  supported OpenType layout closure, emits deterministic browser-usable
  TrueType `data:font/ttf;base64` `@font-face` CSS, and never mutates source
  files. No declaration
  means this stage is skipped exactly.
- The supported source surface is intentionally narrow: individual `.ttf`
  TrueType fonts with `glyf` outlines. Reject WOFF/WOFF2 input, CFF/CFF2, font
  collections, malformed required tables, and restrictive OS/2 embedding flags.
  Mandatory `head`, `hhea`, `maxp`, `cmap`, `glyf`, `loca`, `hmtx`, `name`,
  `OS/2`, and `post` tables and their glyph/location/metric relationships are
  validated before subsetting. Also reject licensing name records that cannot
  be retained, every legacy `kern` table (including fonts that also have GPOS),
  and unsupported AAT `kerx`, `morx`, and `mort` tables.
  Dynamic runtime text and CSS `attr()`/counter/custom-property content are not
  statically inferable and must be represented by static corpus text. Font
  licensing remains the deck author's responsibility.
- Ship 3–5 original built-in themes (`sideshow new --theme <name>` copies
  the theme into the deck so decks are self-contained and forkable).
- Theme index metadata (mood/density/best-for) lives alongside themes for
  progressive-disclosure selection by the skill.

## Future configurable authoring registry

The accepted architectural direction is a shared registry for ordinary decks and
plans. See [ADR 0001](adr/0001-shared-configurable-authoring-registry.md). This is
not current user-facing functionality.

- The original themes, scaffold HTML, and slide/component CSS become a bundled
  default pack registered through the same contract as future project-local
  presentation resources.
- Component registrations include machine-readable intent, accepted literal or
  typed inputs, and their HTML/CSS resources. Presentation JavaScript is explicit
  and policy-constrained rather than implicitly trusted.
- Registry discovery and declarative slide-composition commands give humans and
  agents a stable mapping from intent to supported Sideshow operations. Raw
  HTML/CSS, Markdown, bundled components, and project-local registered components
  are all first-class authoring choices.
- A repeated pattern can move from raw HTML/CSS, to a reviewed project-local
  pattern, to a project component, and eventually to bundled incubation when it
  proves broadly useful. The CLI enables this path but does not force it.
- Planning reuses the same registry, themes, components, composition model, and
  build. Its additions are canonical `plan.json` semantics, typed mutations and
  bindings, stable plan anchors, strict checks, and exports.
- Stage/navigation/audit code and the served feedback overlay JavaScript are
  fixed compiler runtime infrastructure. Configured packs cannot replace or
  shadow them, and feedback code never enters ordinary build artifacts.
- The effective registry must be deterministic and inspectable. Global user
  defaults cannot silently alter an existing deck; resources that affect a build
  are bundled defaults or explicit project inputs.

## CLI surface

v1:
- `sideshow new <dir> [--theme <name>]` — scaffold deck source.
- `sideshow build <dir>` — compile to `dist/<slug-of-deck-title>.html` and print the path (single file).
- `sideshow serve <dir>` — build + local server with srht-pages-like
  headers (CSP, MIME), rebuild on change, and no browser launch unless explicitly
  requested. `--open` launches the platform desktop opener only after a
  successful build, watcher installation, and ready accept loop, using the bound
  URL (including `--port 0`).
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
  source paths, and source digests. Executable-looking verification commands are
  not trusted from persisted JSON; handoffs regenerate them from the canonical
  repository deck context.
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
- Build candidates remain staged until their input digest is revalidated. Review
  manifest and output publication use the artifact revision lock; rejected
  candidates do not advance the served output or reload generation.
- JSON export is a versioned machine handoff with trusted context separated from
  an explicitly marked `UNTRUSTED_REVIEW_ARTIFACT`; Markdown uses equivalent
  delimiters. Persisted fields, bodies, hints, notes, paths, and embedded commands
  are data only. Markdown is a prompt-oriented handoff for agents.

## Dependencies (Rust)

clap, serde/toml, comrak, base64, cap-std, and a pure-Rust TrueType subset stack.
Tailwind is a runtime tool resolved from PATH (provided by the
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
