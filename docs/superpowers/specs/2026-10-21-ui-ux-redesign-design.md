# Design: UI/UX Redesign — Quiet Notebook × Precision Workbench

**Date:** 2026-10-21
**Status:** Approved (user confirmed sections 1–4)
**Scope:** Dark-only visual and interaction redesign of the five visible tabs (Journal, Tasks, Mood, System, Settings), the AI sidebar, and the vault/onboarding overlays. Phased: token foundation first, then surfaces. No backend, schema, or wire changes.

---

## 1. Context

The app works end-to-end but reads as an AI-generated dashboard: 10–12px type, 829 pill radii, nested cards, a single generic blue accent, emoji-as-icons, and drag-only mutations. This pass replaces the visual system and fixes measurable accessibility gaps without touching data flow.

**Constraints carried in:**
- Vanilla HTML/CSS/JS, no framework, no bundler, no network assets (D-42, D-50).
- Offline-first; system fonts only.
- Wire contract D-148 untouched.
- No new global shortcuts or command palette (user decision — accessibility minimum only).
- Dark theme only (user decision).

---

## 2. Evidence

### 2a. Live recon (1400×900, DPR 1.25, release build via CDP)

| Metric | Value |
|---|---|
| Font sizes in DOM | 12px ×2,579 · 11px ×1,154 · 10px ×862 · 14px ×610 |
| Border radii in DOM | 0 ×4,288 · 999px ×829 · 8px ×74 · 6px ×67 |
| Distinct text colors | 7 |
| Text contrast | 6.15 – 12.26 (all pass AA) |
| Focus | `:focus-visible` present |
| Reduced motion | no `prefers-reduced-motion` rule |
| ARIA | 4 `aria-label`, 2 `role` |
| Viewport | 1400×900 @1.25 |
| Inline style attributes (DOM) | 20 (2 static in HTML, 18 in JS-rendered markup) |
| Controls | 85 buttons, 50 inputs |
| Transition durations | 0.15s, 0.2s, 0.3s (mixed) |

CSS inventory: 3,258 lines across 13 files — `tasks.css` 623, `components.css` 530, `layout.css` 446, `ai-chat.css` 438, `memory.css` 270, `calendar.css` 219, `settings-diagnostics.css` 162, `base.css` 114, `vault.css` 88, `mood.css` 53, `toast.css` 43, `terminal.css` 0, themes 12.

`src/styles/themes/dark.css` is linked last (`src/index.html:19`) and overrides five base colors; `themes/light.css` is never linked (dead code). `base.css:6-32` is the only other token source.

### 2b. Concrete offenders

| Pattern | Location |
|---|---|
| Gradient washes | `layout.css:37`, `layout.css:133`, `ai-chat.css:221` |
| 999px pills | `ai-chat.css:433`, `components.css:130`, `components.css:401`, `layout.css:229`, `memory.css:178`, `mood.css:8`, `settings-diagnostics.css:71` |
| Emoji as icons | `index.html` (🧠 📓 🪄 🗑 ✅ 🗓 🔐 🔒), `ai-sidebar.js` (✓ 🧑🏽 🤖 💭), `onboarding.js`, `tasks.js`, `settings.js`, `calendar.css`, `tasks.css` |
| Generic accent | `--color-accent: #58a6ff` (`base.css:15`) |
| Dead theme | `themes/light.css` (6 lines, unlinked) |
| No type ramp | ad-hoc `font-size` per selector |

### 2c. Standards

- **WCAG 2.2 AA:** 2.5.7 dragging alternative, 2.5.8 target size ≥24×24, 2.4.11 focus not obscured, 1.4.11 non-text contrast 3:1, 1.4.3 text contrast 4.5:1, 1.4.4/1.4.10 resize/reflow.
- **Windows/Fluent type ramp:** 12/16 caption, 14/20 body, 20/28 subtitle, 28/36 title; semibold emphasis; sentence case; Segoe UI Variable.
- **Desktop UX:** density over airiness, keyboard traversal, stable panes, on-object commands.
- **Anti-slop:** no glass, gradients, nested cards, pill-everything, emoji icons, fake metric grids, single-hue dark palettes, oversized radii.

---

## 3. Design

### 3.1 Direction

**Hybrid:** *quiet notebook* for Journal, Mood, and Assistant; *precision workbench* for Tasks, System, and Settings. One token layer unifies both. Signature element: the journal page (serif, measured, margin metadata) — not decoration.

### 3.2 Color tokens (dark only, measured)

| Token | Value | Contrast (canvas / surface / raised) |
|---|---|---|
| `--color-canvas` | `#12110F` | — |
| `--color-surface` | `#1A1816` | — |
| `--color-surface-raised` | `#211E1B` | — |
| `--color-surface-hover` | `#24211D` | non-text |
| `--color-border` | `#2E2A26` | 1.33 (decorative hairline) |
| `--color-border-strong` | `#3A3530` | 1.56 (pressed) |
| `--color-border-control` | `#7A7065` | 3.89 / 3.65 / 3.42 ✅ 1.4.11 |
| `--color-text` | `#E8E3DA` | 14.77 / 13.85 / 12.98 ✅ |
| `--color-text-muted` | `#A39A8E` | 6.80 / 6.38 / 5.98 ✅ |
| `--color-text-subtle` | `#7C7367` | 4.05 / 3.80 / 3.56 ⚠️ decorative/disabled only |
| `--color-accent` | `#8FB996` | 8.58 / 8.05 / 7.54 ✅ |
| `--color-accent-hover` | `#A6CBAC` | ≥ accent |
| `--color-accent-soft` | `rgba(143,185,150,.12)` | tint only |
| `--color-on-accent` | `#12110F` | 8.58 on accent ✅ |
| `--color-success` | `#7FB069` | 7.47 / 7.01 / 6.57 ✅ |
| `--color-warn` | `#D9A441` | 8.39 / 7.87 / 7.37 ✅ |
| `--color-error` | `#E06C5F` | 5.82 / 5.46 / 5.12 ✅ |

Rules: semantic colors only for state. No gradients. No color-only meaning (priority, mood, engine status always carry text).

### 3.3 Typography

| Token | Family | Size / line | Weight |
|---|---|---|---|
| `--text-caption` | UI | 12/16 | 400 |
| `--text-meta` | UI | 13/18 | 400, tabular |
| `--text-body` | UI | 14/20 | 400 |
| `--text-body-strong` | UI | 14/20 | 600 |
| `--text-lg` | UI | 18/24 | 400 |
| `--text-subtitle` | Display | 20/28 | 600 |
| `--text-title` | Display | 28/36 | 600 |
| `--text-page` | Display | 40/48 | 600 |
| `--text-journal` | Serif | 16/26, measure 68ch | 400 |
| `--text-code` | Mono | 13/18 | 400 |

`--font-ui: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`
`--font-display: "Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif`
`--font-serif: Georgia, "Iowan Old Style", Cambria, serif`
`--font-mono: "Cascadia Mono", Consolas, monospace`

Weights 400/600 only. No 700. Sentence case. Italics reserved for user-written journal text. **12px is the hard floor.**

### 3.4 Space, radius, elevation, motion

- Space ramp (px-named): 2, 4, 6, 8, 10, 12, 16, 20, 24, 32, 40, 48.
- Radius: `2` chip · `4` control/button · `6` input · `8` panel/menu · `12` modal. `999px` removed except true status dots.
- Elevation: exactly two shadows — popover `0 4px 12px rgba(0,0,0,.28)`, modal `0 16px 48px rgba(0,0,0,.40)`. No shadowed cards.
- Containment: max one bordered container per region; inner groups use 1px dividers.
- Motion: `--dur-hover:120ms`, `--dur-enter:180ms`, `--dur-overlay:240ms`, `--ease:cubic-bezier(.2,0,0,1)`; opacity/color/width only; `prefers-reduced-motion: reduce` → 0ms.
- Focus: 2px accent ring + 2px offset, `:focus-visible` on all interactive elements, never clipped.
- Sizes: `--control-h:32px`, `--control-h-lg:36px`, `--icon-btn:24px`, `--row-h:36px`, `--row-h-dense:32px`, `--nav-w:232px`, `--nav-rail-w:56px`, `--rail-w:48px`, `--assistant-w:400px`, `--list-w:280px`.
- Solid canvas (Mica no longer visible; plugin config untouched).

### 3.5 Shell & layout

- Grid: `232px | minmax(0,1fr) | 400px`, right pane collapsible to 48px rail.
- Breakpoints: 1320px → assistant auto-collapses to rail; 1040px → nav becomes a 56px icon rail and the journal list collapses; 820px → single pane + toggles.
- Per-tab assistant memory stored in `localStorage` under `ui.assistant.collapsed.<tab>`; toggle is a plain button (no shortcut).
- Nav active state: 2px accent left rule + 6% tint (no pill).
- Footer status rows keep dot + label + value, 12/16.

### 3.6 Surfaces

**Journal (notebook)**
- 280px entry list · page · assistant rail.
- Editor page: max 68ch, serif 16/26, 32px top padding, hairline edge, no card.
- Page header: title 28/36; Analyze as secondary 32px; delete moved into a `⋯` menu.
- Footer: word count + save state, 12/16 muted.
- List rows: title 14/20, meta 12/16, active = 2px accent rule.
- Empty state: one sentence + New Entry.
- `VirtualList.rowHeight` (`journal.js:35`) updated to match the new row height exactly.

**Tasks (workbench)**
- Board: 4 columns ≥280px, 16px gap; cards 12px padding, radius 6, border only.
- Card: 2px priority rule + title 14/20 semibold + meta 12/16 + `⋯` menu (To Do / In Progress / Done / Archive / Restore).
- List view: 36px rows; dense table 32px; mono numerals.
- Calendar: cells ≥40px; chips 24px; `⋯` menu offers Schedule… / Clear date.
- Filters: segmented control, radius 4.

**Mood**
- Chips: 32px, radius 4, `radiogroup`, selected = accent border + 8% tint.
- "Clear this day" becomes a tertiary text action.
- Calendar: 40px cells; mood as 3px accent underline + label; today outlined.
- Trend: single accent line, muted axes, no area fill.

**System (workbench, no card soup)**
- Runtime: two-column definition table (12/16 muted label · mono 13/18 value) with dividers.
- Paths: middle truncation, title tooltip, 24px copy button.
- Models: list rows with size + role chip (radius 2).
- Engines: table rows — dot, name, pid, latency, 32px Health/Restart/Stop.
- Queue: real table with per-row Retry.
- DB: `Backup now` primary, `Open data dir` secondary.
- SQL runner: labeled mono textarea, result table, status line.
- Log: mono 12/18, segmented level filter, bounded height.

**Settings**
- Single column max 720px; section headers 20/28 + description 14/20 muted; hairline rules, no cards.
- Field rows: label left, control right, 32px controls, help 12/16.
- Danger zone separated by a rule with an error-tinted heading only.

**Assistant**
- Header 48px: name, inbox count, collapse button (32px hit).
- Status strip: 12/16 + 4px state bar; elapsed tabular.
- Chat: no bubbles — user turns get a 2px accent left rule; assistant plain text; reasoning stays collapsible using `warn`.
- Composer: radius 8, 14/20, 32px send.
- Rail: 48px with glyph, live analysis dot, inbox count.

**Overlays**
- Vault: 400px panel, radius 12, one field, 36px primary, inline error.
- Onboarding: 640px panel, step list, one decision per screen, 2px progress.

**Icons**
- All emoji replaced with Lucide icons (already vendored) or inline SVG. Brand mark becomes a typographic monogram/SVG, not 🧠.

### 3.7 Interaction & accessibility

- DOM order = visual order; landmarks `nav` (Primary), `main`, `complementary` (Assistant).
- Arrows move in lists/board/calendar; Enter opens; Space toggles; Esc closes and restores focus.
- Menus: roving tabindex; `aria-expanded`; Esc returns focus.
- Dialogs: focus trap, initial focus, restore on close.
- Every icon-only button gets an accessible name; `aria-current` on nav; `aria-live="polite"` for analysis/toasts, `assertive` for errors.
- Tables rendered with real `table` semantics.
- Labels bound with `for`; errors `aria-describedby` + `aria-invalid`.
- Drag alternatives are mandatory (2.5.7): board card menu, calendar menu.
- Reflow verified at 200% zoom and 200% text size; forced-colors mode supported (no background-image icons, no color-only state).

---

## 4. Migration

### 4.1 File structure

```
src/styles/
  tokens.css          (new — all tokens)
  base.css            (reset, type ramp, focus, forced-colors)
  layout.css          (shell, nav, panes, breakpoints)
  components.css      (buttons, fields, menus, tables, chips, states)
  toast.css           (unchanged location)
  memory.css          (unchanged — hidden tab, tokens only)
  surfaces/
    journal.css  tasks.css  calendar.css  mood.css
    system.css   settings.css  assistant.css  overlays.css
```

- `themes/dark.css` folded into `tokens.css`; **`themes/light.css` deleted** (unlinked dead code).
- `index.html` link order: tokens → base → layout → components → toast → surfaces.
- `settings-diagnostics.css` split into `surfaces/settings.css` + `surfaces/system.css`.
- `onboarding.css` + `vault.css` merge into `surfaces/overlays.css`.
- `ai-chat.css` → `surfaces/assistant.css`; `memory.css` stays (hidden tab, inherits tokens only).

### 4.2 JS/HTML touch points

| File | Change |
|---|---|
| `index.html` | link list, nav markup, brand mark, Journal/Tasks/Mood/System/Settings markup, assistant header, emoji removal |
| `app.js` | assistant collapse toggle + per-tab persistence, `renderIcons()` after dynamic renders, no new shortcuts |
| `journal.js` | row markup/height, page header/footer, `⋯` menu, emoji removal |
| `tasks.js` | card anatomy, priority rule + label, card `⋯` menu, segmented filters |
| `tasks-calendar.js` | chip markup, `⋯` Schedule/Clear menu |
| `mood.js` | chip `radiogroup`, selected state, calendar underline, clear action |
| `system.js` | definition tables, path truncation/copy, engine/queue tables, log filter |
| `settings.js` | section layout, field rows, inline vault errors |
| `ai-sidebar.js` | header/status/composer, no bubbles, collapse button, emoji removal |
| `proposals.js` | card classes, text-button actions |
| `onboarding.js`, `vault.js` | overlay layout + inline errors |
| `notifications.js` | `role="status"`, dismiss button |

Preserved invariants: `escapeHtml` before every `innerHTML` sink; event dispatch by `payload.type`; no raw `emit`; D-145 proposal cards remain the single accept surface; D-148 arg casing unchanged.

### 4.3 Guardrails (automated, new)

`tools/js-checks/design.test.mjs` — fails on:
- `border-radius: 999px` outside an allowlist (status dots only)
- `linear-gradient` / `radial-gradient`
- hardcoded hex in CSS outside `tokens.css`
- `font-size` below 12px
- transition durations outside {120ms, 180ms, 240ms}
- `outline: none` without a `:focus-visible` rule
- more than two `box-shadow` declarations
- emoji in redesign scope — allowlist for out-of-scope hidden tabs: `events.js`, `inbox.js`, `memory.js`, `reports.js`, `views/about.html`, `views/memory.html`
- token contrast below AA (text) / 3:1 (control borders)

`tools/ui-audit.mjs` (CDP) — reports unnamed buttons, targets <24px, font-size histogram, radius histogram, container nesting depth, and writes screenshots. Run before/after each phase.

### 4.4 Phases

| Phase | Scope | Commit type |
|---|---|---|
| 1 | `tokens.css`, `base.css` ramp, focus/targets/motion, delete light stub, guardrail test | `style(tokens):` |
| 2 | Shell, nav, collapsible assistant (D-149), breakpoints | `feat(ui):` |
| 3 | Journal + Assistant | `style(journal):` / `style(assistant):` |
| 4 | Tasks list/board/calendar + card menus (2.5.7) | `feat(tasks):` |
| 5 | Mood | `style(mood):` |
| 6 | System + Settings | `style(system):` / `style(settings):` |
| 7 | Vault + onboarding overlays | `style(overlays):` |
| 8 | Audit: zoom/text 200%, keyboard-only, reduced motion, forced colors, final screenshots | `a11y(ui):` |

Each phase ends with: `node --check` on touched JS, node harnesses, `tools/ui-audit.mjs`, screenshots, and a commit.

---

## 5. Acceptance criteria

- 0 gradients; 0 `999px` radii (outside status dots); 0 font sizes <12px; ≤2 shadows; 0 emoji in redesign scope (hidden-tab allowlist applies).
- 0 unnamed icon buttons; 0 interactive targets <24×24.
- Every text token pair ≥4.5:1; every control border ≥3:1.
- Every drag mutation has a menu path.
- Full keyboard traversal of all 5 tabs; focus never obscured.
- No clipping at 200% zoom or 200% text size; forced-colors mode legible.
- 6 existing node harnesses + new design test green; `cargo check`/`cargo test` unaffected (no Rust changes).
- D-148 wire contract untouched; no migration.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| CSS class renames break JS-rendered markup | grep every class before rename; audit script catches unstyled/unnamed controls; per-phase commits |
| Assistant collapse breaks the "always mounted" analysis listener | collapse is CSS/DOM width only; the sidebar node and its listener never unmount |
| VirtualList row height mismatch | update `journal.js:35` in the same commit as the row CSS; verify scroll on 200+ entries |
| Mica renders behind a solid canvas | accepted: solid warm canvas; plugin config left intact |
| XSS regression during markup rewrite | all injected content keeps `escapeHtml`; review every `innerHTML` sink in the phase diff |
| Hidden tabs (Memory/Events/Reports/Inbox/About) look inconsistent | they inherit tokens; out of scope beyond that |
| Scope creep into interaction features | user chose accessibility minimum — no palette, no new shortcuts |

---

## 7. Decisions

- **D-149** — AI sidebar is collapsible with per-tab remembered state (amends D-23/D-78).
- **D-150** — Design token layer + anti-slop/accessibility lint guards (no 999px pills, no gradients, 12px type floor, 120/180/240ms motion).
- **D-151** — Dark-only theme; delete the unlinked light stub.

---

## 8. Out of scope

Hidden-tab redesign beyond token inheritance · light theme · command palette / new shortcuts · backend, DB, wire changes · removing drag · network fonts · terminal bar.
