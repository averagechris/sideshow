# Static planning components

Use the compiler-owned `.plan-*` classes for strategy, roadmap, and execution-plan slides. They are plain HTML/CSS: no custom elements, no component JavaScript, and no framework assumptions.

## Rules for agents

- Use semantic native elements first: `header`, `footer`, `section`, `article`, `figure`, `table`, `ol`, `ul`, `time`.
- Use namespaced `.plan-*` classes only for planning components.
- Put the canonical workflow value in `data-state` (`draft`, `todo`,
  `in_progress`, `in_review`, `blocked`, `done`, or `completed`). Use the bounded
  `data-tone="good|warn|risk|info"` variants to select presentation emphasis and
  `data-critical="true"` only for exceptional prominence.
- Always include visible text for status and risk; color must reinforce, not replace, the label.
- Prefer HTML/CSS-first diagrams. Inline SVG is acceptable when it has a text label/caption and visible node text.

## Common pattern

```html
<section class="plan-shell">
  <header class="plan-header">
    <div>
      <p class="plan-eyebrow">Q3 execution plan</p>
      <h1 class="plan-title">Ship the static planner</h1>
    </div>
    <span class="plan-status" data-state="watch">Watch: API track dependency</span>
  </header>
  <div class="plan-grid" style="--plan-cols: 2">
    <article class="plan-card"><h3>Decision</h3><p class="plan-decision">Keep components compiler-owned and JavaScript-free.</p></article>
    <aside class="plan-callout" data-tone="risk"><strong>Risk:</strong> status chips must include text labels, not color alone.</aside>
  </div>
  <footer class="plan-footer"><span class="plan-meta">Owner: Agent track</span></footer>
</section>
```

## Vocabulary

- Shell: `.plan-shell`, `.plan-header`, `.plan-footer`, `.plan-title`, `.plan-eyebrow`, `.plan-meta`.
- Cards/lists: `.plan-grid`, `.plan-cards`, `.plan-card`, `.plan-list`.
- Status: `.plan-status` with `data-state` or `data-tone`, with readable text inside.
- Emphasis: `.plan-callout`, `.plan-decision`, `.plan-risk`.
- Metrics: `.plan-metrics` containing `.plan-metric` and a visible `<strong>` value.
- Work/process: `.plan-workstreams`, `.plan-workstream`, `.plan-milestones`, `.plan-milestone`, `.plan-flow`, `.plan-step`, `.plan-timeline`, `.plan-event`, `.plan-lanes`, `.plan-lane`.
- Tables/diagrams: `.plan-matrix`, `.plan-deps`, `.plan-diagram` with `data-node`/`data-edge` and `figcaption`.
