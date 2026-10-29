# UI/UX Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the app's ad-hoc styling with a measured dark token system and rebuild the five visible tabs, assistant, and overlays against it, without touching Rust, the database, or the wire contract.

**Architecture:** Add `tokens.css` as the single source of truth and keep every legacy CSS variable mapped to a new token so the app stays shippable after each commit. Migrate surfaces one phase at a time, deleting the compatibility aliases only in the final task. A Node guard test enforces the anti-slop and contrast rules.

**Tech Stack:** Vanilla HTML/CSS/JS, Lucide (vendored), Chart.js (vendored), SortableJS (vendored), Node ESM test scripts, Tauri v2 (unchanged).

## Global Constraints

- Dark theme only. No light theme, no theme toggle.
- No gradients, no glassmorphism, no `border-radius: 999px` except status dots.
- 12px is the minimum font size; body text is 14/20; weights 400/600 only.
- Motion durations only 120ms, 180ms, 240ms; `prefers-reduced-motion: reduce` → 0ms.
- Exactly two shadows: `--shadow-popover`, `--shadow-modal`.
- Controls 32px (`--control-h`), primary 36px, icon hit area ≥24×24.
- Text tokens ≥4.5:1; control borders ≥3:1.
- No new keyboard shortcuts, no command palette.
- No Rust, schema, migration, or wire-contract changes (D-148).
- Every `innerHTML` sink keeps `escapeHtml` (D-145).
- Files under `docs/` and `.superpowers/` are gitignored: use `git add -f`.
- Verify with `node --check <file>`, `node tools/js-checks/<x>.test.mjs`, `cargo check`.

---

### Task 1: Token layer + base reset

**Files:**
- Create: `src/styles/tokens.css`
- Rewrite: `src/styles/base.css`
- Modify: `src/index.html:7-19`
- Delete: `src/styles/themes/dark.css`, `src/styles/themes/light.css`

**Interfaces:**
- Produces: every token name used by later tasks (`--color-*`, `--fs-*`, `--lh-*`, `--space-*`, `--radius-*`, `--dur-*`, `--ease`, `--control-h`, `--nav-w`, `--assistant-w`, `--list-w`, `--rail-w`, `--nav-rail-w`).
- Produces: legacy aliases (`--color-bg`, `--color-muted`, `--color-ai`, `--color-surface-soft`, `--color-border-soft`, `--radius`, `--radius-sm`, `--space`, `--transition`, `--shadow-sm`, `--shadow-md`, `--sidebar-left-w`, `--sidebar-right-w`) so unmigrated surfaces keep working.

- [ ] **Step 1: Create `tokens.css`**

```css
:root {
  /* ── Color ─────────────────────────────────────────────── */
  --color-canvas:         #12110F;
  --color-surface:        #1A1816;
  --color-surface-raised: #211E1B;
  --color-surface-hover:  #24211D;
  --color-border:         #2E2A26;
  --color-border-strong:  #3A3530;
  --color-border-control: #7A7065;
  --color-text:           #E8E3DA;
  --color-text-muted:     #A39A8E;
  --color-text-subtle:    #7C7367;
  --color-accent:         #8FB996;
  --color-accent-hover:   #A6CBAC;
  --color-accent-soft:    rgba(143, 185, 150, 0.12);
  --color-on-accent:      #12110F;
  --color-success:        #7FB069;
  --color-success-soft:   rgba(127, 176, 105, 0.12);
  --color-warn:           #D9A441;
  --color-warn-soft:      rgba(217, 164, 65, 0.12);
  --color-error:          #E06C5F;
  --color-error-soft:     rgba(224, 108, 95, 0.12);

  /* ── Type ──────────────────────────────────────────────── */
  --font-ui:      "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  --font-display: "Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif;
  --font-serif:   Georgia, "Iowan Old Style", Cambria, serif;
  --font-mono:    "Cascadia Mono", Consolas, monospace;

  --fs-caption: 12px;  --lh-caption: 16px;
  --fs-meta:    13px;  --lh-meta:    18px;
  --fs-body:    14px;  --lh-body:    20px;
  --fs-lg:      18px;  --lh-lg:      24px;
  --fs-subtitle:20px;  --lh-subtitle:28px;
  --fs-title:   28px;  --lh-title:   36px;
  --fs-page:    40px;  --lh-page:    48px;
  --fs-journal: 16px;  --lh-journal: 26px;

  /* ── Space (px-named) ──────────────────────────────────── */
  --space-2: 2px;   --space-4: 4px;   --space-6: 6px;
  --space-8: 8px;   --space-10: 10px; --space-12: 12px;
  --space-16: 16px; --space-20: 20px; --space-24: 24px;
  --space-32: 32px; --space-40: 40px; --space-48: 48px;

  /* ── Radius ────────────────────────────────────────────── */
  --radius-chip: 2px;
  --radius-control: 4px;
  --radius-input: 6px;
  --radius-panel: 8px;
  --radius-modal: 12px;

  /* ── Elevation ─────────────────────────────────────────── */
  --shadow-popover: 0 4px 12px rgba(0, 0, 0, 0.28);
  --shadow-modal:   0 16px 48px rgba(0, 0, 0, 0.40);

  /* ── Motion ────────────────────────────────────────────── */
  --dur-hover: 120ms;
  --dur-enter: 180ms;
  --dur-overlay: 240ms;
  --ease: cubic-bezier(0.2, 0, 0, 1);

  /* ── Sizes ─────────────────────────────────────────────── */
  --control-h: 32px;
  --control-h-lg: 36px;
  --icon-btn: 24px;
  --row-h: 36px;
  --row-h-dense: 32px;
  --nav-w: 232px;
  --nav-rail-w: 56px;
  --rail-w: 48px;
  --assistant-w: 400px;
  --list-w: 280px;

  /* ── Legacy aliases (removed in Task 10) ───────────────── */
  --color-bg: var(--color-canvas);
  --color-surface-soft: var(--color-surface-raised);
  --color-border-soft: var(--color-border);
  --color-muted: var(--color-text-muted);
  --color-ai: var(--color-accent);
  --radius: var(--radius-panel);
  --radius-sm: var(--radius-input);
  --space: 16px;
  --transition: var(--dur-hover) var(--ease);
  --shadow-sm: var(--shadow-popover);
  --shadow-md: var(--shadow-modal);
  --sidebar-left-w: var(--nav-w);
  --sidebar-right-w: var(--assistant-w);
}

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    transition-duration: 0ms !important;
    animation-duration: 0ms !important;
    animation-iteration-count: 1 !important;
    scroll-behavior: auto !important;
  }
}
```

- [ ] **Step 2: Rewrite `base.css`**

```css
*, *::before, *::after {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

html, body {
  height: 100%;
  width: 100%;
  overflow: hidden;
  background: var(--color-canvas);
  color: var(--color-text);
  font-family: var(--font-ui);
  font-size: var(--fs-body);
  line-height: var(--lh-body);
  -webkit-font-smoothing: antialiased;
  text-rendering: optimizeLegibility;
}

h1, h2, h3, h4 {
  font-family: var(--font-display);
  font-weight: 600;
  line-height: 1.25;
}

a {
  color: var(--color-accent);
  text-decoration: none;
  cursor: pointer;
}
a:hover { text-decoration: underline; }

button, input, select, textarea {
  font-family: var(--font-ui);
  font-size: var(--fs-body);
  line-height: var(--lh-body);
  color: var(--color-text);
}

button {
  cursor: pointer;
  border: none;
  background: none;
  border-radius: var(--radius-control);
  transition: color var(--dur-hover) var(--ease),
              background-color var(--dur-hover) var(--ease),
              border-color var(--dur-hover) var(--ease);
}

:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
}

::selection {
  background: var(--color-accent-soft);
  color: var(--color-text);
}

::-webkit-scrollbar { width: 10px; height: 10px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb {
  background: var(--color-border-strong);
  border-radius: var(--radius-control);
  border: 2px solid var(--color-canvas);
}
::-webkit-scrollbar-thumb:hover { background: var(--color-border-control); }

.hidden { display: none !important; }

.visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
  white-space: nowrap;
  border: 0;
}

@media (forced-colors: active) {
  * { forced-color-adjust: auto; }
  :focus-visible { outline: 2px solid CanvasText; }
}
```

- [ ] **Step 3: Update `index.html` stylesheet links**

Replace lines 7–19 with:

```html
  <link rel="stylesheet" href="styles/tokens.css" />
  <link rel="stylesheet" href="styles/base.css" />
  <link rel="stylesheet" href="styles/layout.css" />
  <link rel="stylesheet" href="styles/toast.css" />
  <link rel="stylesheet" href="styles/components.css" />
  <link rel="stylesheet" href="styles/settings-diagnostics.css" />
  <link rel="stylesheet" href="styles/ai-chat.css" />
  <link rel="stylesheet" href="styles/tasks.css" />
  <link rel="stylesheet" href="styles/calendar.css" />
  <link rel="stylesheet" href="styles/memory.css" />
  <link rel="stylesheet" href="styles/mood.css" />
  <link rel="stylesheet" href="styles/onboarding.css" />
  <link rel="stylesheet" href="styles/vault.css" />
```

- [ ] **Step 4: Delete the theme files**

```bash
git rm src/styles/themes/dark.css src/styles/themes/light.css
```

- [ ] **Step 5: Verify**

```bash
node -e "const fs=require('fs');const s=fs.readFileSync('src/styles/tokens.css','utf8');for(const t of ['--color-canvas','--color-accent','--fs-body','--radius-control','--dur-hover'])if(!s.includes(t))throw new Error('missing '+t);console.log('tokens ok')"
grep -c "themes/" src/index.html
```

Expected: `tokens ok`, then `0`.

- [ ] **Step 6: Commit**

```bash
git add src/styles/tokens.css src/styles/base.css src/index.html
git commit -m "style(tokens): add measured dark token layer and base reset"
```

---

### Task 2: Design guard test

**Files:**
- Create: `tools/js-checks/design.test.mjs`

**Interfaces:**
- Consumes: `src/styles/tokens.css` values, all `src/**/*.{css,js,html}`.
- Produces: `PENDING` set of not-yet-migrated CSS files; each later task removes its files. Task 10 empties it.

- [ ] **Step 1: Write the test**

```js
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = process.cwd();
const PENDING = new Set([
  "src/styles/layout.css",
  "src/styles/components.css",
  "src/styles/ai-chat.css",
  "src/styles/tasks.css",
  "src/styles/calendar.css",
  "src/styles/mood.css",
  "src/styles/settings-diagnostics.css",
  "src/styles/onboarding.css",
  "src/styles/vault.css",
  "src/styles/toast.css",
  "src/styles/memory.css",
]);

const EMOJI_ALLOW = new Set([
  "src/js/tabs/events.js",
  "src/js/tabs/inbox.js",
  "src/js/tabs/memory.js",
  "src/js/tabs/reports.js",
  "src/views/about.html",
  "src/views/memory.html",
]);

const failures = [];
const warn = (file, msg) => failures.push(`${file}: ${msg}`);

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      if (p.includes("assets") || p.includes("node_modules")) continue;
      walk(p, out);
    } else out.push(p);
  }
  return out;
}

const files = walk(join(ROOT, "src"));
const css = files.filter((f) => f.endsWith(".css"));

// tokens
const tokensSrc = readFileSync(join(ROOT, "src/styles/tokens.css"), "utf8");
const tokens = new Map();
for (const m of tokensSrc.matchAll(/--([a-z0-9-]+):\s*(#[0-9A-Fa-f]{6})/g)) {
  tokens.set(m[1], m[2]);
}

function lum(hex) {
  const n = parseInt(hex.slice(1), 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => {
    v /= 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * ch[0] + 0.7152 * ch[1] + 0.0722 * ch[2];
}
function ratio(a, b) {
  const l1 = lum(a), l2 = lum(b);
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
}

const PAIRS = [
  ["color-text", "color-canvas", 4.5],
  ["color-text", "color-surface", 4.5],
  ["color-text", "color-surface-raised", 4.5],
  ["color-text-muted", "color-canvas", 4.5],
  ["color-text-muted", "color-surface", 4.5],
  ["color-text-muted", "color-surface-raised", 4.5],
  ["color-accent", "color-canvas", 4.5],
  ["color-accent", "color-surface", 4.5],
  ["color-accent", "color-surface-raised", 4.5],
  ["color-error", "color-canvas", 4.5],
  ["color-error", "color-surface", 4.5],
  ["color-success", "color-surface", 4.5],
  ["color-warn", "color-surface", 4.5],
  ["color-on-accent", "color-accent", 4.5],
  ["color-border-control", "color-surface", 3],
  ["color-border-control", "color-surface-raised", 3],
];
for (const [fg, bg, min] of PAIRS) {
  if (!tokens.has(fg) || !tokens.has(bg)) {
    warn("tokens.css", `missing ${fg} or ${bg}`);
    continue;
  }
  const r = ratio(tokens.get(fg), tokens.get(bg));
  if (r < min) warn("tokens.css", `${fg} on ${bg} = ${r.toFixed(2)} < ${min}`);
}

let shadowDecls = 0;
for (const file of css) {
  const rel = relative(ROOT, file).replaceAll("\\", "/");
  const src = readFileSync(file, "utf8");
  const pending = PENDING.has(rel);

  if (!pending) {
    for (const m of src.matchAll(/(?:linear|radial)-gradient\(/g))
      warn(rel, `gradient at ${src.slice(0, m.index).split("\n").length}`);
    for (const m of src.matchAll(/border-radius:\s*999px/g))
      warn(rel, `pill radius at ${src.slice(0, m.index).split("\n").length}`);
    for (const m of src.matchAll(/font-size:\s*(\d+(?:\.\d+)?)px/g)) {
      if (Number(m[1]) < 12)
        warn(rel, `font-size ${m[1]}px at ${src.slice(0, m.index).split("\n").length}`);
    }
    for (const m of src.matchAll(/font-size:\s*(0?\.\d+|0)rem/g)) {
      if (Number(m[1]) < 0.75)
        warn(rel, `font-size ${m[1]}rem at ${src.slice(0, m.index).split("\n").length}`);
    }
    for (const m of src.matchAll(/transition(?:-duration)?:[^;]*?(\d+(?:\.\d+)?)(ms|s)/g)) {
      const ms = m[2] === "s" ? Number(m[1]) * 1000 : Number(m[1]);
      if (![0, 120, 180, 240].includes(ms))
        warn(rel, `transition ${ms}ms at ${src.slice(0, m.index).split("\n").length}`);
    }
  }
  if (rel !== "src/styles/tokens.css") {
    for (const m of src.matchAll(/#[0-9A-Fa-f]{3,8}\b/g)) {
      if (!pending) warn(rel, `hardcoded color ${m[0]}`);
    }
  }
  shadowDecls += (src.match(/box-shadow:/g) || []).length;
  if (src.includes("outline: none") && !src.includes(":focus-visible")) {
    warn(rel, "outline:none without :focus-visible");
  }
  if (!pending && /outline:\s*none/.test(src)) {
    warn(rel, "outline:none is forbidden");
  }
}
if (shadowDecls > 2) warn("styles", `box-shadow declarations = ${shadowDecls} (max 2)`);

const emoji = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/gu;
for (const file of files.filter((f) => /\.(js|html|css)$/.test(f))) {
  const rel = relative(ROOT, file).replaceAll("\\", "/");
  if (EMOJI_ALLOW.has(rel)) continue;
  const src = readFileSync(file, "utf8");
  if (emoji.test(src)) warn(rel, "emoji found");
}

if (failures.length) {
  console.error(`design checks failed (${failures.length}):`);
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log("design checks ok");
```

- [ ] **Step 2: Run it**

Run: `node tools/js-checks/design.test.mjs`
Expected: `design checks ok` (pending files are exempt).

- [ ] **Step 3: Commit**

```bash
git add tools/js-checks/design.test.mjs
git commit -m "test(design): guard tokens, contrast, slop patterns, and emoji"
```

---

### Task 3: Core components

**Files:**
- Rewrite: `src/styles/components.css`
- Rewrite: `src/styles/toast.css`
- Modify: `src/index.html` only if a class rename is unavoidable

**Interfaces:**
- Produces: `.btn-primary`, `.btn-secondary`, `.btn-ghost`, `.btn-danger`, `.icon-btn`, `.field`, `.field-label`, `.field-help`, `.menu`, `.menu-item`, `.data-table`, `.chip`, `.segmented`, `.state-empty`, `.state-loading`, `.state-error`, `.dialog`.

- [ ] **Step 1: Replace `components.css`**

Required rules (keep every existing class name used by JS):

```css
/* Buttons */
.btn-primary, .btn-secondary, .btn-ghost, .btn-danger {
  display: inline-flex; align-items: center; justify-content: center;
  gap: var(--space-8);
  height: var(--control-h);
  padding: 0 var(--space-12);
  border-radius: var(--radius-control);
  font-size: var(--fs-body); font-weight: 600;
  border: 1px solid transparent;
  transition: background-color var(--dur-hover) var(--ease),
              border-color var(--dur-hover) var(--ease),
              color var(--dur-hover) var(--ease);
}
.btn-primary { background: var(--color-accent); color: var(--color-on-accent); }
.btn-primary:hover { background: var(--color-accent-hover); }
.btn-secondary { background: transparent; border-color: var(--color-border-control); color: var(--color-text); }
.btn-secondary:hover { background: var(--color-surface-raised); }
.btn-ghost { background: transparent; color: var(--color-text-muted); }
.btn-ghost:hover { background: var(--color-surface-raised); color: var(--color-text); }
.btn-danger { background: transparent; border-color: var(--color-error); color: var(--color-error); }
.btn-danger:hover { background: var(--color-error-soft); }
.btn-primary:disabled, .btn-secondary:disabled, .btn-ghost:disabled, .btn-danger:disabled {
  opacity: 0.4; pointer-events: none;
}
.btn-lg { height: var(--control-h-lg); padding: 0 var(--space-16); }

.icon-btn {
  display: inline-flex; align-items: center; justify-content: center;
  width: var(--icon-btn); height: var(--icon-btn);
  min-width: var(--icon-btn); min-height: var(--icon-btn);
  border-radius: var(--radius-control);
  color: var(--color-text-muted);
}
.icon-btn:hover { background: var(--color-surface-raised); color: var(--color-text); }
.icon-btn.danger:hover { background: var(--color-error-soft); color: var(--color-error); }

/* Fields */
.field { display: flex; flex-direction: column; gap: var(--space-6); }
.field-label { font-size: var(--fs-body); color: var(--color-text); }
.field-help { font-size: var(--fs-caption); line-height: var(--lh-caption); color: var(--color-text-muted); }
.field-error { font-size: var(--fs-caption); color: var(--color-error); }
input, select, textarea {
  height: var(--control-h);
  padding: 0 var(--space-10);
  background: var(--color-surface);
  border: 1px solid var(--color-border-control);
  border-radius: var(--radius-input);
}
textarea { height: auto; min-height: 96px; padding: var(--space-8) var(--space-10); resize: vertical; }
input:hover, select:hover, textarea:hover { border-color: var(--color-text-subtle); }
input:focus-visible, select:focus-visible, textarea:focus-visible {
  outline: 2px solid var(--color-accent); outline-offset: 2px;
  border-color: var(--color-accent);
}
input[aria-invalid="true"], textarea[aria-invalid="true"] { border-color: var(--color-error); }
::placeholder { color: var(--color-text-subtle); }

/* Menus */
.menu {
  min-width: 180px; padding: var(--space-4);
  background: var(--color-surface-raised);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-panel);
  box-shadow: var(--shadow-popover);
}
.menu-item {
  display: flex; align-items: center; gap: var(--space-8);
  width: 100%; height: var(--control-h); padding: 0 var(--space-8);
  border-radius: var(--radius-control);
  font-size: var(--fs-body); text-align: left;
}
.menu-item:hover, .menu-item:focus-visible { background: var(--color-surface-hover); }
.menu-item.danger { color: var(--color-error); }
.menu-separator { height: 1px; margin: var(--space-4) 0; background: var(--color-border); }

/* Tables */
.data-table { width: 100%; border-collapse: collapse; font-size: var(--fs-body); }
.data-table th {
  text-align: left; font-weight: 600; font-size: var(--fs-caption);
  line-height: var(--lh-caption); color: var(--color-text-muted);
  padding: var(--space-8) var(--space-12); border-bottom: 1px solid var(--color-border);
}
.data-table td {
  padding: var(--space-8) var(--space-12); border-bottom: 1px solid var(--color-border);
  font-variant-numeric: tabular-nums;
}
.data-table tr:last-child td { border-bottom: none; }
.data-table tbody tr:hover { background: var(--color-surface); }

/* Chips / badges / segmented */
.chip {
  display: inline-flex; align-items: center; gap: var(--space-4);
  height: var(--icon-btn); padding: 0 var(--space-6);
  border-radius: var(--radius-chip);
  border: 1px solid var(--color-border-strong);
  font-size: var(--fs-caption); line-height: var(--lh-caption);
  color: var(--color-text-muted);
}
.segmented { display: inline-flex; gap: 2px; padding: 2px; background: var(--color-surface); border-radius: var(--radius-control); }
.segmented > button { height: 26px; padding: 0 var(--space-10); border-radius: 2px; font-size: var(--fs-caption); color: var(--color-text-muted); }
.segmented > button[aria-selected="true"], .segmented > button.active { background: var(--color-surface-raised); color: var(--color-text); }

/* States */
.state-empty, .state-loading, .state-error {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  gap: var(--space-12); padding: var(--space-40);
  text-align: center; color: var(--color-text-muted);
  font-size: var(--fs-body);
}
.state-error { color: var(--color-error); }
.state-loading .loading-spinner { border-color: var(--color-border); border-top-color: var(--color-accent); }

/* Dialog */
.dialog {
  background: var(--color-surface-raised);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-modal);
  box-shadow: var(--shadow-modal);
}
```

- [ ] **Step 2: Replace `toast.css`**

```css
#toast-container { position: fixed; right: var(--space-24); bottom: var(--space-24); display: flex; flex-direction: column; gap: var(--space-8); z-index: 1000; }
.toast {
  display: flex; align-items: flex-start; gap: var(--space-8);
  max-width: 360px; padding: var(--space-10) var(--space-12);
  background: var(--color-surface-raised);
  border: 1px solid var(--color-border-strong);
  border-left: 2px solid var(--color-accent);
  border-radius: var(--radius-panel);
  box-shadow: var(--shadow-popover);
  font-size: var(--fs-meta); line-height: var(--lh-meta);
  animation: toast-in var(--dur-enter) var(--ease);
}
.toast--success { border-left-color: var(--color-success); }
.toast--warn { border-left-color: var(--color-warn); }
.toast--error { border-left-color: var(--color-error); }
@keyframes toast-in { from { opacity: 0; } to { opacity: 1; } }
```

- [ ] **Step 3: Verify**

```bash
node --check src/js/app.js
node tools/js-checks/design.test.mjs
```

Expected: `design checks ok`.

- [ ] **Step 4: Commit**

```bash
git add src/styles/components.css src/styles/toast.css
git commit -m "style(components): controls, fields, menus, tables, and states"
```

---

### Task 4: Shell, nav, and collapsible assistant

**Files:**
- Rewrite: `src/styles/layout.css`
- Modify: `src/index.html:24-63` (brand, nav, assistant header)
- Modify: `src/js/app.js`

**Interfaces:**
- Consumes: tokens from Task 1.
- Produces: `#right-sidebar.is-collapsed`, `#btn-toggle-assistant`, `aria-expanded`, `aria-current="page"` on `.nav-item`, `localStorage["ui.assistant.collapsed.<tab>"]`.

- [ ] **Step 1: Replace the shell rules in `layout.css`**

```css
#app-grid {
  display: grid;
  grid-template-columns: var(--nav-w) minmax(0, 1fr) var(--assistant-w);
  height: 100vh; width: 100vw;
}
#app-grid.assistant-collapsed { grid-template-columns: var(--nav-w) minmax(0, 1fr) var(--rail-w); }

#left-sidebar {
  display: flex; flex-direction: column;
  background: var(--color-surface);
  border-right: 1px solid var(--color-border);
  overflow: hidden;
}
.app-brand { display: flex; align-items: center; gap: var(--space-10); padding: var(--space-16) var(--space-16) var(--space-12); }
.brand-mark {
  display: grid; place-items: center;
  width: 28px; height: 28px;
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-control);
  font-family: var(--font-display); font-size: 15px; font-weight: 600;
  color: var(--color-accent);
}
.brand-name { font-size: var(--fs-body); font-weight: 600; }
.brand-tag { font-size: var(--fs-caption); line-height: var(--lh-caption); color: var(--color-text-muted); }

.nav-menu { display: flex; flex-direction: column; gap: 2px; padding: var(--space-8); }
.nav-section-label { padding: var(--space-12) var(--space-8) var(--space-4); font-size: var(--fs-caption); line-height: var(--lh-caption); color: var(--color-text-subtle); }
.nav-item {
  display: flex; align-items: center; gap: var(--space-10);
  height: var(--row-h); padding: 0 var(--space-8);
  border-left: 2px solid transparent; border-radius: 0;
  font-size: var(--fs-body); color: var(--color-text-muted); text-align: left;
}
.nav-item:hover { background: var(--color-surface-raised); color: var(--color-text); }
.nav-item.active { border-left-color: var(--color-accent); background: var(--color-accent-soft); color: var(--color-text); }
.nav-item svg { width: 16px; height: 16px; }
.sidebar-footer { margin-top: auto; padding: var(--space-12) var(--space-16); border-top: 1px solid var(--color-border); display: flex; flex-direction: column; gap: var(--space-6); }
.status-row { display: flex; align-items: center; gap: var(--space-6); font-size: var(--fs-caption); line-height: var(--lh-caption); }
.status-label { color: var(--color-text-muted); }
.status-value { margin-left: auto; color: var(--color-text); max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.status-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--color-text-subtle); }
.status-dot.ok { background: var(--color-success); }
.status-dot.warn { background: var(--color-warn); }
.status-dot.error { background: var(--color-error); }
.version-chip { font-size: 11px; color: var(--color-text-subtle); } /* raised to 12px in Task 10 cleanup */

#main-content { min-width: 0; display: flex; flex-direction: column; overflow: hidden; }
.tab-view { display: none; height: 100%; min-height: 0; }
.tab-view.active { display: flex; flex-direction: column; flex: 1; min-height: 0; }

#right-sidebar {
  display: flex; flex-direction: column;
  background: var(--color-surface);
  border-left: 1px solid var(--color-border);
  overflow: hidden;
  transition: width var(--dur-enter) var(--ease);
}
#app-grid.assistant-collapsed #right-sidebar { width: var(--rail-w); }
#app-grid.assistant-collapsed #right-sidebar > :not(.assistant-rail) { display: none; }
.assistant-rail { display: none; }
#app-grid.assistant-collapsed .assistant-rail {
  display: flex; flex-direction: column; align-items: center; gap: var(--space-12);
  padding: var(--space-12) 0; width: 100%;
}

@media (max-width: 1320px) {
  #app-grid { grid-template-columns: var(--nav-w) minmax(0, 1fr) var(--rail-w); }
}
@media (max-width: 1040px) {
  #app-grid { grid-template-columns: var(--nav-rail-w) minmax(0, 1fr) var(--rail-w); }
  .app-brand .brand-text, .nav-item span, .nav-section-label, .sidebar-footer { display: none; }
  .nav-item { justify-content: center; padding: 0; }
}
```

- [ ] **Step 2: Update `index.html` brand mark**

Replace `<div class="brand-mark">🧠</div>` with:

```html
<div class="brand-mark" aria-hidden="true">H</div>
```

- [ ] **Step 3: Add the assistant toggle + rail in `index.html`**

Inside `#right-sidebar`, before `.ai-header`:

```html
<div class="assistant-rail">
  <button id="btn-toggle-assistant-rail" class="icon-btn" aria-label="Expand assistant" title="Expand assistant">
    <i data-lucide="sparkles"></i>
  </button>
  <span id="rail-analysis-dot" class="status-dot" aria-hidden="true"></span>
  <span id="rail-inbox-count" class="chip" aria-hidden="true">0</span>
</div>
```

Inside `.ai-header`, add:

```html
<button id="btn-toggle-assistant" class="icon-btn" aria-label="Collapse assistant" aria-expanded="true" title="Collapse assistant">
  <i data-lucide="panel-right-close"></i>
</button>
```

- [ ] **Step 4: Add collapse logic in `app.js`**

```js
const COLLAPSE_KEY = (tab) => `ui.assistant.collapsed.${tab}`;
const grid = document.getElementById("app-grid");
const assistantToggle = document.getElementById("btn-toggle-assistant");
const railToggle = document.getElementById("btn-toggle-assistant-rail");

function setAssistantCollapsed(collapsed, persist = true) {
  const tab = document.querySelector(".nav-item.active")?.dataset.tab || "journal";
  grid.classList.toggle("assistant-collapsed", collapsed);
  assistantToggle?.setAttribute("aria-expanded", String(!collapsed));
  if (persist) localStorage.setItem(COLLAPSE_KEY(tab), collapsed ? "1" : "0");
  window.renderIcons?.();
}

function restoreAssistantState(tab) {
  setAssistantCollapsed(localStorage.getItem(COLLAPSE_KEY(tab)) === "1", false);
}

assistantToggle?.addEventListener("click", () => setAssistantCollapsed(true));
railToggle?.addEventListener("click", () => setAssistantCollapsed(false));
```

Call `restoreAssistantState(tab)` inside the existing tab-switch handler, next to `loadView(tab)`. Also set `aria-current` on nav items in the same handler:

```js
document.querySelectorAll(".nav-item").forEach((el) =>
  el.setAttribute("aria-current", el.classList.contains("active") ? "page" : "false"));
```

- [ ] **Step 5: Verify**

```bash
node --check src/js/app.js
node tools/js-checks/design.test.mjs
```

Expected: `design checks ok`.

- [ ] **Step 6: Commit**

```bash
git add src/styles/layout.css src/index.html src/js/app.js
git commit -m "feat(ui): measured shell, nav, and collapsible assistant (D-149)"
```

---

### Task 5: Journal + Assistant surfaces

**Files:**
- Create: `src/styles/surfaces/journal.css`
- Create: `src/styles/surfaces/assistant.css`
- Modify: `src/index.html` journal markup
- Modify: `src/js/tabs/journal.js`
- Modify: `src/js/ai-sidebar.js`
- Modify: `src/js/proposals.js`

**Interfaces:**
- Consumes: `.btn-*`, `.icon-btn`, `.menu`, `.data-table`, tokens.
- Produces: `.journal-page`, `.journal-page-header`, `.journal-page-footer`, `.entry-row`, `.entry-row.active`, `.assistant-status`, `.chat-user`, `.chat-assistant`.

- [ ] **Step 1: Markup contract**

Journal: `.journal-view-layout` → `aside.journal-list-panel` (width `--list-w`) + `section.journal-page` (max 68ch) + assistant.
Entry rows: `button.entry-row` with `.entry-row-title` (14/600), `.entry-row-meta` (12/16 muted), active gets a 2px accent left border.
Page header: title `h1.journal-page-title` (28/36), `.btn-secondary` Analyze, `.icon-btn` overflow with `.menu` containing Delete.
Page footer: `.journal-page-footer` 12/16 muted with word count + save state.
Editor: `.journal-editor` serif 16/26, max width 68ch, transparent background, no border, `padding: var(--space-32) 0`.

Assistant: header 48px, `.assistant-status` line 12/16 + 4px state bar, `.chat-user` (2px accent left rule, padding-left 12), `.chat-assistant` plain, composer bordered radius 8 with 32px send.

- [ ] **Step 2: Replace remaining `font-size`, radius, and color values in the two new CSS files with tokens.** Copy the existing selectors from `layout.css`, `components.css`, and `ai-chat.css` that belong to journal/assistant, then apply:

```css
.journal-view-layout { display: grid; grid-template-columns: var(--list-w) minmax(0, 1fr); height: 100%; min-height: 0; }
.journal-list-panel { border-right: 1px solid var(--color-border); display: flex; flex-direction: column; min-height: 0; }
.entry-row { height: 64px; padding: var(--space-8) var(--space-12); border-left: 2px solid transparent; border-radius: 0; text-align: left; width: 100%; }
.entry-row:hover { background: var(--color-surface); }
.entry-row.active { border-left-color: var(--color-accent); background: var(--color-accent-soft); }
.journal-page { max-width: 68ch; margin: 0 auto; padding: var(--space-32) var(--space-24); overflow-y: auto; }
.journal-editor { width: 100%; min-height: 320px; height: auto; padding: 0; border: none; background: transparent; border-radius: 0; font-family: var(--font-serif); font-size: var(--fs-journal); line-height: var(--lh-journal); }
```

- [ ] **Step 3: Update `journal.js:35`**

```js
rowHeight: 64,
```

- [ ] **Step 4: Replace `ai-sidebar.js` message markup**

User message: `<div class="chat-user">…</div>`; assistant: `<div class="chat-assistant">…</div>`; remove bubble wrappers and emoji. Keep `escapeHtml` on every interpolation. Replace ✓/🤖/💭 with `<i data-lucide="check|bot|message-circle">` and call `window.renderIcons?.()` after render.

- [ ] **Step 5: Update `proposals.js`**

Card classes: `.proposal-card` (border 1px, radius 8, padding 12), actions `.btn-secondary` + `.btn-ghost` at 32px. Keep escape-first.

- [ ] **Step 6: Update `index.html` link list**

Add `styles/surfaces/journal.css` and `styles/surfaces/assistant.css`; remove `styles/ai-chat.css`.

- [ ] **Step 7: Verify**

```bash
node --check src/js/tabs/journal.js
node --check src/js/ai-sidebar.js
node --check src/js/proposals.js
node tools/js-checks/proposals.test.mjs
node tools/js-checks/chat-format.test.mjs
node tools/js-checks/design.test.mjs
```

Expected: all green; `design checks ok`.

- [ ] **Step 8: Commit**

```bash
git add src/styles/surfaces/journal.css src/styles/surfaces/assistant.css src/index.html src/js/tabs/journal.js src/js/ai-sidebar.js src/js/proposals.js
git commit -m "style(journal,assistant): notebook page and flat chat surface"
```

---

### Task 6: Tasks + Calendar

**Files:**
- Create: `src/styles/surfaces/tasks.css`, `src/styles/surfaces/calendar.css`
- Modify: `src/js/tabs/tasks.js`, `src/js/tabs/tasks-calendar.js`, `src/js/tabs/board-model.js` (no logic change)
- Modify: `src/index.html` (link list, filter markup)

**Interfaces:**
- Produces: `.task-card`, `.task-card-menu`, `.priority-rule`, `.task-filters`; `openTaskMenu(taskId, anchorEl)`; menu items call `task_update_status`.

- [ ] **Step 1: Card markup contract**

```html
<article class="task-card" data-id="…">
  <span class="priority-rule priority-high" aria-hidden="true"></span>
  <button class="task-card-title">…</button>
  <div class="task-card-meta"><span>Due …</span><span class="priority-label">High</span></div>
  <button class="icon-btn task-card-menu" aria-label="Task actions" aria-haspopup="menu" aria-expanded="false">
    <i data-lucide="more-horizontal"></i>
  </button>
</article>
```

The menu contains To Do / In Progress / Done / Archive (or Restore) and calls the existing `task_update_status`; after success, re-render.

- [ ] **Step 2: Add the menu builder to `tasks.js`**

```js
function openTaskMenu(taskId, anchor) {
  document.querySelectorAll(".menu.task-menu").forEach((m) => m.remove());
  const task = allTasks.find((t) => t.id === taskId);
  if (!task) return;
  const items = task.archived
    ? [["Restore", () => setStatus(taskId, "todo")]]
    : [["To Do", () => setStatus(taskId, "todo")],
       ["In Progress", () => setStatus(taskId, "in_progress")],
       ["Done", () => setStatus(taskId, "done")],
       ["Archive", () => setStatus(taskId, "archived")]];
  const menu = document.createElement("div");
  menu.className = "menu task-menu";
  menu.setAttribute("role", "menu");
  for (const [label, fn] of items) {
    const b = document.createElement("button");
    b.className = "menu-item";
    b.setAttribute("role", "menuitem");
    b.textContent = label;
    b.addEventListener("click", () => { menu.remove(); fn(); });
    menu.appendChild(b);
  }
  document.body.appendChild(menu);
  const r = anchor.getBoundingClientRect();
  menu.style.position = "fixed";
  menu.style.top = `${r.bottom + 4}px`;
  menu.style.left = `${Math.min(r.left, window.innerWidth - 200)}px`;
  anchor.setAttribute("aria-expanded", "true");
  menu.querySelector("button")?.focus();
  const close = (e) => {
    if (e.type === "keydown" && e.key !== "Escape") return;
    menu.remove(); anchor.setAttribute("aria-expanded", "false"); anchor.focus();
    document.removeEventListener("keydown", close);
    document.removeEventListener("click", close, true);
  };
  document.addEventListener("keydown", close);
  document.addEventListener("click", close, true);
}
```

- [ ] **Step 3: Calendar menu**

Add the same `⋯` button on each chip; menu items: `Schedule…` (sets `due_date` via the existing `task_update` path or a date input in a small dialog) and `Clear date`.

- [ ] **Step 4: CSS**

```css
.task-board { display: grid; grid-template-columns: repeat(4, minmax(280px, 1fr)); gap: var(--space-16); }
.task-column { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: var(--radius-panel); }
.task-card { position: relative; display: flex; flex-direction: column; gap: var(--space-6); padding: var(--space-12); background: var(--color-surface-raised); border: 1px solid var(--color-border); border-radius: var(--radius-input); }
.priority-rule { position: absolute; left: 0; top: 0; bottom: 0; width: 2px; border-radius: var(--radius-input) 0 0 var(--radius-input); }
.priority-high { background: var(--color-error); }
.priority-medium { background: var(--color-warn); }
.priority-low { background: var(--color-text-subtle); }
.priority-urgent { background: var(--color-error); }
.task-card-title { font-size: var(--fs-body); font-weight: 600; text-align: left; }
.task-card-meta { display: flex; gap: var(--space-8); font-size: var(--fs-caption); line-height: var(--lh-caption); color: var(--color-text-muted); }
.task-card-menu { position: absolute; top: var(--space-8); right: var(--space-8); }
```

Calendar: cells 40px minimum, chips 24px, no card shadows.

- [ ] **Step 5: Verify**

```bash
node --check src/js/tabs/tasks.js
node --check src/js/tabs/tasks-calendar.js
node tools/js-checks/board.test.mjs
node tools/js-checks/design.test.mjs
```

Expected: green.

- [ ] **Step 6: Commit**

```bash
git add src/styles/surfaces/tasks.css src/styles/surfaces/calendar.css src/index.html src/js/tabs/tasks.js src/js/tabs/tasks-calendar.js
git commit -m "feat(tasks): workbench board, card menus, and non-drag status paths"
```

---

### Task 7: Mood

**Files:**
- Create: `src/styles/surfaces/mood.css`
- Modify: `src/js/tabs/mood.js`
- Modify: `src/index.html` link list

- [ ] **Step 1: Chips become a radiogroup**

Each chip: `role="radio"` container `role="radiogroup"`, `aria-checked`, 32px height, 4px radius, selected = `border-color: var(--color-accent)` + `background: var(--color-accent-soft)`. Space selects; arrows move.

- [ ] **Step 2: Calendar + trend CSS**

40px cells, mood as a 3px accent underline with a text label, today outlined. Chart: single accent line, muted axes, no area fill.

- [ ] **Step 3: Clear action**

Replace the red button with `<button class="btn-ghost">Clear this day</button>`. Keep the existing command.

- [ ] **Step 4: Verify**

```bash
node --check src/js/tabs/mood.js
node tools/js-checks/mood.test.mjs
node tools/js-checks/design.test.mjs
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src/styles/surfaces/mood.css src/index.html src/js/tabs/mood.js
git commit -m "style(mood): calm chips, labelled calendar, single-line trend"
```

---

### Task 8: System + Settings

**Files:**
- Create: `src/styles/surfaces/system.css`, `src/styles/surfaces/settings.css`
- Delete: `src/styles/settings-diagnostics.css`
- Modify: `src/js/tabs/system.js`, `src/js/tabs/settings.js`, `src/index.html`, `src/index.html:links`

- [ ] **Step 1: Replace card grids with tables**

System runtime: `<table class="data-table">` with `<th scope="row">` labels and `<td class="mono">` values. Paths: `<code class="path">` with `direction: rtl; text-overflow: ellipsis;` or JS middle-truncation, plus a 24px copy button with `aria-label`.

Engines/queue: tables. Log: `<pre class="log-view">` with mono 12/18 and segmented level filter.

- [ ] **Step 2: Settings layout**

```css
.settings-page { max-width: 720px; padding: var(--space-24); }
.settings-section { padding: var(--space-20) 0; border-bottom: 1px solid var(--color-border); }
.settings-section h2 { font-size: var(--fs-subtitle); line-height: var(--lh-subtitle); margin-bottom: var(--space-4); }
.settings-section p { color: var(--color-text-muted); margin-bottom: var(--space-16); }
.field-row { display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-16); align-items: center; }
.danger-zone h2 { color: var(--color-error); }
```

- [ ] **Step 3: Inline errors**

Vault password change: render `#vault-password-error` (`.field-error`, `role="alert"`) beside the field; set `aria-invalid` on the input; never a global toast for validation.

- [ ] **Step 4: Verify**

```bash
node --check src/js/tabs/system.js
node --check src/js/tabs/settings.js
node tools/js-checks/design.test.mjs
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src/styles/surfaces/system.css src/styles/surfaces/settings.css src/index.html src/js/tabs/system.js src/js/tabs/settings.js
git rm src/styles/settings-diagnostics.css
git commit -m "style(system,settings): definition tables and form sections"
```

---

### Task 9: Overlays + remaining emoji

**Files:**
- Create: `src/styles/surfaces/overlays.css`
- Delete: `src/styles/onboarding.css`, `src/styles/vault.css`
- Modify: `src/js/onboarding.js`, `src/js/vault.js`, `src/js/notifications.js`, `src/index.html`

- [ ] **Step 1: Overlay CSS**

Vault panel 400px, radius 12, shadow modal, one field, 36px primary, inline error. Onboarding panel 640px, step list, 2px progress, one decision per screen.

- [ ] **Step 2: Emoji → Lucide**

Replace ✅/⚠/❌ in onboarding, 🗑/🪄/📓/🔐/🔒 in index, and ⚡/📅/🔒 in tasks with `data-lucide` icons. Call `renderIcons()` after every dynamic render.

- [ ] **Step 3: Toasts**

`#toast-container` sets `role="status"` and `aria-live="polite"`; each toast gets a dismiss `icon-btn` with accessible name; auto-dismiss 5000ms.

- [ ] **Step 4: Verify**

```bash
node --check src/js/onboarding.js
node --check src/js/vault.js
node --check src/js/notifications.js
node tools/js-checks/design.test.mjs
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src/styles/surfaces/overlays.css src/index.html src/js/onboarding.js src/js/vault.js src/js/notifications.js
git rm src/styles/onboarding.css src/styles/vault.css
git commit -m "style(overlays): vault and onboarding dialogs, lucide icons, live toasts"
```

---

### Task 10: Cleanup, audit, docs

**Files:**
- Modify: `src/styles/tokens.css` (remove legacy aliases)
- Modify: `tools/js-checks/design.test.mjs` (`PENDING` → empty)
- Modify: `src/styles/layout.css` (version chip 12px)
- Modify: `AGENTS.md`, `CHANGELOG.md`

- [ ] **Step 1: Remove every legacy alias** from `tokens.css` (`--color-bg`, `--color-surface-soft`, `--color-border-soft`, `--color-muted`, `--color-ai`, `--radius`, `--radius-sm`, `--space`, `--transition`, `--shadow-sm`, `--shadow-md`, `--sidebar-left-w`, `--sidebar-right-w`).

- [ ] **Step 2: Find stragglers**

```bash
grep -rn "var(--color-bg\|var(--color-muted\|var(--color-ai\|var(--color-surface-soft\|var(--color-border-soft\|var(--radius)\|var(--radius-sm\|var(--shadow-sm\|var(--shadow-md\|var(--sidebar-" src/styles src/index.html
```

Fix each to its token equivalent.

- [ ] **Step 3: Empty `PENDING` in the guard test**, fix every reported violation, then run:

```bash
node tools/js-checks/design.test.mjs
node --check src/js/app.js
```

Expected: `design checks ok`.

- [ ] **Step 4: Full harness + Rust sanity**

```bash
for f in tools/js-checks/*.test.mjs; do node "$f"; done
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all harnesses pass; `cargo check` 0 errors; 44/44 tests.

- [ ] **Step 5: Live audit**

With the release build running under CDP `:9222`, run `tools/ui-audit.mjs`; verify: 0 unnamed buttons, 0 targets <24px, no font-size <12px, no `999px` radii, ≤2 shadows, screenshots of all 5 tabs plus collapsed/expanded assistant, 200% zoom, reduced motion, forced colors.

- [ ] **Step 6: Docs**

- `AGENTS.md`: status line for this pass, file structure update (`styles/surfaces/`), D-149–D-151, conventions (token usage, guard test).
- `CHANGELOG.md`: Added/Changed/Fixed entries naming every file.

- [ ] **Step 7: Commit**

```bash
git add -f docs/superpowers/plans/2026-10-21-ui-ux-redesign.md
git add src/styles src/index.html src/js tools/js-checks/design.test.mjs AGENTS.md CHANGELOG.md
git commit -m "style(ui): finish token migration, audit pass, and docs (D-149..D-151)"
```

---

## Self-Review

**Spec coverage:** tokens (§3.2–3.4 → Task 1), type ramp (Task 1), shell/sidebar (Task 4), Journal/Assistant (Task 5), Tasks/Calendar (Task 6), Mood (Task 7), System/Settings (Task 8), overlays/icons (Task 9), guardrails (Task 2), audit (Task 10), decisions/docs (Task 10). No gaps.

**Placeholder scan:** no TBD/TODO; every task names exact files, classes, and verification commands.

**Type consistency:** token names match Task 1 across all tasks; class names (`.task-card`, `.entry-row`, `.field-row`, `.data-table`) are used consistently; `setStatus` refers to the existing `task_update_status` wrapper in `tasks.js`.
