# Static planning components

Use the compiler-owned `.plan-*` classes for strategy, alignment, roadmap,
proposed-work, and retrospective slides. They are plain HTML/CSS: no custom
elements, no component JavaScript, and no framework assumptions. Planning slides
help a team refine and digest work into its issue tracker; they are not a live
execution dashboard.

## Rules for agents

- Use semantic native elements first: `header`, `footer`, `section`, `article`, `figure`, `table`, `ol`, `ul`, `time`, `details`, `summary`.
- Use namespaced `.plan-*` classes only for planning components.
- Put the canonical workflow value in `data-state` (`draft`, `todo`,
  `in_progress`, `in_review`, `blocked`, `done`, or `completed`). Use the bounded
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
    <span class="plan-status" data-state="watch">Watch: API track dependency</span>
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

- Use rails when order matters and the audience should follow a proof or decision path from top to bottom.
- Use lanes when parallel ownership or workstream separation matters.
- Use layers when context, system boundaries, or constraints explain why work is arranged a certain way.
- Use connectors when one item depends on another and the dependency itself is the message.
- Use cards for standalone facts, decisions, metrics, or risks; avoid making every slide a wall of cards.
- Use compact evidence when proof is important but commands/logs would distract from the narrative. Native `<details>` gives keyboard support without JavaScript.

## Lifecycle boundary

- Use statuses to communicate planning maturity or a proposed starting state, not
  to mirror live tracker status.
- Show enough decomposition, dependencies, acceptance, and verification intent
  for a human or agent to draft tracker issues after review.
- Do not design slides as a replacement backlog, sprint board, or completion log.
- Optional prototypes should answer planning questions; their findings belong in
  the rationale, decision, or evidence narrative.
- For a later demo or project summary, reuse these visual primitives in a separate
  ordinary deck. Authors and agents can synthesize plan, tracker, code, and media
  context themselves; Sideshow does not need direct integrations or ingestion.
