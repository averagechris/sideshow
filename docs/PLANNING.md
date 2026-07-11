# Sideshow planning feature design

This document is the implementation contract for a planning mode that turns
human/agent/team intent into structured plan data and a reviewable static visual
projection. It is intentionally narrower than a general project-management app.

## Goals

- Capture the alignment artifact that already emerges from human-agent work:
  outcomes, constraints, decisions, proposed workstreams, dependencies, touched
  files, acceptance criteria, verification intent, risks, and planning maturity.
- Keep the canonical plan machine-readable and diff-friendly while allowing many
  projections: HTML review pages, slides, diagrams, exports, and future 2D maps.
- Give humans and agents a shared lifecycle: frame, explore, optionally prototype,
  align, refine, share, and digest into the team's execution system.
- Reuse Sideshow's existing static build, review, markup, theming, accessibility,
  and asset guardrails wherever the contracts match.
- Make the prototype useful without a database, hosted service, browser editor,
  or large JavaScript build chain.

## Non-goals

- Replacing issue trackers, Linear, todo.sr.ht, GitHub Projects, or source-control
  history.
- Directly integrating with issue trackers, importing their state, or keeping a
  plan synchronized with execution after handoff.
- Driving the complete development loop or becoming the live source of task
  assignment, scheduling, implementation status, blockers, or completion evidence.
- Inventing a live collaborative editor or a hosted planning service.
- Making the visual projection the source of truth.
- Running arbitrary user JavaScript inside plan review output.
- Solving all diagram layout automatically in v1.
- Shipping a full dependency graph engine, scheduling optimizer, or resource
  planner in the prototype.

## Human-agent-team alignment lifecycle

Planning is an alignment loop, not a one-time generated document and not a
project-execution loop:

1. **Frame**: human names the desired outcome, boundaries, audience, and known
   constraints.
2. **Explore**: humans and agents develop alternatives, proposed workstreams,
   dependencies, risks, decisions, and verification intent.
3. **Prototype when useful**: build only enough to test an uncertain assumption
   or make an important tradeoff concrete, then record what was learned.
4. **Align**: human and team review a static projection. Comments may point at
   plan IDs, rendered cards, or source lines, but accepted changes update the
   canonical data.
5. **Refine and share**: accepted feedback sharpens the proposal until the team
   has enough context and confidence to decide what should enter execution.
6. **Digest**: a human or agent uses the structured plan and exports to draft
   appropriately shaped issues in Linear, todo.sr.ht, GitHub Issues, or another
   organizational system. Sideshow supplies the building blocks; it does not call
   tracker APIs, import tracker state, or synchronize the two artifacts.
7. **Conclude planning**: after handoff, the issue tracker owns assignment,
   priority, scheduling, implementation status, blockers, and completion. The
   plan remains a durable rationale and alignment record, not a competing ledger.

The lifecycle should be visible in the data model. A plan is not merely a slide
deck; it is a working agreement with traceable intent and proposed execution
shape. Status fields describe planning maturity or the state proposed at handoff,
not an obligation to mirror live tracker state.

## Canonical data vs visual projection

The canonical source is structured plan data under the deck directory, not the
generated HTML or review artifact. The prototype uses:

```text
my-plan/
  plan.json        # canonical alignment and proposed-work data
  deck.toml        # projection metadata
  theme.css        # forkable visual identity
  slides/          # authored human-facing projection
  assets/          # optional local assets
  dist/            # generated output, never edited by hand
```

Rules:

- `plan.json` owns IDs, planning state, proposed relationships and files,
  acceptance checks, and verification intent/commands for a future implementer.
- Slides explain the plan to humans and may be deliberately more expressive,
  but must not contain the only copy of an actionable task or dependency.
- Generated HTML is disposable. Authored slides remain normal deck source.
- Every rendered component that represents data must carry stable `data-plan-id`
  and `data-plan-kind` attributes so reviews can map comments back to source.
- Manual edits to generated output are unsupported and should be overwritten by
  the next build.

## Schema v2

Use strict normalized JSON for the prototype because it is the direct,
unambiguous machine boundary for humans and agents. The checked schema rejects
unknown fields, unknown enum values, duplicate IDs, broken references, and
dependency cycles. Markdown export is derived from that JSON as a manual drafting
digest: readable per-task packets that preserve context for issue writing, not a
second source of truth and not a promise that packets map one-to-one to tracker
issues.

```json
{
  "schema_version": 2,
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
    "title": "Keep proposed work separate from its visual projection",
    "status": "accepted",
    "rationale": "Issue drafting needs strict data while humans need visual explanation."
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
      "verification": {
        "intent": "Prove the CLI rejects malformed plans and preserves exact agent commands in canonical JSON.",
        "commands": ["cargo test plan_ --test cli"]
      }
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
- Status values are closed enums. Unknown statuses fail `plan check`; these values
  express planning maturity and proposed state, not synchronized tracker status.
- Task file paths are repository-relative when the plan lives inside a repo.
- Task acceptance checks state observable completion conditions; verification is
  a required object with nonblank human-readable `intent` and at least one
  nonblank verbatim agent command in `commands`.
- Unknown fields are rejected. The old `verification_commands` task field is not
  accepted by schema v2; keep command strings exactly as the agent should run
  them.
- The Markdown export separates `Verification intent` from `Agent commands` so
  reviewers understand purpose without weakening exact command handoff. Command
  strings remain trusted/verbatim executable material copied from canonical plan
  data; review annotations and other untrusted feedback are never included in the
  digest as commands or plan facts.
- Each manual drafting packet repeats the plan objective, identifies its stable
  canonical record path by workstream and task ID, and points back to plan-level
  constraints, non-goals, decisions, and risks. Exact commands appear in authored
  order in individually fenced shell blocks so whitespace, multiline commands,
  shell metacharacters, and backticks survive copy/paste without normalization.
- The plan deliberately keeps execution evidence and status history out of v2.
  Those belong in the eventual issue tracker, code review, CI, and delivery
  systems rather than a second project tracker.

## CLI workflow

Prototype commands should be explicit and mirror existing Sideshow habits:

```sh
sideshow plan new my-plan --theme signal
sideshow plan check my-plan
sideshow build my-plan
sideshow plan serve my-plan --open
sideshow plan export my-plan --format markdown --output PLAN_DIGEST.md
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
  human-readable planning digest. JSON is a stable tool boundary that a human or
  agent may use while drafting tracker issues; it is not a tracker integration.

Plan-specific commands exist where they add semantics. Build remains shared so a
plan is still a normal, expressive Sideshow deck rather than a second renderer.
No command authenticates to, reads from, or writes to an issue tracker. Agents can
combine exports with separately obtained organizational context using their own
tools and judgment.

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
- `WorkstreamBoard`: proposed workstreams and their relationships.
- `TaskCard`: proposed owner, outcomes, files, acceptance, and verification.
- `DependencyMap`: dependency list or diagram with unresolved references called
  out by `plan check` before build.
- `FileImpactList`: files grouped by role and linked tasks.
- `VerificationPanel`: proof intent and suggested implementation commands.
- `RiskRegister`: severity/likelihood/status/mitigation.
- `AlignmentRail`: questions, decisions, prototype findings, and readiness to
  digest into external issues.
- `ReviewAnchors`: stable invisible or visible anchors for comments and markup.

The vocabulary must not collapse into a wall of cards. Components should also
provide visually distinct relationship patterns: rails for ordered progress,
lanes for parallel ownership, layers for system boundaries, connectors for
dependencies, typographic emphasis for a single decision, and compact inline
evidence. Card containers are one primitive, not the default answer to every
planning concept.

Verification needs progressive disclosure. The human projection should lead with
the proposed proof strategy (for example, schema rejection coverage, browser
interaction, or migration rollback), while suggested exact commands remain
available in an expandable detail and are preserved verbatim for issue drafting
or a future implementation agent.

## Handoff to organizational systems

The plan ends at a reviewed digest, not at automated issue creation. Sideshow
provides strict source, stable IDs, Markdown/JSON exports, expressive slides, and
review guardrails. A human or agent can use those pieces to draft work in the
appropriate system:

- Linear for an organization's internal execution;
- todo.sr.ht for SourceHut projects an owner maintains;
- GitHub Issues for projects that use GitHub, including external contributions;
- another tracker or written process chosen by the team.

The digest should carry enough context to make that translation reliable:
outcomes, rationale, scope, proposed decomposition, dependencies, touched files,
acceptance checks, risks, and verification intent. Its Markdown form is a set of
per-task packets for manual drafting. Those packets are source material, not
tracker records: the translator still chooses split/combine boundaries, labels,
teams, priority, milestones, and tracker-specific conventions. Direct tracker
adapters, credentials, imports, issue creation, synchronization, and execution
history are outside the product contract.

Packets use stable plan IDs rather than tracker identifiers or inferred source
line numbers. A canonical source reference such as `plan.json → workstream
ws-delivery → task task-align` remains meaningful across formatting changes and
directs accepted feedback back to structured source.

Links to resulting issues may be added to ordinary narrative source when useful,
but Sideshow does not need a tracker-state model. Once execution begins, the
tracker and code-hosting systems are authoritative for what is actually happening.

## After the work

A project summary, demo, retrospective, or “how we built it” story is a natural
Sideshow deck, but it is a separate authoring activity rather than a continuation
of the planning state machine. An agent or author may manually combine the
original plan, tracker state, code changes, screenshots, demos, and lessons using
ordinary deck and planning components.

Sideshow only needs the composable building blocks: static slides, diagrams,
evidence disclosure, local assets, review, exports, and visual QA. It does not
need to ingest the original plan, query trackers, inspect repositories, or encode
the synthesis workflow itself.

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
- Warn on missing proposed owners, tasks without verification, outcomes without
  proof, and risks without mitigation.
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
4. JSON export of normalized schema v2.
5. Documentation and one example plan used by tests.

Explicitly defer:

- Visual editing.
- Hosted collaboration.
- Client-side graph libraries.
- Automatic scheduling/resource leveling.
- Tracker API integrations, tracker imports, and live synchronization.
- Execution evidence/history and implementation status tracking after handoff.
- Automatic ingestion of plans, trackers, or repositories for retrospective decks.
- Full slide-deck projection, except where the review page can reuse existing
  Sideshow components cheaply.

## Future 2D fit

The schema should not assume a single linear deck. Workstreams, outcomes, and
dependencies already form a graph. Future 2D projection can map:

- horizontal axis: outcomes, phases, or workstreams;
- vertical axis: drill-down detail, risk/verification views, or alternate
  audience layers;
- edges: dependencies and decision lineage.

This is a projection concern. Schema v2 should preserve enough stable IDs and
relationships for 2D layouts without exposing a 2D authoring model in the
prototype CLI.

## Open questions

- Should freeform narrative remain only in slides, or should a future schema gain an
  optional Markdown context file for dense handoffs?
- Which tracker-neutral digest shape best helps an agent or human draft issues
  without encoding Linear, todo.sr.ht, or GitHub conventions?
- Should planning status vocabulary be simplified further now that it explicitly
  does not mirror execution state?
- Should `sideshow check` auto-detect `plan.json`, or should all plan validation
  require `sideshow plan ...` subcommands?
- Which existing review overlay pieces are generic enough to reuse directly, and
  which need a small shared abstraction?
- Do dependency diagrams need a build-time graph layout dependency in v1, or are
  semantic tables and swimlanes enough for the prototype?
- How should source locations be reported so review comments and manually created
  issues can refer back to the right planning record?
- What export presentation best supports a reviewed, manual issue-drafting step
  while keeping JSON tracker-neutral?
