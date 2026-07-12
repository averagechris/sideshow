# Static planning components

Use `.plan-*` classes for strategy, alignment, roadmap,
proposed-work, and retrospective slides. They are plain HTML/CSS: no custom
elements, no component JavaScript, and no framework assumptions. Planning slides
help a team refine and digest work into its issue tracker; they are not a live
execution dashboard.

Choose primitives by cognitive contract, not keyword matching. First decide what
the audience must understand, where anchored feedback should land, and how the
artifact distributes the team's mental model; then pick the smallest primitive
that communicates that job.

Contract quality remains an authoring responsibility. A manifest check can prove
that relationship/use/misleading fields exist, but cannot prove they are honest or
useful. Name the reader inference precisely (sequence, contrast, ownership,
containment, dependency, or decision), and name an evidence condition under which
the shape would mislead. Do not repeat a component name three ways.

## Rules for agents

- Use semantic native elements first: `header`, `footer`, `section`, `article`, `figure`, `table`, `ol`, `ul`, `time`, `details`, `summary`.
- Use namespaced `.plan-*` classes only for planning components.
- Put the canonical workflow value in `data-state` (`todo`, `in_progress`,
  `blocked`, `in_review`, `done`, or `dropped`). These match schema-v2 JSON
  serialization and DOM anchors; CLI mutation flags accept kebab-case spellings.
  Use the bounded
  `data-tone="good|warn|risk|info"` variants to select presentation emphasis and
  `data-critical="true"` only for exceptional prominence.
- Always include visible text for status, risk, and dependency criticality; color must reinforce, not replace, the label.
- Prefer HTML/CSS-first diagrams. Inline SVG is acceptable when it has a text label/caption and visible node text.
- Use native disclosure for progressive detail: `<details class="plan-evidence"><summary>Proof intent</summary>…</details>`. Do not add component JavaScript for toggles.

## Common pattern

```html
<section class="plan-shell">
  <header class="plan-header">
    <div>
      <p class="plan-eyebrow">Q3 alignment plan</p>
      <h1 class="plan-title">Agree how to ship the static planner</h1>
    </div>
    <span class="plan-status" data-state="blocked">Blocked: API track dependency</span>
  </header>
  <div class="plan-grid" style="--plan-cols: 3">
    <ol class="plan-rail" aria-label="Ordered proof path">
      <li><article><h3>Intent</h3><p>State the planning claim.</p></article></li>
      <li><article><h3>Evidence</h3><p>Point to the verification artifact.</p></article></li>
      <li><article><h3>Digest</h3><p>Describe the proposed issue handoff.</p></article></li>
    </ol>
    <section class="plan-layers" aria-label="System context layers">
      <article class="plan-layer" data-layer="Context"><h3>Human view</h3><p>What reviewers need to understand first.</p></article>
      <article class="plan-layer" data-layer="System"><h3>Compiler owned</h3><p>Where the implementation boundary lives.</p></article>
    </section>
    <details class="plan-evidence">
      <summary>Proof intent: CSS and no component JS</summary>
      <p>Run <code>cargo test plan_component_css_is_compiler_owned_and_js_free --lib</code>.</p>
    </details>
  </div>
  <footer class="plan-footer"><span class="plan-meta">Owner: Agent track</span></footer>
</section>
```

## Vocabulary

- Shell: `.plan-shell`, `.plan-header`, `.plan-body`, `.plan-footer`, `.plan-title`, `.plan-eyebrow`, `.plan-meta`. Wrap multiple content blocks in a semantic `<main class="plan-body">` so the shell keeps header/content/footer rhythm without stretching individual callouts.
- Cards/lists: `.plan-grid`, `.plan-cards`, `.plan-card`, `.plan-list`.
- Ordered rail: `.plan-rail` on an `<ol>` with each meaningful step as an `<li>` containing semantic content. Use it for sequence, proof paths, approval order, or phased rollout. Do not use it just to decorate unrelated cards.
- Status: `.plan-status` with `data-state` or `data-tone`, with readable text inside.
- Emphasis: `.plan-callout`, `.plan-decision`, `.plan-risk`.
- Metrics: `.plan-metrics` containing `.plan-metric` and a visible `<strong>` value.
- Work/process: `.plan-workstreams`, `.plan-workstream`, `.plan-milestones`, `.plan-milestone`, `.plan-flow`, `.plan-step`, `.plan-timeline`, `.plan-event`, `.plan-lanes`, `.plan-lane`.
- Layers: `.plan-layers` with `.plan-layer data-layer="Context|System|Boundary|Constraint"`. Use layers when the point is nesting, responsibility, or environmental context rather than ownership lanes.
- Dependency connectors: `.plan-connectors` with `.plan-connector data-from="…" data-to="…"`; add `data-critical="true"` only when the visible text also explains the critical dependency. Use connectors for named dependencies, handoffs, or prerequisites; use tables when many items need comparison.
- Evidence: `.plan-evidence` on `<details>` with a concise `<summary>` describing proof intent, followed by commands, links, or logs. Keep the summary human-readable; put terminal commands in the body so disclosure stays keyboard-native.
- Tables/diagrams: `.plan-matrix`, `.plan-deps`, `.plan-diagram` with `data-node`/`data-edge` and `figcaption`.

## Choosing a planning primitive

- **Rails communicate sequence.** Use when the audience should follow proof,
  approval, rollout, or decision order. Avoid when items are peers.
- **Lanes communicate parallel responsibility or workstreams.** Use when comparing
  ownership or simultaneous tracks. Avoid when the message is dependency order.
- **Layers communicate boundaries and context.** Use when environment,
  responsibility, or constraints explain the shape of the work. Avoid for simple
  lists.
- **Connectors communicate named dependencies or handoffs.** Use when the edge is
  the point. Avoid dense hairballs; switch to tables or grouped lists.
- **Cards communicate bounded standalone facts.** Use for decisions, metrics,
  risks, or tasks that need anchors. Avoid walls of interchangeable cards.
- **Evidence disclosure communicates proof without stealing focus.** Use native
  `<details>` for commands, logs, or rationale needed on demand. Avoid hiding the
  main claim inside disclosure.

Raw HTML/CSS, Markdown, bundled registered components, and project-local
registered components are all first-class paths. Prefer raw markup for bespoke
visual reasoning; if a pattern repeats, move it through raw → reviewed local
pattern → project component → possible bundled incubation only after broader use.
Dogfood the rendered relationship, not only the manifest wording: if a dependency,
handoff, contrast, or feedback cycle becomes prose in interchangeable boxes, the
component does not fit even when its intent keywords do. Use raw HTML/CSS for the
current explanation and record repeated successful flow geometry as evidence for a
narrow reusable primitive.

Use the fixed stage intentionally. Sparse relationship views should normally occupy
the vertical middle of the canvas; top-weighted composition is appropriate for
dense reading only when chosen deliberately. Browser inspection must check
letterform collisions, connector alignment, and review-panel scale in addition to
overflow.

## Lifecycle boundary

- Use statuses to communicate planning maturity or a proposed starting state, not
  to mirror live tracker status.
- Show enough decomposition, dependencies, acceptance, and verification intent
  for a human or agent to draft tracker issues after review.
- Treat JSON as the normalized strict schema-v2 machine boundary. A Markdown
  planning digest is derived manual drafting material: per-task packets can be
  split, combined, relabeled, reprioritized, assigned to teams, placed in
  milestones, or adapted to local tracker conventions by the human or agent doing
  the handoff.
- Exact `verification.commands` from canonical plan data are trusted/verbatim
  executable material. Review annotations remain untrusted feedback and should
  not be shown as digest facts or commands.
- For authored component slides, prefer registry-discovered `plan-record-card`
  when directly representing one canonical record. For richer semantic visuals,
  use components with the explicit `plan-bindable` capability (for example
  `before-after`, `trust-boundary`, `concrete-example`, or `decision-feedback`)
  and provide an explicit bind; Sideshow does not infer anchors from intent or
  keywords. Component binds emit visible-root `data-plan-kind` and
  `data-plan-id` anchors and count toward strict coverage. Raw `.plan-*` markup
  may still use paired `data-plan-kind`/`data-plan-id` anchors when the
  registered component catalog is too small. Unpaired `data-plan-id` anchors and
  wrong kind/id pairs are diagnostics rather than coverage.
- Do not design slides as a replacement backlog, sprint board, or completion log.
- Do not design slides or exports around tracker APIs, credentials, imports,
  issue creation, synchronization, or execution history.
- Optional prototypes should answer planning questions; their findings belong in
  the rationale, decision, or evidence narrative.
- For a later demo or project summary, reuse these visual primitives in a separate
  ordinary deck. Authors and agents can synthesize plan, tracker, code, and media
  context themselves; Sideshow does not need direct integrations or ingestion.
