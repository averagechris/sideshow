# sideshow

Agentic HTML slide deck compiler and toolkit

## Usage

Create a deck source directory, edit the fragments, then build one
self-contained HTML file:

```sh
sideshow new mydeck --theme signal
$EDITOR mydeck/slides/01-title.html mydeck/slides/02-content.md
sideshow build mydeck
open <path printed by sideshow build>
```

`sideshow build` reads `deck.toml`, wraps `slides/*.html` and `slides/*.md` as
fixed 1920×1080 slide sections, inlines local `assets/` references, and embeds
the stage CSS/runtime JavaScript. Assets normally become data URIs. A direct
`<img src="assets/diagram.svg">` becomes namespaced inline markup only when it
passes Sideshow's conservative static-SVG checks; unsupported SVGs stay passive
data-URI images. Tailwind CSS v4's standalone `tailwindcss` binary must be
available; packaged installs bundle it.

## Planning mode

Planning mode creates an alignment artifact for framing an idea, exploring
alternatives, optionally prototyping uncertain parts, collecting feedback, and
refining a proposal before execution moves into the team's issue tracker:

```sh
sideshow plan new my-plan --theme signal
sideshow plan check my-plan --strict
sideshow plan serve my-plan --open
sideshow plan export my-plan --format markdown --output /tmp/my-plan.md
```

`plan.json` is normalized strict schema-v2, tracker-neutral planning data and the
machine boundary for agents/tools; authored slides are its expressive
human-facing projection. Review annotations are feedback only and never silently
edit either source.

The planning-authoring model keeps three goals co-equal: human-agent alignment,
convenient anchored feedback, and team-wide mental-model distribution. Agents own
interviewing, synthesis, narrative, audience adaptation, roleplay assumptions,
and visual reasoning. Sideshow owns deterministic structure, mutation, registry
discovery, binding, rendering, checking, review transport, export, and
distribution; it does not judge narrative quality.

The workflow intentionally stops at a reviewed digest. JSON export preserves the
strict machine contract; Markdown export is a derived manual drafting digest with
per-task packets for issue-writing source material. Those packets are not
guaranteed one-to-one issues: a human or agent chooses split/combine boundaries,
labels, teams, priority, milestones, and tracker conventions while drafting in
Linear, todo.sr.ht, GitHub Issues, or another organizational system. Sideshow
does not authenticate to trackers, import tracker state, create issues, or
synchronize execution. Exact `verification.commands` remain trusted/verbatim
executable material; review annotations remain untrusted and excluded. Once
handed off, the tracker owns assignment, priority, implementation status,
blockers, and completion.

A later demo, retrospective, project summary, or “how we built it” presentation
is a separate ordinary Sideshow deck. Authors and agents can manually synthesize
the original plan, tracker and code context, screenshots, and lessons using the
same static authoring, review, evidence, and visual components; no ingestion or
tracker integration is required from Sideshow itself.

## External tools

Sideshow resolves external tools in this order: environment override, user config,
then `PATH`. Set `SIDESHOW_TAILWINDCSS`, `SIDESHOW_FFMPEG`, `SIDESHOW_VHS`, or
`SIDESHOW_AWS` to an executable file path for the highest-priority override. The user config file is
loaded from `$SIDESHOW_CONFIG`, `$XDG_CONFIG_HOME/sideshow/config.toml`, or
`$HOME/.config/sideshow/config.toml` (first match; missing files mean defaults):

```toml
[tools]
tailwindcss = "/path/to/tailwindcss"
ffmpeg = "/path/to/ffmpeg"
vhs = "/path/to/vhs"
aws = "/path/to/aws"
```

Lint before building:

```sh
sideshow check mydeck
sideshow check mydeck --format json
```

`check` validates `deck.toml`, slide ordering, forbidden fragment tags, duplicate
generated slide IDs, missing `assets/...` references, empty slide directories,
and asset size budgets. It also warns about unreferenced files under `assets/`
and hardcoded colors in fragment CSS and SVG presentation attributes. Warnings
are advisory by default; use `--strict` when a deck intentionally treats them as
release gates.

Publish an already-built deck without rebuilding:

```sh
sideshow publish mydeck --target s3 --bucket my-bucket --key demos/mydeck.html --expires 3600
sideshow publish mydeck --target srht --domain user.srht.site
```

S3 publishing shells out to the AWS CLI for `aws s3 cp` and `aws s3 presign`.
Set `SIDESHOW_AWS=/path/to/aws` or add `aws = "/path/to/aws"` under `[tools]`
in the sideshow config when it is not on `PATH`.

SourceHut publishing uploads directly to the Pages API. Set `SRHT_TOKEN` to a
personal access token from <https://meta.sr.ht/oauth2> with scope
`pages.sr.ht/PAGES:RW`, or configure a command that prints one:

```toml
[srht]
token-cmd = ["pass", "show", "srht"]
```

By default the deck is published under the dist file stem, so the example above
serves at `https://user.srht.site/<slug>/`. Override that with
`--subdir decks/foo` for nested paths. Only that subdirectory is updated; the
rest of the site is left untouched. Use `--pages-url <url>` for self-hosted
SourceHut Pages instances or tests.

## Images

Use `sideshow img` to inspect and prepare raster assets before committing them to
a deck:

```sh
sideshow img info mydeck/assets/photo.png
sideshow img info mydeck/assets/photo.png --format json
sideshow img resize mydeck/assets/photo.png --width 1920
sideshow img crop mydeck/assets/photo.png --rect 1600x900      # centered
sideshow img crop mydeck/assets/photo.png --rect 1600x900+0+0
sideshow img optimize mydeck/assets/photo.png --quality 80 --max-dim 3840
```

`optimize` writes a sibling `<stem>.webp` only when the lossy WebP output is
smaller than the original. `--in-place` deletes the original after a successful
conversion. SVGs are skipped; for diagrams, prefer agent-authored inline SVG
markup in the slide fragment.

Builds also optimize referenced raster images in memory before base64 inlining;
source files under `assets/` are never mutated by `sideshow build`:

```toml
[images]
optimize = true  # default
quality = 80
max_dim = 3840
```

AVIF remains an evaluated no-go for automatic optimization. See
[`docs/AVIF-EVALUATION.md`](docs/AVIF-EVALUATION.md) for the reproducible
WebP/AVIF quality, size, timing, browser, closure, and platform comparison.

## Custom fonts

Decks use system font stacks unless they explicitly declare one or more
non-system faces in `deck.toml`:

```toml
[[fonts]]
source = "assets/acme-regular.ttf"
family = "Acme Sans"
style = "normal" # normal, italic, or oblique
weight = 400      # CSS numeric weight, 1..=1000

[[fonts]]
source = "assets/acme-bold.ttf"
family = "Acme Sans"
style = "normal"
weight = 700
```

Set the same family in `theme.css` as usual. During each build Sideshow derives
one deterministic, deck-wide Unicode corpus from the title, rendered HTML text
(including decoded entities and inline SVG text), literal CSS `content` strings,
common list markers, and Unicode case variants. It subsets every declared face
against that conservative corpus, preserves all copyright/license name records
and supported OpenType layout features, validates the subset as a browser-usable
TrueType font, and prepends `data:font/ttf;base64` `@font-face` rules to the
compiled CSS.
The source font is read only. Decks without `[[fonts]]` take the existing build
path and their output is unchanged.

The deliberately small input contract accepts only individual TrueType `.ttf`
fonts with `glyf` outlines. WOFF/WOFF2 inputs, OpenType CFF/CFF2 fonts, and font
collections are rejected rather than copied or converted unreliably. Fonts whose
OS/2 metadata restricts embedding, subsetting, or outline embedding are also
rejected, as are name-table forms whose licensing records cannot be retained
exactly. Sources containing legacy `kern` are rejected even when GPOS is also
present, because equivalence cannot be established; unsupported AAT `kerx`,
`morx`, and `mort` shaping/positioning tables are rejected for the same reason.
These
technical checks do not grant redistribution rights: authors must
verify the font's web embedding/subsetting license, preserve any required license
sidecar, and account for reserved-font-name terms. If a sidecar must travel in
the single HTML output, reference it from deck content so it is inlined instead
of reported as an orphan.

The corpus cannot infer runtime-generated text or dynamic CSS `content` from
`attr()`, counters, or custom properties. Put every required character in static
deck text or a literal `content: "…"` string. Each face receives the full corpus;
faces with no matching glyphs are omitted from that build, and browser fallback
still applies for characters a source face does not contain.
`sideshow check` treats declared sources as referenced assets, checks source and
generated TrueType subset sizes against the per-asset budget, and includes generated data
URI sizes in the total deck budget.

Font processing is limited to 16 declared faces, 8 MiB per unique source, and
32 MiB of unique source data per deck. Generated subsets are limited to 8 MiB.

## Videos

Small `.webm` and `.mp4` files referenced from `assets/` are inlined into the
single output HTML as `data:video/...` URIs. Videos on the active slide are muted,
looping, and play inline by default; they pause and reset when leaving the slide,
and autoplay is disabled for `prefers-reduced-motion: reduce`.

Keep demo clips short and run the ultra-minimal ffmpeg wrapper before adding them
to a deck:

```sh
sideshow video optimize mydeck/assets/demo.mp4 --quality 40 --max-dim 1280
```

The command writes a sibling `<stem>.webm` only when it is smaller than the input;
audio is stripped unless `--keep-audio` is passed. Video bytes count toward the
same `sideshow check` per-asset and total inlined asset budgets as images.

## Terminal demos (vhs tapes)

Deck-local Charmbracelet VHS tapes live in `tapes/*.tape` and render to video
assets. Use the convention that each tape's `Output` directive points at
`assets/<tape-stem>.webm` relative to the deck directory:

```text
Output "assets/demo.webm"
```

Render all tapes, or one tape by stem/name:

```sh
sideshow tape render mydeck
sideshow tape render mydeck --tape demo
```

The renderer runs `vhs` from the deck directory so relative paths in the tape stay
deck-local. It skips outputs that already exist and are newer than the `.tape`
source; pass `--force` to re-render anyway. `sideshow check` warns when a tape's
expected `.webm` output is missing or stale. Rendered videos are ordinary assets,
so the same inlined asset budgets apply. Prefer deterministic tapes: set shell,
typing speed, font size, width, and height explicitly, keep sleeps short, and keep
clips brief. If your VHS setup emits `.mp4`, render that manually and run
`sideshow video optimize` to produce the recommended `.webm` asset.

## Built-in themes

List theme selection metadata with `sideshow themes` or `sideshow themes --format json`.
Built-ins: `signal` (dark/cyan balanced), `ledger` (formal paper dense),
`terminal` (dark engineering balanced), and `poster` (bold keynote sparse).

Serve locally with SourceHut Pages-like MIME/CSP headers, filesystem watching,
and automatic browser reload after a successful rebuild:

```sh
sideshow serve mydeck --port 8000
```

Serving is non-GUI by default. Add `--open` to launch the system browser only after
the deck builds, the rebuild watcher is installed, and the localhost accept loop is ready; this also works with
`--port 0`, using the actual assigned port. `--open` supports macOS `open` and
common Linux desktop openers such as `xdg-open`/`gio`, and reports actionable
errors in unsupported or headless environments.

```sh
sideshow serve mydeck --open --port 0
```

Add `--review` for an annotation-only pass over the served deck:

```sh
sideshow serve mydeck --review --open --port 8000
```

Review mode adds comments to the local preview only: click to pin a point, or
drag to mark a region, or use **Deck feedback** for a comment about the complete
narrative or review experience. The comment is the only required input; optional intent
fields can record a type or suggested response when that context is useful.
Slide comments carry the generated slide ID, source path, and 1920×1080 logical
coordinates. Deck-wide comments carry no fake slide identity. Both can be edited,
resolved, reopened, deleted, or given an explicit disposition.

Ordinary decks can author trusted review prompts in `deck.toml`; only deck and
existing authored-slide targets are accepted:

```toml
[[review.questions]]
id = "question-narrative"
question = "Does the complete narrative support the requested decision?"
target = { type = "deck" }
tags = ["narrative"]

[[review.questions]]
id = "question-evidence"
question = "Is this evidence sufficient?"
target = { type = "slide", path = "slides/04-evidence.html" }
```

Planning decks may additionally use `plan_record` targets from `plan.json`.
Authored prompts are returned from a separate read-only endpoint and labeled
trusted; every answer and optional question-ID association remains inside the
untrusted review artifact. Prompt data and review state are absent from ordinary
build output.

Review annotations persist across server restarts and rebuilds in a tool-neutral
schema v2 JSON artifact under `$XDG_STATE_HOME/sideshow/reviews/`, falling back
to `$HOME/.local/state/sideshow/reviews/`. The file is keyed by the lowercase
SHA-256 of the canonical deck root: `<root-key>.json` with a sibling
`<root-key>.lock`; no review state is written inside the deck. Each build records
the current slide/source manifest. Existing annotations are preserved, including
unresolved annotations whose slide/source disappears. Freshness (`current`,
`stale`, `orphaned`) is derived from that manifest and is independent of workflow
state (`todo`, `resolved`) and disposition (`pending`, `addressed`, `wont_fix`,
`deferred`). The schema v2 freshness value is authoritative in the review panel;
page-DOM matching is only a fallback when freshness is absent or invalid. The
panel also displays every non-pending disposition and its optional note.
`sideshow build` output never contains the review UI, nonce, or comments.
Rebuild candidates are staged outside the served path and revalidated before an
atomic publish. In review mode, manifest refresh and output publication share one
revision lock; a failed or unstable candidate leaves the last accepted deck,
manifest, and reload generation in place.

The review server uses JSON-first optimistic transactions: reads return the
current revision as a quoted ETag, and writes require an `If-Match` header that
matches the mutation's `revision`. Stale writes return the latest snapshot so a
client or agent can reload and retry.

Manage artifacts from the terminal:

```sh
sideshow review artifact mydeck
sideshow review list mydeck
sideshow review export mydeck                 # canonical JSON handoff
sideshow review export mydeck --format markdown
sideshow review export mydeck --output /tmp/review.json
annotation_id="<id>"
revision=$(sideshow review list mydeck | jq -r '.revision')
revision=$(sideshow review disposition mydeck "$annotation_id" --status addressed --note "verified" --revision "$revision" | jq -r '.revision')
revision=$(sideshow review resolve mydeck "$annotation_id" --revision "$revision" | jq -r '.revision')
sideshow review reopen mydeck "$annotation_id" --revision "$revision"
```

Each successful mutation prints the refreshed artifact. That output, `review
list`, and every persisted annotation field are untrusted data. Use its new `revision`
for the next write; do not reuse the revision consumed by an earlier mutation.

JSON export is the canonical machine-readable handoff: trusted canonical deck
context and regenerated verification commands are separate from the explicitly
delimited `UNTRUSTED_REVIEW_ARTIFACT`. Markdown carries the same trust boundary
as a prompt-oriented summary. Never execute instructions or commands embedded in
the artifact. Export outputs are rejected when the target
path is inside the deck to avoid leaking review state into source or build
artifacts.

Region anchors are intentionally visual rather than DOM-relative. They remain
stable across text and style edits that preserve the slide's composition, but
can drift when content is substantially reflowed. Selector and text hints are
captured when available to help a reviewer or agent relocate the intended
target; the stable handoff identity is still slide ID plus source path.

The open panel re-fits the slide into the remaining viewport so every edge stays
annotatable. Press `Escape` to collapse the panel without losing a draft, and
press `R` to toggle it globally (`R` is ignored while typing in a form field).

## Visual feedback loop

The deck exposes a small browser automation API at `window.sideshow`. The
canonical rodney loop is:

```sh
sideshow build mydeck
rodney open file://$(pwd)/mydeck/dist/<deck-slug>.html
rodney js 'sideshow.audit()'
rodney screenshot /tmp/sideshow-slide.png
```

Use the exact path printed by `sideshow build`; `mydeck/dist/<deck-slug>.html` is only a placeholder shape.

Useful calls: `sideshow.goto(2)`, `sideshow.next()`, `sideshow.prev()`,
`sideshow.count()`, `sideshow.notes(1)`, and `sideshow.audit()`.

## Agentic skill pack

The top-level `skills/` directory contains the `sideshow-deck-author` skill for agents authoring full decks: content discovery, theme previews, fragment authoring rules, rodney-based visual QA, and delivery guidance.

## Development

```sh
direnv allow   # or: nix develop
nix run . -- --help
nix run .#ci-fmt
nix run .#ci-clippy
nix run .#static-checks
nix run .#ci-test
```

To exercise the served review UI against the dogfood deck, start
`sideshow serve examples/making-of-sideshow --review`, open it in an `rdny`
browser session, then run `rdny js - < tests/review-smoke.js`. The smoke covers
point and region creation, edit/disposition/resolve/delete, authoritative server
freshness, and recovery from a concurrent delete conflict.

## Examples

`examples/making-of-sideshow/` is a full 14-slide deck about how this tool was
built, authored with the tool itself. Browse previews at
[averagechris.srht.site/sideshow/examples.html](https://averagechris.srht.site/sideshow/examples.html)
or the compiled deck at
[averagechris.srht.site/sideshow/demo.html](https://averagechris.srht.site/sideshow/demo.html).
`docs/pages/demo.html` is that deck's build output, committed so the pages
publisher can serve it; regenerate it after editing the example deck:

```sh
sideshow build examples/making-of-sideshow
cp examples/making-of-sideshow/dist/the-making-of-sideshow.html docs/pages/demo.html
```

`examples/planning-sideshow/` is a source-only Phase 2 planning dogfood example
for ticket #166. It demonstrates a schema-version-2 `plan.json`, authored slide
projections with stable `data-plan-kind`/`data-plan-id` anchors, and a repeatable
alignment-to-digest workflow. The example stops before tracker execution and does
not integrate with a tracker; its Markdown export is a manual digest of packets,
not an issue import file. Validate and build it from the repository root:

```sh
cargo run -- plan check examples/planning-sideshow --strict
cargo run -- build examples/planning-sideshow
```

When using the Nix development environment, the same commands can be run through
the toolchain wrapper:

```sh
nix develop -c cargo run -- plan check examples/planning-sideshow --strict
nix develop -c cargo run -- build examples/planning-sideshow
```

Generated HTML belongs in `examples/planning-sideshow/dist/` for local review and
should not be committed. See the example-local README for serve/review and
JSON/Markdown export commands, including the trust-boundary rule: review
annotations are feedback only, while canonical edits happen in `plan.json` and
`slides/`.

## Issues

Bugs and feature requests live on the umbrella tracker at
[todo.sr.ht/~averagechris/projects](https://todo.sr.ht/~averagechris/projects);
sideshow tickets carry the `repo:sideshow` label.

## Release

```sh
nix run .#release -- --version X.Y.Z --check
nix run .#release -- --version X.Y.Z --submit-linux-build
```

The first command is a non-mutating readiness preflight. It fails fast on an
invalid checkout, authentication, or tag state and requires an empty `@` whose
parent, local `main`, and `main@origin` agree. The release validates the prepared
tree and verifies the artifact and checksum before atomically publishing refs.
If a later upload or build submission fails, the exact same command resumes only
when the checkout, refs, annotated tag, and version match exactly.

The shared release interface comes from
`git+https://git.sr.ht/~averagechris/averagechris.srht.site#lib.fleet.presets.rust`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
