# sideshow fragment patterns

Use these as small starting points, then tailor copy, spacing, and SVG geometry to the deck.

## Title slide

```html
<div class="h-full flex flex-col justify-center px-28">
  <p class="kicker">Platform review</p>
  <h1 class="mt-8 text-hero font-black tracking-tight leading-none">Latency is now a product risk</h1>
  <p class="mt-10 max-w-4xl text-body text-muted">A focused plan for the next two quarters.</p>
</div>
<template data-notes>Open with the customer impact, then name the three decisions needed today.</template>
```

## Stat row

```html
<div class="h-full px-24 py-24 flex flex-col justify-between">
  <div>
    <p class="kicker">Current state</p>
    <h2 class="mt-6 text-display font-bold tracking-tight">The bottleneck is concentrated</h2>
  </div>
  <div class="grid grid-cols-3 gap-8">
    <div class="panel"><div class="stat">71%</div><p class="text-3xl text-muted">of slow requests hit one dependency</p></div>
    <div class="panel"><div class="stat">4×</div><p class="text-3xl text-muted">variance between regions</p></div>
    <div class="panel"><div class="stat">18</div><p class="text-3xl text-muted">incidents with the same signature</p></div>
  </div>
</div>
```

## Two-column argument

```html
<div class="h-full px-24 py-20 grid grid-cols-[0.9fr_1.1fr] gap-16 items-center">
  <div>
    <p class="kicker">Trade-off</p>
    <h2 class="mt-6 text-display font-bold leading-tight">Optimize the path users actually take</h2>
    <p class="mt-8 text-body text-muted">The long tail matters, but the median path is where trust is won or lost.</p>
  </div>
  <div class="space-y-6 text-3xl">
    <div class="panel" data-step><strong>Now:</strong> six services on the critical path</div>
    <div class="panel" data-step><strong>Next:</strong> cache the two volatile joins</div>
    <div class="panel" data-step><strong>Later:</strong> collapse duplicate authorization checks</div>
  </div>
</div>
```

## Full-bleed SVG diagram

```html
<div class="h-full px-20 py-16">
  <p class="kicker">Target architecture</p>
  <svg class="mt-10 w-full h-[820px]" viewBox="0 0 1760 820" role="img" aria-label="Request path through cache, API, and database">
    <defs>
      <marker id="arrow" markerWidth="14" markerHeight="14" refX="12" refY="7" orient="auto"><path d="M0,0 L14,7 L0,14 Z" fill="var(--color-accent)"/></marker>
    </defs>
    <rect x="40" y="210" width="360" height="220" rx="36" fill="var(--color-panel)" stroke="var(--color-accent)"/>
    <text x="220" y="330" text-anchor="middle" fill="var(--color-ink)" font-size="54" font-weight="800">Client</text>
    <path d="M420 320 H720" stroke="var(--color-accent)" stroke-width="10" marker-end="url(#arrow)"/>
    <rect x="750" y="160" width="420" height="320" rx="36" fill="var(--color-panel)" stroke="rgb(255 255 255 / .24)"/>
    <text x="960" y="310" text-anchor="middle" fill="var(--color-ink)" font-size="54" font-weight="800">Edge cache</text>
    <text x="960" y="380" text-anchor="middle" fill="var(--color-muted)" font-size="34">hot joins served here</text>
    <path d="M1190 320 H1490" stroke="var(--color-accent)" stroke-width="10" marker-end="url(#arrow)"/>
    <rect x="1520" y="210" width="200" height="220" rx="36" fill="var(--color-panel)" stroke="rgb(255 255 255 / .24)"/>
    <text x="1620" y="335" text-anchor="middle" fill="var(--color-ink)" font-size="44" font-weight="800">API</text>
  </svg>
</div>
```

## Dense reading slide

```html
<div class="h-full px-24 py-20">
  <p class="kicker">Decision record</p>
  <h2 class="mt-5 text-display font-bold tracking-tight">Adopt regional read-through caching</h2>
  <div class="mt-10 grid grid-cols-2 gap-8 text-2xl leading-snug">
    <div class="panel"><h3 class="text-4xl font-bold">Why now</h3><p class="mt-4 text-muted">Traffic has crossed the point where retry storms amplify downstream variance.</p></div>
    <div class="panel"><h3 class="text-4xl font-bold">Scope</h3><p class="mt-4 text-muted">Cache account summary joins and eligibility reads; exclude write-sensitive workflows.</p></div>
    <div class="panel"><h3 class="text-4xl font-bold">Risk</h3><p class="mt-4 text-muted">Staleness budget must be explicit and observable per tenant.</p></div>
    <div class="panel"><h3 class="text-4xl font-bold">Measure</h3><p class="mt-4 text-muted">P95 below 380ms and no increase in stale-decision support tickets.</p></div>
  </div>
</div>
```
