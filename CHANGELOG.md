# Changelog

## Unreleased

- Dual-license under MIT OR Apache-2.0 (LICENSE, LICENSE-MIT, LICENSE-APACHE).


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
