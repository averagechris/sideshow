# Planning Sideshow example

This is a real dogfood deck for ticket #166 Phase 2, not a synthetic fixture. It
keeps the canonical planning contract in `plan.json` (`schema_version: 2`) and
uses authored slides as stable projections over that contract.

The example is an alignment artifact. Its loop ends when a human or agent has a
reviewed digest suitable for drafting work in the team's issue tracker. It does
not synchronize execution status or create tracker issues. A later demo or
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
plan data or the repository-local commands documented here.

## Organizational handoff

Use the exported JSON or Markdown as input while manually drafting appropriately
shaped issues in Linear, todo.sr.ht, GitHub Issues, or another team system. The
author or agent remains responsible for issue boundaries, labels, ownership, and
tracker-specific conventions. Sideshow intentionally has no tracker credentials,
imports, or synchronization contract.
