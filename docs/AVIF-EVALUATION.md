# AVIF raster optimization evaluation

**Decision date:** 2026-07-10  
**Decision:** **No-go for production AVIF encoding for now.** Keep WebP as the
only automatic raster optimization target.

AVIF produced a meaningful win for the representative photograph, but the
current Rust encoder was roughly 20 times slower at equivalent measured quality.
For the representative text-heavy screenshot, AVIF had to be 25% larger than
WebP to meet the same SSIM. Browser coverage is also lower, and a Linux build and
measurement of the candidate encoder could not be completed from the available
Darwin host. That is not enough evidence to add the encoder to every Sideshow
build.

No production image behavior or dependency changed as a result of this
evaluation. In particular, builds still preserve animated GIF/WebP/APNG bytes,
leave source assets untouched, use an optimized image only when it is smaller
than its source, rewrite all existing asset-reference forms in the same way, and
calculate warnings from source asset sizes rather than optimized output.

## Candidate and versions

The candidate is the encoder already exposed by `image` 0.25.10's `avif`
feature. It uses `ravif` 0.13.0 and `rav1e` 0.8.1. This is the closest additive
candidate for Sideshow's current `image = 0.25.10` pipeline and does not require
an external encoder at runtime. The evaluation-only lock file records the full
graph and checksums; notable versions are:

| Component | Version | Purpose |
| --- | ---: | --- |
| `image` | 0.25.10 | Decode and Lanczos3 resize, plus the candidate AVIF API |
| `webp` | 0.3.1 | Current production encoder API |
| vendored libwebp (`libwebp-sys`) | 1.3.1 (`libwebp-sys` 0.9.6) | Current WebP codec |
| `ravif` | 0.13.0 | Pure-Rust AVIF image encoder |
| `rav1e` | 0.8.1 | AV1 encoder used by `ravif` |
| `avif-serialize` | 0.8.9 | AVIF container serialization |
| FFmpeg | 8.1.1 | Decode and SSIM measurement only |
| Rust | 1.95.0 | Benchmark build toolchain from pinned nixpkgs |

`image` 0.25.10 documents `avif` as the `ravif`-backed encoder feature; its
separate `avif-native` feature is for decoding with `dav1d`. Sideshow does not
need AVIF decode support to evaluate AVIF as an output target, and adding it
would create a different native dependency question.

Sources:

- [`image` 0.25.10 AVIF API and dependency list][image-avif]
- [`image` 0.25.10 feature definitions][image-features]
- [`ravif` 0.13.0 encoder API and dependency list][ravif]
- The exact evaluation graph: `tools/avif-evaluation/encoder/Cargo.lock`

## Reproducible method

Run from the repository root:

```sh
nix run .#avif-evaluation -- \
  --iterations 3 \
  --output avif-results.json \
  --workdir .avif-evaluation
```

The app and encoder are benchmark-only flake outputs; neither is in the
Sideshow package or release artifact. The harness:

1. Downloads one photograph and one software screenshot, then rejects either
   unless its SHA-256 matches the manifest embedded in
   `tools/avif-evaluation/run.py`.
2. Decodes each source and applies the production maximum-dimension rule
   (`3840`) with `image`'s Lanczos3 resize.
3. Encodes WebP with the exact current `webp` API and AVIF with `image`'s
   `AvifEncoder` at speed 4. Both codecs use one thread.
4. Performs one untimed warm-up, then reports the median of three encode-only
   runs. Decode, resize, and file I/O are outside the measured interval.
5. Decodes each output with pinned FFmpeg and reports whole-frame SSIM against a
   lossless PNG of the resized input.
6. Sweeps qualities 40, 50, 60, 70, 80, 90, and 95. It compares the smallest
   AVIF whose SSIM is at least the SSIM of WebP quality 80, Sideshow's default.
7. Reports raw output size, exact base64 data-URI size, source comparison,
   dimensions, settings, and output SHA-256 in JSON.

Quality numbers are deliberately **not** compared directly across codecs;
“quality 80” has no codec-independent meaning. SSIM is an objective proxy, not
a substitute for a blinded visual study. The two fixtures are representative
of common slide-deck content, but they are not a complete image corpus.

### Fixtures

| Class | Source | Input | Evaluated dimensions | License |
| --- | --- | ---: | ---: | --- |
| Photo | [Fronalpstock, Switzerland][photo] by Hannes Röst, SHA-256 `24eb29eccdf0af691b406d1a3d22c0ef5d761cc454d8a917848c33958f6fc857` | 14,679,474 B JPEG | 3839×1725 | CC BY-SA 3.0 |
| Screenshot | [ilo sona Like][screenshot] by kala pona Tonyu, SHA-256 `f23d15195c45da37c8e7a1e4fb5e97fc7759b41cb71c5260358f2fcb54f41fc0` | 99,637 B PNG | 1536×873 | CC0 1.0 |

Fixtures are fetched rather than committed so their attribution stays explicit
and the repository does not acquire 14 MB of benchmark data. Checksums make the
inputs immutable for measurement purposes.

## Darwin results

Measured on 2026-07-10 on an Apple M5 Max (`arm64`), Darwin kernel 25.5.0,
reported by Python as macOS 26.5.2. Nix was 2.34.7 (Determinate Nix 3.21.1),
Rust was 1.95.0, Python was 3.13.13, and FFmpeg was 8.1.1. The pinned nixpkgs
revision was `d407951447dcd00442e97087bf374aad70c04cea`.

The quality sweep and the additional screenshot quality-95 point were run as:

```sh
nix run .#avif-evaluation -- --iterations 3 \
  --qualities 40 50 60 70 80 90 \
  --output darwin-results.json --workdir .avif-evaluation/darwin
nix run .#avif-evaluation -- --iterations 3 \
  --qualities 80 90 95 \
  --output darwin-q95-results.json --workdir .avif-evaluation/darwin-q95
```

Equivalent-quality selections:

| Fixture | Codec/settings | SSIM | Encode median | Output | Data URI | Versus WebP |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Photo | WebP q80 | 0.964486 | 1,000.849 ms | 695,368 B | 927,183 B | baseline |
| Photo | AVIF q70, speed 4 | 0.965570 | 19,976.797 ms | 427,909 B | 570,571 B | **38.46% smaller; 19.96× slower** |
| Screenshot | WebP q80 | 0.997976 | 205.606 ms | 42,098 B | 56,155 B | baseline |
| Screenshot | AVIF q95, speed 4 | 0.998832 | 4,366.408 ms | 52,585 B | 70,139 B | **24.91% larger; 21.24× slower** |

All four outputs were smaller than their source, so the current source-based
“only if smaller” gate would accept them. A format race should nevertheless
retain WebP for this screenshot because it is smaller at equivalent SSIM.
Base64 preserves the raw-size ordering; the four-byte MIME-prefix difference
does not alter either conclusion.

Timing varied noticeably while the shared host was under load (for example,
the repeated photo WebP q80 median was 457.343 ms). Size, SSIM, and output hashes
were stable across the two runs. The table uses timings paired within the run
that supplied each equivalent-quality point; timing should be treated as an
order-of-magnitude result, not a microbenchmark. Even the favorable paired
measurements put AVIF about 20× behind WebP.

## Browser support as of 2026-07-10

Can I Use reported **93.42%** global AVIF support and **96.15%** WebP support,
using June 2026 usage data. AVIF full support began in Chrome 85, Firefox 93,
Edge 121, and Safari 16.4 (Safari 16.1–16.3 was partial); WebP full support began
in Chrome 32, Firefox 65, Edge 18, and Safari 16.0. AVIF remained unsupported in
IE, Opera Mini, QQ Browser 14.9, and KaiOS 2.5/3 in that data set.

MDN, last modified 2026-04-07, likewise recommends a WebP/JPEG/PNG fallback for
AVIF because support has less historical depth. An automatic Sideshow rewrite
replaces one asset reference with one data URI, so it cannot silently add that
fallback. Authors can explicitly provide `<picture>` sources, but converting
every referenced raster source to AVIF would defeat that arrangement.

Sources: [Can I Use AVIF][caniuse-avif], [Can I Use WebP][caniuse-webp], and
[MDN's image format guide][mdn]. Accessed 2026-07-10.

## Build, closure, and platform evidence

The evaluation crate is feature-gated so Nix can build identical WebP-only and
WebP+AVIF binaries. On the Darwin host:

| Evaluation output | NAR/runtime closure |
| --- | ---: |
| WebP-only encoder | 1,398,232 B |
| WebP+AVIF encoder | 2,969,272 B |
| Indicative AVIF delta | **1,571,040 B (1.50 MiB)** |

Both binaries were self-contained Mach-O arm64 executables whose only dynamic
link was `/usr/lib/libSystem.B.dylib`. This is useful evidence that the
`ravif`/`rav1e` candidate does not add a Darwin runtime shared-library
dependency. It is **not** an exact Sideshow binary delta: the benchmark has a
smaller API surface and uses thin LTO, while production uses fat LTO and symbol
stripping.

For context, the unchanged production package measured 5,836,336 B NAR size and
834,849,368 B runtime closure on Darwin. Almost all of that closure is the
existing `tailwindcss_4` wrapper and its LLVM dependencies, so an encoder would
primarily affect the top-level binary, Cargo vendor/build graph, and release
compile time rather than the already-large runtime closure.

The candidate and WebP-only Nix derivations evaluate for all four declared
systems (`aarch64-darwin`, `x86_64-darwin`, `aarch64-linux`, and
`x86_64-linux`). The candidate built and ran on aarch64 Darwin. Attempts to build
both Linux derivations on this machine stopped with Nix's expected `platform
mismatch`: no Linux builder was configured. Existing SourceHut CI and release
jobs prove the unchanged production WebP package on x86_64 Linux, but they do
not build the benchmark-only candidate. Therefore there is **no measured Linux
encode timing, output hash, linkage, or closure result**, and no claim of
cross-platform byte-for-byte output reproducibility is made.

Reproduce the package comparison on each native system with:

```sh
webp="$(nix build --no-link --print-out-paths \
  .#avif-evaluation-encoder-webp-only)"
avif="$(nix build --no-link --print-out-paths \
  .#avif-evaluation-encoder)"
nix path-info --json-format 1 -S "$webp" "$avif"
nix run .#avif-evaluation -- --iterations 3 --output results.json
```

On Linux, also record `file` and `ldd` for both binaries and compare output
SHA-256 values with Darwin. On Darwin, use `otool -L` instead of `ldd`.

## Production invariants and implementation bar

The no-go means all existing behavior remains unchanged without new tests. Any
future implementation must add focused tests and preserve these contracts:

- Detect animation before decode/encode and retain animated GIF, WebP, and APNG
  byte-for-byte. AVIF input must stay unsupported until animated-AVIF detection
  exists, or it risks flattening animation.
- Never write source assets during a build. The explicit `img optimize` command
  may write a sibling only after successful encoding and may delete the source
  only after a successful smaller replacement.
- Compare the chosen candidate with the original source and inline it only when
  smaller. If multiple codecs are attempted, select the smallest output that
  meets the quality policy rather than preferring AVIF by name.
- Preserve `src`, `srcset` (including `<source>`), CSS `url(...)`, query/fragment
  suffixes, MIME types, and current filesystem-confinement behavior.
- Keep `sideshow check`'s per-asset and deck-total budgets based on original,
  de-duplicated source files and projected source data URIs. Optimizer results
  must not make a source-budget warning disappear.
- Validate the complete package and release artifact on native Darwin and
  x86_64 Linux, including license/deny/audit checks and linkage.

## Reevaluation trigger

Reopen this decision when all of the following can be demonstrated:

1. A pinned Rust encoder builds in the normal package/release path on native
   Darwin and x86_64 Linux, with deterministic settings and no problematic
   license, audit, or dynamic-linkage additions.
2. A broader photo/screenshot/alpha corpus shows a consistent data-URI win at
   equivalent perceptual quality; screenshot regressions are avoided by a
   measured per-image codec choice.
3. Representative AVIF encode time is no more than roughly 3× WebP, or encoding
   is made explicitly opt-in so default deck builds do not absorb the cost.
4. The compatibility policy either accepts the then-current browser floor or
   implements real fallback semantics without breaking author-provided
   `<picture>` sources.
5. Linux and Darwin runs publish exact versions, settings, closure/linkage data,
   timings, and output hashes. Missing cross-platform evidence remains a
   conservative no-go.

[caniuse-avif]: https://caniuse.com/avif
[caniuse-webp]: https://caniuse.com/webp
[image-avif]: https://docs.rs/image/0.25.10/image/codecs/avif/index.html
[image-features]: https://docs.rs/crate/image/0.25.10/features
[mdn]: https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Image_types#avif_image
[photo]: https://commons.wikimedia.org/wiki/File:Fronalpstock_big.jpg
[ravif]: https://docs.rs/ravif/0.13.0/ravif/struct.Encoder.html
[screenshot]: https://commons.wikimedia.org/wiki/File:Screenshot_of_ilo_sona_Like.png
