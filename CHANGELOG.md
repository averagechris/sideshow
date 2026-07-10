# Changelog

## Unreleased

- Persist local review annotations in XDG state as schema v2 tool-neutral JSON
  artifacts keyed by canonical deck root, with revision/ETag conflict handling,
  CLI artifact/list/export/clear/resolve/reopen/disposition commands, and
  JSON-first plus prompt-Markdown handoff exports that cannot be written into the
  deck. The served review panel honors server-derived freshness and surfaces
  explicit non-pending dispositions with their optional notes.

## v0.4.0 - 2026-07-10

- Add opt-in `sideshow serve --review` controls for point and logical-region
  annotations with optimistic multi-tab conflict handling. Review UI and state
  are confined to local served responses and never enter build artifacts.

## v0.3.0 - 2026-07-09

- Parse HTML asset references structurally, including `srcset`, while confining
  local reads to the deck's `assets/` directory.
- Inline safe direct SVG image sources as sanitized, deck-wide namespaced markup;
  unsupported SVGs retain passive data-URI image behavior.
- Watch deck inputs during `sideshow serve` and reload served browser tabs after
  successful, atomically written rebuilds.
- Warn from `sideshow check` about orphaned assets and hardcoded fragment colors.
- Remove the generated Tailwind banner while preserving unrelated license
  comments.
## v0.2.0 - 2026-07-08

- Dual-license under MIT OR Apache-2.0 (LICENSE, LICENSE-MIT, LICENSE-APACHE).
- Added `sideshow publish` with S3 presigned upload (`--target s3`) and direct SourceHut Pages publishing (`--target srht --domain <domain>`, subdir-scoped so existing site content is untouched).
## v0.1.0 - 2026-07-06

### Added

- Added the `sideshow-deck-author` agentic skill pack with fragment patterns and
  the canonical build/check/rodney verification workflow.
- Implemented `sideshow new` and `sideshow build` with deck parsing, slide
  wrapping, Tailwind CSS compilation, embedded runtime/theme assets, asset
  inlining, and core tests.
- Added the `window.sideshow` automation API, including navigation, notes, and
  defensive slide audit reporting.
- Added `sideshow check` static linting and `sideshow serve` local SourceHut
  Pages-like serving with rebuild-on-request.
- Added `ledger`, `terminal`, and `poster` built-in themes plus `sideshow themes`
  metadata output for human and JSON theme selection.
- Added `sideshow img` image inspection, resize, crop, and only-if-smaller WebP
  optimization tools.
- Added build-time in-memory raster optimization via `[images]` deck config plus
  SVG data-URI guidance notes.
- Added `sideshow check` warning-level image size budgets and `--strict` warning
  promotion.
- Initial project scaffold.

### Fixed

- Deduplicated forbidden fragment tag findings so `<script></script>` reports a
  single `fragment_contract` error.
