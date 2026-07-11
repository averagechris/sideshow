# Sideshow planning feature design

This document is the implementation contract for a planning mode that turns
human/agent/team intent into structured plan data and a reviewable static visual
projection. It is intentionally narrower than a general project-management app.

## Goals

- Capture the planning artifact that already emerges from human-agent work:
  outcomes, constraints, decisions, workstreams, tasks, dependencies, touched
  files, acceptance criteria, verification, risks, and status.
- Keep the canonical plan machine-readable and diff-friendly while allowing many
  projections: HTML review pages, slides, diagrams, exports, and future 2D maps.
- Give humans and agents a shared lifecycle: draft, align, execute, verify,
  update, and hand off.
- Reuse Sideshow's existing static build, review, markup, theming, accessibility,
  and asset guardrails wherever the contracts match.
- Make the prototype useful without a database, hosted service, browser editor,
  or large JavaScript build chain.

## Non-goals

- Replacing issue trackers, Linear, todo.sr.ht, GitHub Projects, or source-control
  history.
- Inventing a live collaborative editor or a hosted planning service.
- Making the visual projection the source of truth.
- Running arbitrary user JavaScript inside plan review output.
- Solving all diagram layout automatically in v1.
- Shipping a full dependency graph engine, scheduling optimizer, or resource
  planner in the prototype.

## Human-agent-team alignment lifecycle

Planning is a loop, not a one-time generated document:

1. **Frame**: human names the desired outcome, boundaries, audience, and known
   constraints.
2. **Structure**: agent creates or updates the canonical plan data, splitting
   work into outcomes, workstreams, tasks, dependencies, risks, and verification.
3. **Align**: human and team review a static projection. Comments may point at
   plan IDs, rendered cards, or source lines, but accepted changes update the
   canonical data.
4. **Execute**: agents and humans work from task IDs and file references. Status
   changes are explicit and reviewable.
5. **Verify**: acceptance criteria and verification commands/evidence are checked
   before marking outcomes complete.
6. **Reconcile**: decisions, scope changes, blockers, and newly discovered risks
   are recorded in the same plan, not buried in chat.
7. **Hand off**: the plan exports a concise status artifact for teammates and a
   durable record for future agents.

The lifecycle should be visible in the data model. A plan is not merely a slide
deck; it is a working agreement with traceable intent and status.

## Canonical data vs visual projection

The canonical source is structured plan data under the deck directory, not the
generated HTML or review artifact. The prototype uses:

```text
my-plan/
  plan.json        # canonical execution data
  deck.toml        # projection metadata
  theme.css        # forkable visual identity
  slides/          # authored human-facing projection
  assets/          # optional local assets
  dist/            # generated output, never edited by hand
```

Rules:

- `plan.json` owns IDs, status, relationships, files, acceptance checks, and
  verification commands.
- Slides explain the plan to humans and may be deliberately more expressive,
  but must not contain the only copy of an actionable task or dependency.
- Generated HTML is disposable. Authored slides remain normal deck source.
- Every rendered component that represents data must carry stable `data-plan-id`
  and `data-plan-kind` attributes so reviews can map comments back to source.
- Manual edits to generated output are unsupported and should be overwritten by
  the next build.

## Schema v1

Use strict JSON for the prototype because it is a direct, unambiguous integration
boundary for implementation agents. The checked schema rejects unknown fields,
unknown enum values, duplicate IDs, broken references, and dependency cycles.
Markdown export is the semi-structured reading and prompt handoff.

```json
{
  "schema_version": 1,
  "title": "Planning feature prototype",
  "status": "in_review",
  "objective": "Create an actionable, reviewable planning workflow.",
  "outcomes": [{
    "id": "outcome-alignment",
    "description": "Humans and agents share one implementation plan.",
    "proof": ["The strict plan check and browser review both pass."]
  }],
  "constraints": [{
    "id": "constraint-static",
    "description": "The built artifact remains self-contained."
  }],
  "non_goals": ["Replace the issue tracker."],
  "decisions": [{
    "id": "decision-canonical-data",
    "title": "Keep execution data separate from its visual projection",
    "status": "accepted",
    "rationale": "Agents need strict data while humans need visual explanation."
  }],
  "workstreams": [{
    "id": "workstream-cli",
    "title": "CLI workflow",
    "status": "in_progress",
    "owner": "implementation-agent",
    "tasks": [{
      "id": "task-plan-check",
      "title": "Validate actionable plan data",
      "status": "todo",
      "owner": "implementation-agent",
      "outcomes": ["outcome-alignment"],
      "dependencies": [],
      "files": ["src/lib.rs", "src/main.rs", "tests/cli.rs"],
      "acceptance_checks": ["Broken IDs and dependency cycles fail."],
      "verification_commands": ["cargo test plan_ --test cli"]
    }]
  }],
  "risks": [{
    "id": "risk-overbuilt",
    "description": "Planning grows into project management.",
    "likelihood": "medium",
    "impact": "high",
    "mitigation": "Keep v1 local, static, and focused on alignment."
  }]
}
```

Schema rules:

- All IDs are stable, lowercase, unique within a plan, and safe for HTML anchors.
- References must resolve: task outcomes and dependencies are checked.
- Status values are closed enums. Unknown statuses fail `plan check`.
- Task file paths are repository-relative when the plan lives inside a repo.
- Task acceptance checks state observable completion conditions; verification
  commands state how an implementation agent should prove them.
- The prototype keeps evidence/status history out of v1 until real usage proves
  the right shape rather than preemptively becoming a project tracker.

## CLI workflow

Prototype commands should be explicit and mirror existing Sideshow habits:

```sh
sideshow plan new my-plan --theme signal
sideshow plan check my-plan
sideshow build my-plan
sideshow plan serve my-plan --open
sideshow plan export my-plan --format markdown --output IMPLEMENTATION_PLAN.md
```

Command contract:

- `sideshow plan new DIR` creates an ordinary deck plus strict `DIR/plan.json`.
  Existing files are never overwritten.
- `sideshow plan check DIR` validates schema, references, enum values, required
  fields, budgets, unsafe markup, and projection-readiness.
- `sideshow build DIR` remains the build command and emits the normal
  self-contained deck artifact.
- `sideshow plan serve DIR` reuses the existing live rebuild server and enables
  review markup by default. Serving must not mutate canonical plan data.
- `sideshow plan export DIR --format json|markdown` exports normalized data or a
  human-readable status report. JSON export is the stable integration boundary.

Plan-specific commands exist where they add semantics. Build remains shared so a
plan is still a normal, expressive Sideshow deck rather than a second renderer.

## Static component and theme architecture

Plan projections should be built from deterministic static components, not
bespoke generated markup per plan.

- Components render server/build-time HTML with semantic elements first.
- CSS uses Sideshow theme tokens and a plan-specific component layer.
- Components carry `data-plan-kind`, `data-plan-id`, and optional
  `data-plan-status` attributes.
- Visual state derives from schema fields, never from CSS class names alone.
- The theme can change color, density, typography, and card treatment without
  changing canonical data.
- The build should produce a single self-contained HTML file like decks do.

Suggested component taxonomy:

- `PlanHeader`: title, owner, status, updated date, summary.
- `OutcomeList` / `OutcomeCard`: actionable outcomes with acceptance coverage.
- `ConstraintPanel`: accepted/proposed constraints grouped by type.
- `DecisionLog`: decisions, rationale, date, supersession.
- `WorkstreamBoard`: workstreams with task rollups and status.
- `TaskCard`: task owner, priority, status, outcomes, files, acceptance,
  verification, blockers.
- `DependencyMap`: dependency list or diagram with unresolved references called
  out by `plan check` before build.
- `FileImpactList`: files grouped by role and linked tasks.
- `VerificationPanel`: commands, evidence, and pending checks.
- `RiskRegister`: severity/likelihood/status/mitigation.
- `StatusTimeline`: dated status entries and blockers.
- `ReviewAnchors`: stable invisible or visible anchors for comments and markup.

The vocabulary must not collapse into a wall of cards. Components should also
provide visually distinct relationship patterns: rails for ordered progress,
lanes for parallel ownership, layers for system boundaries, connectors for
dependencies, typographic emphasis for a single decision, and compact inline
evidence. Card containers are one primitive, not the default answer to every
planning concept.

Verification needs progressive disclosure. The human projection should lead with
the proof strategy (for example, schema rejection coverage, browser interaction,
or migration rollback), while exact commands remain available in an expandable
detail and are preserved verbatim in the structured implementation-agent export.

## Diagram strategy

Diagrams should be HTML/CSS-first in v1:

- Use lists, cards, CSS grid, columns, and connecting labels before generated
  graphics.
- Prefer readable dependency tables over fragile auto-layout when plans are small.
- Render simple swimlanes and boards as semantic HTML so they remain accessible,
  searchable, printable, and easy for agents to inspect.
- Allow static inline SVG generated at build time as an optional follow-up for
  dependency graphs or timelines once the source data is stable.
- If graph layout is added, it must be deterministic, offline, and produce static
  markup; no client-side graph framework is required for v1.

## Review, markup reuse, and trust boundary

Reuse Sideshow's existing review and markup ideas where they fit:

- Generated review output should expose stable anchors and source references.
- Existing static HTML checks, forbidden tag handling, asset inlining, print CSS,
  reduced-motion behavior, and review overlays should be reused instead of
  duplicated.
- Review markup is commentary, not authority. Accepted changes are applied to
  `plan.json` and, when appropriate, authored slides; annotations never silently
  change the canonical plan.
- The trust boundary is the same as deck fragments: plan data and notes are local
  source; generated output must not execute untrusted scripts, fetch remote code,
  or grant extra filesystem/network privileges.

## Guardrail philosophy

Guardrails should prevent ambiguous or unsafe artifacts without making small
plans painful:

- Fail on broken references, duplicate IDs, invalid statuses, unsafe HTML, and
  output budget violations.
- Warn on missing owners, stale `updated` dates, tasks without verification,
  outcomes without acceptance criteria, and risks without mitigation.
- Keep escape hatches explicit: `--strict` promotes warnings to failures;
  `--allow-warnings` may be used in prototype scripts but should be visible.
- Prefer actionable diagnostics that name the record ID and field.

## Dependency policy

Careful dependencies are allowed. The feature should not contort itself to avoid
small, high-value crates, but it must avoid large JavaScript build chains.

- Rust crates for schema validation helpers, static HTML escaping,
  and deterministic graph layout are acceptable after size/maintenance review.
- No npm, Vite, React, client-side graph runtime, or browser bundler in v1.
- Any optional renderer must be build-time, offline, deterministic, and covered by
  static checks.
- Prefer dependencies already present in the repository when they meet the need.

## Budgets

Initial prototype budgets should be conservative and enforceable by
`sideshow plan check`:

- Plan source: `plan.json` <= 512 KiB.
- Records: <= 200 tasks, <= 50 outcomes, <= 50 workstreams, <= 400 dependencies.
- Assets: reuse existing per-asset and total inlined asset budgets where possible.
- Output HTML: target <= 5 MiB without embedded media; warn above 2 MiB.
- Runtime JavaScript: none required beyond existing review/navigation helpers.
- Build time: target under 1 second for a typical 50-task plan on a developer
  laptop, excluding image optimization.

## Accessibility and security

- Render semantic headings, lists, tables, and landmarks before visual flourishes.
- Preserve keyboard navigation and print/PDF readability.
- Respect `prefers-reduced-motion` and avoid required animation.
- Do not encode status by color alone; include text labels and accessible names.
- Escape plan fields whenever they are projected into generated markup. Authored
  slides continue through the existing fragment and Markdown rules.
- Reject `<script>`, event-handler attributes, remote styles/scripts, and unsafe
  SVG content in plan projections.
- Keep generated review pages self-contained and offline-capable.

## Prototype scope

The first implementation should deliver:

1. `sideshow plan new` with a minimal valid `plan.json` and visual deck scaffold.
2. `sideshow plan check` with schema, enum, ID, reference, budget, and unsafe
   markup validation.
3. Shared `sideshow build` plus `sideshow plan serve`, with served review enabled
   by default and no review state in built output.
4. JSON export of normalized schema v1.
5. Documentation and one example plan used by tests.

Explicitly defer:

- Visual editing.
- Hosted collaboration.
- Client-side graph libraries.
- Automatic scheduling/resource leveling.
- Full slide-deck projection, except where the review page can reuse existing
  Sideshow components cheaply.

## Future 2D fit

The schema should not assume a single linear deck. Workstreams, outcomes, and
dependencies already form a graph. Future 2D projection can map:

- horizontal axis: outcomes, phases, or workstreams;
- vertical axis: drill-down detail, risk/verification views, or alternate
  audience layers;
- edges: dependencies and decision lineage.

This is a projection concern. Schema v1 should preserve enough stable IDs and
relationships for 2D layouts without exposing a 2D authoring model in the
prototype CLI.

## Open questions

- Should freeform narrative remain only in slides, or should schema v2 gain an
  optional Markdown context file for dense handoffs?
- Should acceptance and verification become independently status-bearing records
  after execution tracking is proven useful?
- Should schema v2 represent verification as `{ intent, commands, evidence }` so
  human projections do not have to infer meaning from shell invocations?
- What status vocabulary best matches todo.sr.ht and Linear without coupling to
  either service?
- Should `sideshow check` auto-detect `plan.json`, or should all plan validation
  require `sideshow plan ...` subcommands?
- Which existing review overlay pieces are generic enough to reuse directly, and
  which need a small shared abstraction?
- Do dependency diagrams need a build-time graph layout dependency in v1, or are
  semantic tables and swimlanes enough for the prototype?
- How should source locations be reported for JSON records so GitHub/sr.ht review
  comments can land on the right lines?
- What import/export bridges are worth adding first: JSON only, Markdown status,
  todo.sr.ht issues, Linear issues, or GitHub task lists?
