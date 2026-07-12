# ADR 0001: Shared configurable authoring registry

- Status: Accepted
- Date: 2026-07-11

## Context

Sideshow has a shared registry for bundled themes and components plus explicit
project-local packs. Ordinary deck authors and planning-mode authors can always
edit raw HTML, Markdown, JSON, and CSS; registry discovery and composition add a
typed path when that better fits the intent.

Planning does have one genuinely distinct artifact: strict `plan.json` data with
plan-specific validation, stable record IDs, dependency semantics, and JSON and
Markdown exports. Its visual projection is otherwise a slide deck and should not
grow into a second authoring or rendering system.

The served feedback overlay is different from presentation content. It is
privileged Sideshow runtime infrastructure with revision, origin, nonce, and
source-identity responsibilities. Allowing a component or theme pack to replace
it would blur the review trust boundary.

## Decision

Sideshow will evolve toward one configurable authoring registry shared by normal
decks and plans.

The registry will describe:

- themes and their selection metadata;
- reusable slide and planning components, including the intents they serve and
  the data or literal properties they accept;
- scaffold resources and presets;
- the HTML, CSS, and, where explicitly allowed, presentation JavaScript that
  implements those registered resources.

The built-in themes, scaffold HTML, and component CSS are treated as bundled
defaults expressed through the same registry contract. They remain
available offline and preserve current names and behavior, but stop being
one-off sources of truth in Rust. Project-local packs may extend or replace
presentation resources through deterministic, confined configuration. User-level
configuration may aid discovery and choose defaults, but an existing deck must
record or vendor every resource that affects reproducible output.

The exact directory names remain an implementation decision, but a pack is
expected to keep registration metadata beside the resources it registers rather
than embedding their content in Rust, for example:

```text
pack.toml
themes/
  signal/
    theme.toml
    theme.css
components/
  workstream-rail/
    component.toml
    template.html
    style.css
    runtime.js          # optional, declared presentation capability
scaffolds/
  plan/
    scaffold.toml
    slides/
      title.html
      proposal.html
```

The bundled defaults and project-local configuration use the same resource and
registration shape even though their trust and override policies differ.

CLI discovery exposes a clear mapping from intent to capability. Humans and
agents should be able to list and explain registered themes and components in a
machine-readable form, choose a component based on intent, and create or update
declarative slide composition without first writing substantial raw HTML.
Literal properties support ordinary decks; typed bindings support structured
artifacts. Raw HTML/CSS, Markdown, bundled components, and project-local
registered components are first-class choices, selected by cognitive contract
rather than keyword matching. Repeated raw patterns may become reviewed local
patterns, then project components, and only later candidates for bundled
incubation.

Planning is the first typed-data extension of this shared system. It adds:

- strict schema-v2 canonical data and typed mutation commands;
- plan-record bindings for shared registered components;
- stable `data-plan-id` and `data-plan-kind` projection anchors;
- plan-specific checks and deterministic JSON/Markdown exports.

It does not add a separate theme system, component registry, deck renderer, or
review overlay.

The following runtime resources remain compiler-owned and non-replaceable:

- stage and navigation behavior required by the deck contract;
- audit and automation behavior required by `window.sideshow`;
- served feedback/review overlay JavaScript and its security protocol;
- build-time validation, asset processing, and output assembly.

Configurable component JavaScript is presentation code, not privileged runtime
code. It must be separately declared, capability-constrained, compatible with
static/offline output, and subject to profile policy. Planning should require
static HTML/CSS components by default; adding arbitrary component JavaScript is
not implied by registry support.

## Configuration and precedence

The intended layers are:

1. A bundled default pack distributed with Sideshow.
2. Project-local registry and resource directories checked into the deck or
   repository.
3. User configuration for defaults and discovery, which must not silently alter
   the reproducible meaning of an existing deck.

The effective registry and the source of each entry must be inspectable through
the CLI. Name collisions, overrides, missing resources, unsupported capabilities,
and paths escaping their declared pack roots must fail clearly.

Pack-owned static assets are part of this contract when a project or user pack
declares them on a component or theme entry. They are not per-slide file inputs:
they are registry resources with digests, accepted-input bytes, and provenance.
Asset paths are confined to the declaring pack root, must resolve to regular
files without symlinks or traversal, and are embedded deterministically when the
component/theme is rendered or applied so builds stay offline and self-contained.
The existing remote/data/javascript/SVG/output-budget policies still apply to the
final deck, and pack JavaScript remains rejected until a separate presentation-JS
decision exists.

## Rendering boundary

Askama remains appropriate for fixed compiler-owned templates compiled into the
application. Editable or user-registered runtime component templates require a
separate deliberately constrained rendering contract; registering resources must
not pretend that arbitrary files are compile-time Askama templates.

All rendering remains deterministic, build-time, offline, and self-contained.
Ordinary data is escaped by default. Raw component output, CSS, and JavaScript
cross explicit narrow trust boundaries and still pass the applicable fragment,
asset, remote-reference, SVG, and output-budget checks.

## Consequences

- Normal deck and planning authoring converge on the same building blocks and
  theme vocabulary.
- Agents can discover supported intent and translate it into Sideshow commands
  instead of synthesizing most markup from scratch.
- Planning remains focused on canonical plan semantics rather than presentation
  infrastructure.
- Existing authored fragments remain valid and remain the lowest-level escape
  hatch.
- Registry, declarative composition, typed plan mutation, and safe project-local
  extension can evolve independently without changing the ownership split between
  agent judgment and deterministic CLI behavior.
- The fixed review overlay stays auditable and cannot be shadowed by configured
  resources.

## Deferred questions

- Further manifest syntax changes beyond the current constrained `pack.toml`
  component/theme/static-asset declarations.
- The constrained runtime template language for project-defined components.
- Which presentation JavaScript capabilities, if any, are allowed per profile.
- Whether a second typed artifact justifies generalizing the plan-data provider
  interface beyond the minimum binding contract needed by planning.
