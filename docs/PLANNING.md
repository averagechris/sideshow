# Sideshow planning feature design

This document is the implementation contract for a planning mode that turns
human/agent/team intent into structured plan data and a reviewable static visual
projection. It is intentionally narrower than a general project-management app.

## Goals

These three goals are co-equal; do not optimize one by weakening the others:

- **Human-agent alignment:** preserve a shared understanding of purpose,
  constraints, assumptions, narrative, risks, and readiness before work enters an
  execution system.
- **Convenient anchored feedback:** make comments easy to place on rendered plan
  concepts and easy to route back to trusted source via stable IDs and source
  references.
- **Team-wide mental-model distribution:** produce a portable static artifact and
  digest that let people who were not in the authoring loop quickly understand the
  proposed shape of the work.

Supporting goals:

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

## Planning-authoring ownership model

The agent owns the judgment-heavy authoring work: interviewing, synthesis,
narrative, audience adaptation, roleplay assumptions, and visual reasoning. The
Sideshow skill guides that frame of mind; it should prompt the agent to ask for an
alignment contract, test assumptions, design the narrative/review path before
choosing visuals, run a cold-reader pass, and ask explicit feedback questions.

Sideshow CLI owns deterministic structure and transport: schema, mutation,
registry discovery, component binding, rendering, checking, review artifact
handling, export, and distribution. It must not judge whether a story is
persuasive, whether the audience will care, or whether the narrative quality is
high. Those calls remain with the human/agent authoring loop.

An alignment contract should name audience, decision needed, non-goals,
assumptions to roleplay when answers are missing, required evidence, review
questions, and what kind of feedback should be anchored in the deck versus handled
as out-of-band discussion.

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
  "objective": "Create an alignment-first, reviewable planning workflow.",
  "outcomes": [{
    "id": "outcome-alignment",
    "description": "Humans and agents share one proposal-oriented planning digest.",
    "proof": ["The strict plan check and browser review both pass."]
  }],
  "constraints": [{
    "id": "constraint-static",
    "description": "The built artifact remains self-contained."
  }],
  "non_goals": ["Replace the issue tracker.", "Model live assignment, blocker, status, or completion state.", "Assume plan tasks map 1:1 to issues."],
  "decisions": [{
    "id": "decision-canonical-data",
    "title": "Keep proposed work separate from its visual projection",
    "status": "accepted",
    "rationale": "Issue drafting needs strict data while humans need visual explanation."
  }],
  "workstreams": [{
    "id": "ws-alignment",
    "title": "Alignment workflow",
    "status": "in_progress",
    "owner": "planning-agent",
    "tasks": [{
      "id": "task-align",
      "title": "Validate alignment plan data",
      "status": "todo",
      "owner": "planning-agent",
      "outcomes": ["outcome-alignment"],
      "dependencies": [],
      "files": ["src/lib.rs", "src/main.rs", "tests/cli.rs"],
      "acceptance_checks": ["Broken IDs and dependency cycles fail."],
      "verification": {
        "intent": "Prove the CLI rejects malformed plans and preserves exact planning commands in canonical JSON.",
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
- `plan mutate` provides atomic typed CRUD for stable-ID record kinds: outcomes,
  constraints, decisions, workstreams, tasks, and risks, plus a full typed
  `update-plan` for title/status/objective. Mutations take a lock, validate the
  resulting schema-v2 plan semantically, write canonical pretty JSON only after a
  valid candidate exists, and print stdout that byte-for-byte matches the written
  `plan.json`.
- Non-goals are intentionally edited only through full JSON authoring for now:
  schema v2 stores them as strings with no stable IDs. Dedicated nested task-list
  mutations are also unnecessary because full typed `update-task` covers outcomes,
  dependencies, files, acceptance checks, and verification commands.
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
- Authored non-command strings are rendered as literal Markdown content using
  context-aware heading, paragraph, inline, and list-item escaping. Newlines remain
  readable without allowing plan prose to create extra headings, lists, links,
  blockquotes, tables, code fences, or active raw HTML. This structural hardening
  does not rewrite canonical JSON and never applies to exact command bytes.
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

- `sideshow plan new DIR` creates an ordinary deck plus strict `DIR/plan.json`
  scaffolded around frame → explore → refine digest semantics. Existing files are
  never overwritten.
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

Mutable record kinds and bindable projection kinds overlap but are not the same
contract. `plan mutate` changes semantic plan records (`outcome`, `constraint`,
`decision`, `workstream`, `task`, `risk`, plus plan metadata); `compose --bind-*`
binds visual components only to bindable record kinds implemented by the
component projection (`outcome`, `constraint`, `decision`, `workstream`, `task`,
and `risk`). Mutation semantic validation proves the canonical plan is coherent;
strict projection checking additionally verifies that authored slides cover the
records expected by reviewers.

## Static component and theme architecture

Plan projection is an extension of ordinary Sideshow authoring, not a second
renderer. The shared registry direction is documented in
[ADR 0001](adr/0001-shared-configurable-authoring-registry.md). Plan projections
should be assembled from the same registered themes and deterministic static
components as ordinary decks, while raw authored HTML and Markdown remain
available as escape hatches.

Authoring paths are first-class choices, not a maturity ladder with only one
approved endpoint:

- raw HTML/CSS for bespoke layout, diagrams, and one-off visual reasoning;
- Markdown for content-shaped prose slides;
- bundled registered components when their cognitive contract fits;
- project-local registered components when a team has a repeated local pattern.

When a raw pattern recurs, prefer: raw slide → reviewed local pattern →
project-local registered component → possible bundled incubation after repeated
cross-project use. Incubation is a product/design decision, not an automatic
promotion.

- The current built-in themes, scaffold HTML, and component CSS should move into
  a bundled default configuration pack rather than remain planning-only special
  cases.
- Registry metadata maps human or agent intent to components, accepted inputs,
  presets, and CLI composition operations.
- Components render build-time HTML with semantic elements first. Project-defined
  component HTML/CSS must be deterministic, confined, and checked; presentation
  JavaScript is explicit and constrained rather than implicitly trusted.
- CSS uses shared Sideshow theme tokens plus component layers.
- Components carry `data-plan-kind`, `data-plan-id`, and optional
  `data-plan-status` attributes.
- Visual state derives from schema fields, never from CSS class names alone.
- The theme can change color, density, typography, and card treatment without
  changing canonical data.
- The build should produce a single self-contained HTML file like decks do.
- Planning adds typed `plan.json` mutation and component-binding semantics,
  strict validation, anchors, and exports. It does not add a separate registry,
  theme system, deck renderer, or feedback overlay.
- Stage/navigation/audit code and the served feedback overlay JavaScript remain
  fixed compiler runtime infrastructure and cannot be replaced by configured
  presentation resources.

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

Before the detailed packets, Markdown includes a compact decomposition overview
in authored workstream/task order. It resolves direct dependencies, derives
reverse “enables” edges, and identifies dependency roots as parallel-start
candidates. This is a review projection only: it does not topologically reorder
work, invent scheduling policy, create tracker blockers, or add derived fields to
strict JSON.

Packets use stable plan IDs rather than tracker identifiers or inferred source
line numbers. A canonical source reference such as `plan.json → workstream
ws-alignment → task task-align` remains meaningful across formatting changes and
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
- Authors may add ordered `review_questions` to `plan.json` when they want to
  direct reviewer attention. Each question has a stable kebab-case `id`, question
  text, optional kebab-case tags, and a deterministic target: the complete deck,
  an existing
  authored slide source in deck slide order (`slides/*.html`, `slides/*.md`, or
  component `slides/*.slide.toml`) or an existing canonical plan record
  (`outcome`, `constraint`, `decision`, `workstream`, `task`, or `risk`). These
  questions are trusted authored source and are preserved in plan JSON/Markdown
  exports and review handoff trusted context. Invalid trusted questions fail
  export rather than disappearing silently.
- Review annotations may still carry optional plan association hints, but those
  hints remain inside `UNTRUSTED_REVIEW_ARTIFACT`; they never override authored
  `review_questions` or become trusted source.
- During `sideshow plan serve`, the served-only review runtime fetches authored
  prompts from `GET /__sideshow/review/questions`, a read-only endpoint separate
  from the mutable annotation artifact and absent from ordinary build output.
  The UI labels these as **Trusted authored prompt** and labels reviewer text as
  untrusted answers/annotations. Prompt visibility is target-aware: deck targets
  remain visible throughout review, slide targets match the active slide source
  path, and plan-record targets match canonical
  `kind/id` anchors visible on the active slide. If a canonical record appears
  more than once, the question target remains that canonical record while any
  answer is still disambiguated by the captured `slide_id`, `source_path`, and
  nearest visible anchor hints. Starting an annotation from a prompt stores only
  optional untrusted `question_id` association metadata; it never modifies or
  promotes reviewer answers into trusted plan source.
- The trust boundary is the same as deck fragments: plan data and notes are local
  source; generated output must not execute untrusted scripts, fetch remote code,
  or grant extra filesystem/network privileges.
- The current review artifact retains only the latest build manifest. Review
  revision numbers protect concurrent mutation; they do not let a reviewer reopen
  a prior rendered plan. A future bounded review-only revision bundle may preserve
  accepted rendered builds for comparison, but it must remain separate from final
  plan/deck distribution and must keep all reviewer data untrusted.
- Review annotations may target the complete deck without inventing slide identity.
  Ordinary decks may author trusted deck/slide prompts under `[[review.questions]]`
  in `deck.toml`; `plan_record` targets remain exclusive to `plan.json`. Duplicate
  IDs across those trusted sources fail export. Answers and associations remain
  untrusted schema-v2 artifact data.

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
  slides continue through the existing fragment and Markdown rules. Markdown
  exports must also preserve their generated structure when authored prose
  contains Markdown punctuation, multiline text, or HTML-shaped strings.
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
- Should planning status vocabulary be simplified further now that it explicitly
  does not mirror execution state?
- Should `sideshow check` auto-detect `plan.json`, or should all plan validation
  require `sideshow plan ...` subcommands?
- What constrained runtime template contract should project-local registered
  components use, while compile-time compiler templates continue to use Askama?
- Do dependency diagrams need a build-time graph layout dependency in v1, or are
  semantic tables and swimlanes enough for the prototype?
- What bounded review revision format can preserve exact older renderings for
  comparison without leaking them into final published output or growing local
  state without limit?
- Should the shared review model add an explicit deck-wide annotation target and a
  trusted ordinary-deck question source alongside plan `review_questions`?
