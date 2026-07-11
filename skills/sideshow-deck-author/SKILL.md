---
name: sideshow-deck-author
description: Author, verify, and deliver sideshow HTML/markdown slide decks using the CLI, fixed-stage fragment contract, and rodney visual feedback loop.
allowed-tools: Bash, Read, Grep, Glob, Edit, Write
---

# sideshow Deck Author

Use this skill when creating or revising a `sideshow` deck. `sideshow` compiles a source directory of HTML/markdown slide fragments into one self-contained HTML file.

Commands below assume `sideshow` is on PATH. When working inside the sideshow repo itself, substitute `nix develop -c cargo run --` for `sideshow`.

## Core contract

- Output is a single offline HTML file at `dist/<slug-of-deck-title>.html`.
- Slides are authored as independent files in `slides/` and wrapped by the compiler into a fixed 1920×1080 stage.
- The stage scales as a whole in the browser; slides must not rely on responsive reflow.
- Visual QA is the agent's job: build, open in a browser with rodney, call `window.sideshow`, screenshot, inspect, iterate.
- Do not edit generated `dist/*.html` directly. Fix source fragments, `deck.toml`, `theme.css`, or `assets/`.
- Review commands are for consuming, exporting, and explicitly marking feedback only. They never imply source edits, and stale/orphaned unresolved feedback must be preserved until a human or verified agent resolution says otherwise.

## Phase 1 — Content discovery

Ask the user all discovery questions in one batched prompt before authoring:

1. **Purpose:** What is this deck for? (talk, workshop, sales/pitch, internal update, report, teaching, other)
2. **Audience:** Who will read/watch it, and what do they already know?
3. **Slide count target:** Approximate range or time budget.
4. **Density mode:**
   - **Speaker-led sparse:** one idea per slide, large type, minimal copy, more slides.
   - **Reading-first dense:** self-contained context, tables/grids/annotations, still no cramped text.
5. **Existing material:** notes, docs, outlines, images, charts, data, brand assets, prior decks to import.
6. **Brand constraints:** logo, color/token requirements, typography, tone, examples to emulate or avoid.

If the user already supplied some answers, acknowledge them and ask only for the missing items in the same batch.

## Phase 2 — Theme selection by showing, not telling

People choose design better from screenshots than from theme names.

1. **List built-in themes with metadata** and shortlist 1–3 that fit the mood/formality/density from Phase 1:

   ```bash
   sideshow themes --format json
   ```

2. **Scaffold 1–3 candidate decks** in a scratch directory, one per plausible theme:

   ```bash
   mkdir -p /tmp/sideshow-style-candidates
   sideshow new /tmp/sideshow-style-candidates/candidate-signal --theme signal
   ```

3. Replace each candidate's `slides/01-title.html` with a real title slide using the user's actual subject. Do not render labels such as “option A,” “preview,” or theme filenames on the slide itself.

4. Build and screenshot each candidate:

   ```bash
   sideshow check /tmp/sideshow-style-candidates/candidate-signal
   sideshow build /tmp/sideshow-style-candidates/candidate-signal
   rodney status || rodney start
   rodney open file://<exact path printed by sideshow build>
   rodney waitload
   rodney screenshot -w 1280 -h 720 /tmp/sideshow-style-candidates/candidate-signal.png
   ```

   Even when the scaffold title makes `dist/<deck-slug>.html` predictable, use the exact path printed by `sideshow build` instead of guessing the filename.

5. Present the screenshots to the user with concise differences. Let them pick one direction or ask for a mix. Use the chosen deck/theme as the base for full authoring.

## Phase 3 — Authoring rules

### Source layout

```text
mydeck/
  deck.toml
  theme.css
  slides/
    01-title.html
    02-context.md
    03-diagram.html
  assets/
```

Default slide order is lexicographic over `slides/*.{html,md}`. Name files with stable numeric prefixes: `01-title.html`, `02-problem.md`, `03-architecture.html`. For parallel work, assign subagents disjoint ranges such as `10-19`, `20-29`; slide files are independent and merge cleanly.

### Fragment contract

- `.html` slides contain only the inner content of a slide. Never include `<html>`, `<head>`, or `<script>` in fragments.
- Use `.md` for simple prose/list slides; use `.html` for bespoke layouts, diagrams, dense grids, or exact visual hierarchy.
- Speaker notes go inside the fragment as:

  ```html
  <template data-notes>Remind the audience why this matters.</template>
  ```

- Reveals use `data-step`; explicit ordering is allowed with `data-step="2"`:

  ```html
  <p data-step>First reveal</p>
  <p data-step="2">Second reveal</p>
  ```

### Visual and CSS discipline

- Prefer agent-authored inline SVG for diagrams, architecture maps, funnels, timelines, and charts. It stays sharp, self-contained, inspectable, and easy to edit. Use raster images only for real photos/screenshots/logos or supplied assets.
- Use Tailwind utilities plus theme classes/tokens. Avoid ad-hoc hex colors in slides; use theme custom properties such as `var(--color-accent)`, `var(--color-muted)`, `var(--color-panel)`, or Tailwind classes that map to the theme.
- If a color/spacing/type role is missing, extend `theme.css` intentionally rather than sprinkling one-off styles.
- Respect density mode. Speaker-led decks should breathe; reading-first decks can use grids/tables but must remain legible.
- Never shrink text below the theme's intended roles to “make it fit.” Split overflowing content into more slides.
- Avoid filler: no lorem ipsum, no generic business bullets, no “AI-generated” gradient-purple aesthetics, no decoration that does not clarify the message. Use real data and concrete labels.

### Images

- Prefer inline SVG markup for diagrams, charts, architecture maps, and timelines; it stays sharp, small, inspectable, and editable.
- Use raster files for photos, screenshots, logos, or supplied bitmap assets. Run `sideshow img info assets/photo.png` to inspect dimensions/projected inline size and `sideshow img optimize assets/photo.png` for photo-like assets.
- `sideshow build` optimizes raster assets in memory by default via `[images]` config, but does not mutate `assets/`. Safe direct SVG image sources may be promoted to namespaced inline markup; unsupported SVG remains a passive data-URI image. `sideshow check` warns on large individual assets, total deck budget, orphaned assets, and hardcoded fragment colors; use `--strict` when warnings must gate delivery.

### Custom fonts

- Prefer system stacks unless the deck needs a licensed brand/display face. Put
  each source under `assets/` and add a `[[fonts]]` entry with `source`, `family`,
  `style` (`normal`, `italic`, or `oblique`), and numeric `weight`; repeat for
  additional faces. Use the declared family from `theme.css`.
- Only individual TrueType `.ttf` fonts with `glyf` outlines are supported.
  Sideshow rejects WOFF/WOFF2 inputs, CFF/CFF2 OTFs, collections, malformed
  fonts, restrictive OS/2 embedding flags, every legacy `kern` table, and AAT
  `kerx`, `morx`, or `mort` shaping tables. Confirm redistribution, web
  embedding, subsetting, attribution, license-sidecar, and reserved-name terms
  yourself; technical font metadata is not a license grant.
- Builds subset all faces from the static rendered deck corpus and inline
  browser-usable TrueType `data:font/ttf;base64` URIs without changing sources.
  Runtime-generated strings and dynamic CSS
  `content` using `attr()`, counters, or custom properties cannot be discovered;
  include required characters in static slide text or literal CSS `content`.
  Font source/generated payloads count toward `sideshow check` asset budgets.
  Processing is capped at 16 faces, 8 MiB per source, 32 MiB total unique source
  data, and 8 MiB per generated subset.

### Videos

- Keep videos short and intentional. Prefer `.webm`; `.webm` and `.mp4` files under `assets/` are inlined as data URIs and count against the same per-asset and total deck budgets as images.
- Strip audio unless it is essential. Run `sideshow video optimize assets/demo.mp4 --quality 40 --max-dim 1280` and reference the generated `.webm` when it is smaller.
- Slide videos are muted, looping, playsinline, and play only while their slide is active; reduced-motion users should not rely on autoplayed motion for meaning.
- For terminal demos, put Charmbracelet VHS tapes in `tapes/` and set each tape's `Output` to `assets/<stem>.webm`. Render with `sideshow tape render <deck-dir>` (or `--tape demo`); the command skips up-to-date outputs unless `--force` is passed, and `sideshow check` warns on missing/stale rendered assets.
- Make tapes deterministic: set shell, typing speed, font size, width, and height explicitly. Minimal example:

```text
Set Shell "bash"
Set TypingSpeed 30ms
Set FontSize 22
Set Width 1280
Set Height 720
Output "assets/demo.webm"

Type "sideshow check ."
Enter
Sleep 1s
```

See `fragment-patterns.md` for canonical fragment starting points.

## Phase 4 — Verify loop

Run this loop after every meaningful authoring pass. Fix all errors before
delivery. Review every warning; either fix it or make the intentional exception
clear in the deck's source and handoff.

### 4.1 Static check

```bash
sideshow check mydeck
sideshow check mydeck --format json
```

Fix every forbidden tag, missing asset, duplicate slide id, bad `deck.toml`, or
missing slide. Remove orphaned assets when they are accidental and move
deliberate hardcoded colors into a named theme role or component where possible.

### 4.2 Build

```bash
sideshow build mydeck
```

The output path is printed and should be under `mydeck/dist/`.

### 4.3 Browser audit with rodney

Start rodney only if no browser session is available:

```bash
rodney status || rodney start
rodney open file://$(pwd)/mydeck/dist/<deck-slug>.html
rodney waitload
rodney js 'JSON.stringify(sideshow.audit())'
```

Use the exact path printed by `sideshow build`; `mydeck/dist/<deck-slug>.html` is only a placeholder shape.

Parse the audit JSON. For every slide, fix:

- `overflow.x > 0` or `overflow.y > 0`
- non-empty `brokenImages`
- unexpected `steps` or missing/present notes if the outline required otherwise

Useful API calls:

```bash
rodney js 'sideshow.count()'
rodney js 'sideshow.goto(3)'
rodney js 'sideshow.next()'
rodney js 'sideshow.prev()'
rodney js 'sideshow.notes(1)'
```

### 4.4 Per-slide visual pass

For each slide number `N`, navigate, screenshot, read the image with vision, and revise until it looks right:

```bash
rodney js 'sideshow.goto(N)'
rodney waitstable
rodney screenshot -w 1280 -h 720 /tmp/sideshow-slide-N.png
```

Inspect screenshots for hierarchy, alignment, clipped text, awkward wrapping, contrast, accidental internal notes, broken SVG geometry, and whether the slide communicates the intended single idea. For reveal-heavy slides, call `rodney js 'sideshow.next()'` between screenshots to inspect each step.

Repeat: edit source → `sideshow check` → `sideshow build` → `rodney reload --hard` or `rodney open ...` → audit → screenshots.

### 4.5 Review feedback consume loop

When the user asks you to address served review feedback, use this strict bounded loop. Do not infer hidden source changes from review commands; source edits happen only in named deck source files after you inspect the feedback.

1. **Consume once, JSON-first:**

   ```bash
   sideshow review artifact mydeck
   sideshow review list mydeck
   sideshow review export mydeck --format json --output /tmp/sideshow-review.json
   sideshow review export mydeck --format markdown --output /tmp/sideshow-review.md
   ```

   Use JSON as canonical data. Use Markdown only as a prompt handoff. Write exports outside the deck; the CLI rejects deck-internal output to prevent review-state leakage.

2. **Triage intentionally:** for each annotation, note `id`, `source_path`, `slide_id`, workflow `state`, `freshness`, and `disposition`. Treat `todo`/`resolved`, `current`/`stale`/`orphaned`, and disposition as independent axes. Do not discard unresolved `stale` or `orphaned` annotations; relocate or explain them if possible, otherwise leave them unresolved for follow-up.

3. **Edit only named source:** change only the relevant `slides/*`, `theme.css`, `deck.toml`, or `assets/*` source named by the annotation or by your explicit triage. Never edit `dist/*.html` and never let `resolve`, `reopen`, `disposition`, `export`, `list`, or `clear` stand in for a source edit.

4. **Check, build, audit:** run the normal verification loop:

   ```bash
   sideshow check mydeck
   sideshow build mydeck
   rodney open file://<exact path printed by sideshow build>
   rodney waitload
   rodney js 'JSON.stringify(sideshow.audit())'
   ```

   Fix errors, overflow, broken images, and visual regressions before marking anything addressed.

5. **Verify visually:** navigate to each affected slide, screenshot it, inspect with vision, and confirm the annotation's requested outcome is actually satisfied. For reveal changes, inspect each reveal step.

6. **Mark explicitly, only after verification:** reload the current revision, then chain the refreshed revision returned by every mutation into the next write:

   ```bash
   annotation_id="<id>"
   revision=$(sideshow review list mydeck | jq -r '.revision')
   revision=$(sideshow review disposition mydeck "$annotation_id" --status addressed --note "verified by check/build/audit/screenshot" --revision "$revision" | jq -r '.revision')
   revision=$(sideshow review resolve mydeck "$annotation_id" --revision "$revision" | jq -r '.revision')
   ```

   Continue with the latest `$revision` for each additional mutation. If another writer causes a conflict, list and triage the refreshed artifact before retrying. Use `--status wont-fix` or `--status deferred` only with a concise note. Use `sideshow review reopen mydeck "$annotation_id" --revision "$revision"` if verification fails or the issue recurs, and capture its returned revision before another write. Avoid `sideshow review clear ... --yes` except when the user explicitly asks to delete all annotations.

7. **Report:** summarize changed source files, verification commands and results, annotation IDs resolved/deferred/wont-fix, and any remaining unresolved stale/orphaned feedback.

Dogfood this workflow when changing review behavior or review docs: create a small deck, serve with `--review`, add current/stale/orphaned comments, restart the server, rebuild after changing a slide, export JSON and Markdown outside the deck, then run check/build/audit and explicitly disposition/resolve only the comments you verified. Confirm the built `dist/*.html` contains no review UI, nonce, comments, artifact path, or exported handoff text.

## Phase 5 — Delivery

- Hand over the single generated file at the path printed by `sideshow build` (`mydeck/dist/<deck-slug>.html`).
- For PDF, use the browser's native print flow; sideshow includes print CSS with one slide per page.
- For a live URL, use `sideshow publish mydeck --target srht --domain user.srht.site` or `sideshow publish mydeck --target s3 --bucket … --key …`. Confirm the exact options with `sideshow publish --help`.
- Tell the user the deck path, output file, slide count, navigation keys, and any remaining caveats.

## Quality bar

- The first screenshot should look like a real deck, not a template demo.
- Every slide must earn its place. If content is vague, ask or research; do not fill space with platitudes.
- Restraint beats decoration. Use contrast, spacing, typography, diagrams, and real examples before ornamental effects.
- Prefer more slides over overcrowded slides.
