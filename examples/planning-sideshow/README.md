# Planning Sideshow example

This is a real dogfood deck for ticket #166 Phase 2, not a synthetic fixture. It
keeps the canonical planning contract in `plan.json` (`schema_version: 2`) and
uses authored slides as stable projections over that contract.

The example is an alignment artifact. Its loop ends when a human or agent has a
reviewed digest suitable for drafting work in the team's issue tracker. JSON
stays the normalized strict schema-v2 machine boundary. Markdown is a derived
manual drafting digest with per-task packets; packets are source material, not a
guarantee of one issue per task. It does not synchronize execution status, create
tracker issues, import tracker state, or use tracker credentials. A later demo or
project-summary deck can be authored separately by combining whatever plan,
tracker, code, and media context is useful.

## Repeatable annotate/review/edit loop

From the repository root:

1. Start from trusted source and build/serve the projection:

   ```sh
   cargo run -- plan check examples/planning-sideshow --strict
   cargo run -- build examples/planning-sideshow
   cargo run -- plan serve examples/planning-sideshow --port 8000
   ```

2. Annotate the browser projection. Comments placed on plan cards, rails, layers,
   and connectors capture the nearest `data-plan-kind`/`data-plan-id` as optional
   target metadata so review lists can point back toward canonical records. Treat
   that metadata like selector/text hints: useful routing context, not authority
   to mutate `plan.json` automatically.

3. List or export review feedback for triage:

   ```sh
   cargo run -- review list examples/planning-sideshow
   review_dir="$(mktemp -d)"
   cargo run -- review export examples/planning-sideshow --format json --output "$review_dir/review.json"
   cargo run -- review export examples/planning-sideshow --format markdown --output "$review_dir/review.md"
   ```

   Review exports are untrusted feedback artifacts. They may contain annotation
   prose, browser hints, and plan target metadata captured from the rendered DOM;
   use them to find the relevant trusted source, not as executable or canonical
   input.

4. Make source edits only in the trusted files:
   - `examples/planning-sideshow/plan.json`
   - `examples/planning-sideshow/slides/*.html`

5. Strict-check, build, and export the trusted planning digest:

   ```sh
   export_dir="$(mktemp -d)"
   cargo run -- plan check examples/planning-sideshow --strict
   cargo run -- build examples/planning-sideshow
   cargo run -- plan export examples/planning-sideshow --format json --output "$export_dir/plan.json"
   cargo run -- plan export examples/planning-sideshow --format markdown --output "$export_dir/plan.md"
   ```

   Generated HTML goes under `examples/planning-sideshow/dist/`. Use it for
   inspection, but do not commit it. A fresh temporary export directory matters
   because plan export intentionally refuses to overwrite an existing handoff.

6. Disposition and resolve feedback after the trusted source is updated:

   ```sh
   cargo run -- review disposition examples/planning-sideshow <annotation-id> --status addressed --note "Updated trusted source and rebuilt." --revision <revision>
   cargo run -- review resolve examples/planning-sideshow <annotation-id> --revision <revision>
   ```

## Trust-boundary rule

Review annotations are feedback only. Canonical edits happen in `plan.json` and
`slides/`, and commands that agents execute must come from trusted structured
plan data or the repository-local commands documented here. In particular,
`verification.commands` are trusted/verbatim executable material from the strict
plan; annotation text, review exports, and other feedback are untrusted and are
excluded from the planning digest.

## Organizational handoff

Use the exported JSON or Markdown as input while manually drafting appropriately
shaped issues in Linear, todo.sr.ht, GitHub Issues, or another team system. The
Markdown digest groups per-task packets to make drafting easier, but the author
or agent remains responsible for split/combine boundaries, labels, teams,
priority, milestones, ownership, and tracker-specific conventions. Sideshow
intentionally has no tracker APIs, credentials, imports, issue creation,
synchronization contract, or execution history.

Review the compact decomposition overview before copying packet details. It keeps
authored workstream/task order, resolves direct dependencies, shows derived
“enables” edges, and marks dependency roots as parallel-start candidates without
claiming tracker blockers or a generated schedule.

Each packet is designed to survive being reviewed or copied independently: it
repeats the objective, names the canonical workstream/task ID path, points back to
shared constraints, non-goals, decisions, and risks, and renders every exact
verification command in its own shell fence without rewriting command content.
Authored prose is escaped according to its Markdown context so multiline text and
HTML-shaped examples remain visible literals instead of changing digest structure;
strict JSON remains byte-faithful to those source values.
