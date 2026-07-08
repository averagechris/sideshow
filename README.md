# sideshow

Agentic HTML slide deck compiler and toolkit

## Usage

Create a deck source directory, edit the fragments, then build one
self-contained HTML file:

```sh
sideshow new mydeck --theme signal
$EDITOR mydeck/slides/01-title.html mydeck/slides/02-content.md
sideshow build mydeck
open <path printed by sideshow build>
```

`sideshow build` reads `deck.toml`, wraps `slides/*.html` and `slides/*.md` as
fixed 1920×1080 slide sections, inlines local `assets/` references as data URIs,
and embeds the stage CSS/runtime JavaScript. Tailwind CSS v4's standalone
`tailwindcss` binary must be on `PATH`; the Nix dev shell provides it.

Lint before building:

```sh
sideshow check mydeck
sideshow check mydeck --format json
```

`check` validates `deck.toml`, slide ordering, forbidden fragment tags, duplicate
generated slide IDs, missing `assets/...` references, empty slide directories, and
image size budgets. Budget findings are warnings by default; use `--strict` to
make warnings fail the check.

## Images

Use `sideshow img` to inspect and prepare raster assets before committing them to
a deck:

```sh
sideshow img info mydeck/assets/photo.png
sideshow img info mydeck/assets/photo.png --format json
sideshow img resize mydeck/assets/photo.png --width 1920
sideshow img crop mydeck/assets/photo.png --rect 1600x900      # centered
sideshow img crop mydeck/assets/photo.png --rect 1600x900+0+0
sideshow img optimize mydeck/assets/photo.png --quality 80 --max-dim 3840
```

`optimize` writes a sibling `<stem>.webp` only when the lossy WebP output is
smaller than the original. `--in-place` deletes the original after a successful
conversion. SVGs are skipped; for diagrams, prefer agent-authored inline SVG
markup in the slide fragment.

Builds also optimize referenced raster images in memory before base64 inlining;
source files under `assets/` are never mutated by `sideshow build`:

```toml
[images]
optimize = true  # default
quality = 80
max_dim = 3840
```

## Built-in themes

List theme selection metadata with `sideshow themes` or `sideshow themes --format json`.
Built-ins: `signal` (dark/cyan balanced), `ledger` (formal paper dense),
`terminal` (dark engineering balanced), and `poster` (bold keynote sparse).

Serve locally with SourceHut Pages-like MIME/CSP headers and rebuild-on-request
when source files change:

```sh
sideshow serve mydeck --port 8000
```

## Visual feedback loop

The deck exposes a small browser automation API at `window.sideshow`. The
canonical rodney loop is:

```sh
sideshow build mydeck
rodney open file://$(pwd)/mydeck/dist/<deck-slug>.html
rodney js 'sideshow.audit()'
rodney screenshot /tmp/sideshow-slide.png
```

Use the exact path printed by `sideshow build`; `mydeck/dist/<deck-slug>.html` is only a placeholder shape.

Useful calls: `sideshow.goto(2)`, `sideshow.next()`, `sideshow.prev()`,
`sideshow.count()`, `sideshow.notes(1)`, and `sideshow.audit()`.

## Agentic skill pack

The top-level `skills/` directory contains the `sideshow-deck-author` skill for agents authoring full decks: content discovery, theme previews, fragment authoring rules, rodney-based visual QA, and delivery guidance.

## Development

```sh
direnv allow   # or: nix develop
nix run . -- --help
nix run .#ci-fmt
nix run .#ci-clippy
nix run .#static-checks
nix run .#ci-test
```

## Examples

`examples/making-of-sideshow/` is a full 14-slide deck about how this tool was
built, authored with the tool itself. Browse previews at
[averagechris.srht.site/sideshow/examples.html](https://averagechris.srht.site/sideshow/examples.html)
or the compiled deck at
[averagechris.srht.site/sideshow/demo.html](https://averagechris.srht.site/sideshow/demo.html).
`docs/pages/demo.html` is that deck's build output, committed so the pages
publisher can serve it; regenerate it after editing the example deck:

```sh
sideshow build examples/making-of-sideshow
cp examples/making-of-sideshow/dist/the-making-of-sideshow.html docs/pages/demo.html
```

## Issues

Bugs and feature requests live on the umbrella tracker at
[todo.sr.ht/~averagechris/projects](https://todo.sr.ht/~averagechris/projects);
sideshow tickets carry the `repo:sideshow` label.

## Release

```sh
nix run .#release -- --version X.Y.Z --submit-linux-build
```

The shared release interface comes from
`git+https://git.sr.ht/~averagechris/averagechris.srht.site#lib.fleet.presets.rust`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
