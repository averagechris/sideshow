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

## Repeatable loop

From the repository root:

1. Edit the source of truth:
   - `examples/planning-sideshow/plan.json`
   - `examples/planning-sideshow/slides/*.html`
2. Strict-check the canonical plan and slide anchors:

   ```sh
   cargo run -- plan check examples/planning-sideshow --strict
   # or, from a Nix shell wrapper:
   nix develop -c cargo run -- plan check examples/planning-sideshow --strict
   ```

3. Build the deck:

   ```sh
   cargo run -- build examples/planning-sideshow
   nix develop -c cargo run -- build examples/planning-sideshow
   ```

   Generated HTML goes under `examples/planning-sideshow/dist/`. Use it for
   inspection, but do not commit it.

4. Serve and review locally:

   ```sh
   cargo run -- plan serve examples/planning-sideshow --port 8000
   nix develop -c cargo run -- plan serve examples/planning-sideshow --port 8000
   ```

5. Export machine-readable and prompt-oriented handoff files outside the repo:

   ```sh
   export_dir="$(mktemp -d)"
   cargo run -- plan export examples/planning-sideshow --format json --output "$export_dir/plan.json"
   cargo run -- plan export examples/planning-sideshow --format markdown --output "$export_dir/plan.md"
   ```

   A fresh temporary directory matters because plan export intentionally refuses
   to overwrite an existing handoff.

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
