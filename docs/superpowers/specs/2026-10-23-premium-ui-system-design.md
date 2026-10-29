# Premium UI System — Design Spec

**Date:** 2026-10-23
**Status:** Approved (brainstorming complete), implementing
**Driver:** Polished feel & premium UX (visible change, moderate refactor)
**Predecessor:** Sprint 11 UI/UX redesign (tokens.css, design guard, D-149..D-151)

---

## 1. Context & Motivation

The app has 6+ surfaces built with bespoke CSS + plain-JS `innerHTML`. Sprint 11
introduced a design-token layer (`tokens.css`) and an automated design guard
(`tools/js-checks/design.test.mjs`), but:

- No reusable component layer — buttons/cards/inputs/fields are duplicated markup.
- Icons are static Lucide `<i data-lucide>`; no motion, no unified API.
- Depth is ad-hoc (`--shadow-card`, `--shadow-elevated`) with no single light
  source and no elevation scale.
- No layout primitives — each surface re-invents spacing/stack/grid.
- No micro-interaction layer.

This spec adds: an expanded token foundation, light-DOM custom elements,
CSS-only animated icons, micro-interactions, layout primitives, and a full
surface sweep — all vanilla, offline, CSP-safe, guard-enforced.

## 2. Decisions (from brainstorming)

| # | Decision |
|---|----------|
| Q1 | Primary driver = **polished feel & premium UX** |
| Q2 | Motion = **hybrid** (keep 120/180/240 for interaction; new semantic tokens for entrance/exit/stagger/tab) |
| Q3 | Web Components = **light-DOM custom elements** (`<template>` + slots, NO Shadow DOM) |
| Q4 | Iconography = **Lucide-animated-style, depth/light/gradient visual language** |
| Q5 | Depth = **discipline-first** (single top-left light, elevation scale 0–3, glass for chrome only) |
| Q6 | Adoption = **full sweep in one effort** |
| Q7 | Icons = **vendored subset (~20–30)** |

## 3. Architecture

### 3.1 Token expansion (`src/styles/tokens.css`)
- **Light source:** `--light-angle: 135deg`.
- **Elevation scale** (`--elev-0`..`--elev-3`): each = layered ambient +
  directional + inset top highlight. `--elev-0` flush/hairline, `--elev-1`
  resting, `--elev-2` hover/raised, `--elev-3` overlay.
- **Surface tokens:** `--surface-reflect` (1px top inner highlight),
  `--accent-border`, `--accent-glow`.
- **Motion tokens:** keep `--dur-hover:120ms`/`--dur-enter:180ms`/`--dur-overlay:240ms`;
  add `--dur-instant:120ms`, `--dur-exit:160ms`, `--dur-tab:200ms`,
  `--dur-stagger:40ms`; easings `--ease`, `--ease-out`, `--ease-spring`.
- Reduced-motion block already zeroes all durations.
- **Back-compat:** `--shadow-card`/`--shadow-elevated`/`--shadow-popover`/`--shadow-modal`
  alias to elevation tokens so existing CSS keeps working.

### 3.2 Custom elements (`src/js/components/`)
Light-DOM, one registration module, loaded before `app.js` logic. Each component:
uses `<template>` + slots, sets `part`-like attributes, is token-driven, and
degrades gracefully if unregistered (markup still readable).

- `<hj-icon name size label tone>` — animated icon.
- `<hj-button variant size icon>` — primary/secondary/ghost/danger.
- `<hj-card elevation interactive>` — surface container.
- `<hj-metric-card label value icon tone>` — stat card w/ count-up.
- `<hj-chip tone>` / `<hj-badge tone>`.
- `<hj-field label help error>` — labeled field wrapper.
- `<hj-empty-state icon title>` — empty placeholder.
- `<hj-panel>` — chrome/overlay container (glass).

### 3.3 Iconography
- Vendor `src/assets/libs/animated-icons/lucide/*.svg` (subset of CSS-only
  two-tone animated Lucide; self-contained `<style>`, hover-triggered, visible
  by default, honors reduced-motion).
- `<hj-icon>` loads SVG bodies once into an in-memory map, renders inline,
  forwards `--icon-primary`/`--icon-secondary`, sets `role="img"` + `aria-label`.
- Fallback to existing Lucide `<i data-lucide>` when a name isn't vendored.

### 3.4 Micro-interactions
Hover lift + shadow growth (`--elev-1`→`--elev-2`), press squish
(`transform: scale(.97)`), focus-visible ring (2px accent), metric count-up,
list stagger on view mount (`--dur-stagger`), tab cross-fade.
All gated by `prefers-reduced-motion`.

### 3.5 Layout primitives
`.hj-stack`, `.hj-cluster`, `.hj-grid-auto`, `.hj-split` — spacing rhythm from
`--space-*`; used to replace bespoke per-surface layout wrappers.

### 3.6 Guard extensions (`design.test.mjs`)
- shadows reference `--elev-*` (existing `var(--shadow-` rule extended).
- no `backdrop-filter` inside transitions/animations.
- transition durations ∈ allowed set (extend set with new semantic values).
- hardcoded colors banned (existing); new tokens must be defined.
- every `<hj-icon name>` referenced in HTML/JS exists in vendored set.
- vendored SVG files present + contain reduced-motion handling.

## 4. Out of Scope
- No backend/Rust changes. No new dependencies (rope: `x86_64-pc-windows-msvc`,
  offline-first preserved). No light theme (D-151). No framework/bundler.

## 5. Verification
- `node tools/js-checks/*.test.mjs` all green (incl. extended design guard).
- `node --check` clean on new JS.
- `cargo test` / `cargo check` / `cargo fmt --check` unchanged-green (no Rust edits).
- Manual markup sanity: index.html parses, views render icons.
