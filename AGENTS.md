# sideshow project guidance

Use `jj` for version-control actions in this repository.

## Issue tracker

Tickets live on https://todo.sr.ht/~averagechris/projects with the
`repo:sideshow` label; use the `srht todo` CLI (it auto-applies the repo label).

## Example deck

`examples/making-of-sideshow/` is the dogfood deck; `docs/pages/demo.html` is its
committed build output. After editing the deck, rebuild and re-copy:
`sideshow build examples/making-of-sideshow && cp examples/making-of-sideshow/dist/the-making-of-sideshow.html docs/pages/demo.html`.

## Development

- Enter the toolchain with `direnv allow` or `nix develop`.
- Nix formatting uses wrapped `alejandra -q`; run `nix fmt` or `nix fmt -- --check .`.
- Prefer local checks: `nix run .#static-checks` (fmt + clippy), `nix run .#ci-test`, `nix run .#ci-machete`, `nix run .#ci-sort`, `nix run .#ci-deny`, `nix run .#ci-audit`.
- `.builds/ci.yml` runs fmt, clippy, test, and the package build on every push.

## sccache

The host sets `RUSTC_WRAPPER=sccache` globally, and it must never be unset to "fix"
build failures. If builds fail with sccache connection or compiler errors, run
`sccache --stop-server` and retry; the supervised launchd agent restarts a healthy
server.

## Release workflow

This repo uses the standard averagechris fleet interface:

```sh
nix run .#static-checks
nix run .#release -- --version X.Y.Z --check
nix run .#release -- --version X.Y.Z --submit-linux-build
```

The preflight is non-mutating and fails fast unless the checkout is Git-backed,
SourceHut authentication works, the requested tag is available, and the empty
jj working-copy commit's parent, local `main`, and `main@origin` agree. The
release prepares the tree, runs fmt/clippy/test plus deny, machete, sort, and the
evaluated help/docs contract, then builds and verifies the artifact and checksum
before atomically publishing `main` and the annotated tag. Do not use lower-level
helpers or bypass flags as the routine workflow. After a post-publication
failure, rerun the exact same command: only exact matching release state resumes
idempotently; mismatched checkout, refs, tag, or version fail closed.

`.builds/ci.yml` runs automatically on every push. `builds/release-linux-x86_64.yml`
is explicit-submit only; do not move it to `.builds/`.
