import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = process.cwd();

// Files not yet migrated. Each phase removes its entries; Task 10 empties this.
const PENDING = new Set([]);

// Hidden tabs are out of redesign scope and keep their emoji for now.
const EMOJI_ALLOW = new Set([
  "src/js/tabs/events.js",
  "src/js/tabs/inbox.js",
  "src/js/tabs/memory.js",
  "src/js/tabs/reports.js",
  "src/views/about.html",
  "src/views/memory.html",
]);

const failures = new Set();
const warn = (file, msg) => failures.add(`${file}: ${msg}`);

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      if (p.includes("assets") || p.includes("node_modules")) continue;
      walk(p, out);
    } else {
      out.push(p);
    }
  }
  return out;
}

const rel = (p) => relative(ROOT, p).replaceAll("\\", "/");
const files = walk(join(ROOT, "src"));
const css = files.filter((f) => f.endsWith(".css"));

// ── Token contrast ───────────────────────────────────────────
const tokenSrc = readFileSync(join(ROOT, "src/styles/tokens.css"), "utf8");
const tokens = new Map();
for (const m of tokenSrc.matchAll(/--([a-z0-9-]+):\s*(#[0-9A-Fa-f]{6})/g)) {
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
  const l1 = lum(a);
  const l2 = lum(b);
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

// ── CSS rules ────────────────────────────────────────────────
const elevationTokens = (tokenSrc.match(/--elev-[0-9]+:/g) || []).length;
if (elevationTokens < 4) warn("tokens.css", `expected elevation scale 0-3, found ${elevationTokens}`);

// Required semantic motion tokens (D-150 hybrid motion model).
for (const t of ["dur-hover", "dur-enter", "dur-overlay", "dur-exit", "dur-tab", "dur-stagger"]) {
  if (!new RegExp(`--${t}:`).test(tokenSrc)) warn("tokens.css", `missing motion token --${t}`);
}
const allowedDurations = new Set([0, 120, 160, 180, 200, 240, 40]);

const lineOf = (src, index) => src.slice(0, index).split("\n").length;

for (const file of css) {
  const path = rel(file);
  const src = readFileSync(file, "utf8");
  const pending = PENDING.has(path);

  if (path !== "src/styles/tokens.css") {
    if (pending) continue;

    for (const m of src.matchAll(/border-radius:\s*999px/g)) {
      warn(path, `pill radius at line ${lineOf(src, m.index)}`);
    }
    for (const m of src.matchAll(/font-size:\s*(\d+(?:\.\d+)?)px/g)) {
      if (Number(m[1]) < 12) warn(path, `font-size ${m[1]}px at line ${lineOf(src, m.index)}`);
    }
    for (const m of src.matchAll(/font-size:\s*(0?\.\d+|0)rem/g)) {
      if (Number(m[1]) < 0.75) warn(path, `font-size ${m[1]}rem at line ${lineOf(src, m.index)}`);
    }
    for (const m of src.matchAll(/transition(?:-duration)?:[^;]*?(\d+(?:\.\d+)?)(ms|s)/g)) {
      const ms = m[2] === "s" ? Number(m[1]) * 1000 : Number(m[1]);
      if (!allowedDurations.has(ms)) {
        warn(path, `transition ${ms}ms at line ${lineOf(src, m.index)}`);
      }
    }
    for (const m of src.matchAll(/#[0-9A-Fa-f]{3,8}\b/g)) {
      warn(path, `hardcoded color ${m[0]} at line ${lineOf(src, m.index)}`);
    }
    for (const m of src.matchAll(/box-shadow:\s*([^;]+)/g)) {
      const v = m[1].trim();
      if (!v.startsWith("var(--shadow-") && !v.startsWith("var(--elev-") && !v.startsWith("var(--accent-glow")) {
        warn(path, `literal box-shadow at line ${lineOf(src, m.index)}`);
      }
    }
    // Never animate backdrop-filter (paint-cost trap; D-150 depth discipline).
    for (const m of src.matchAll(/transition:[^;]*backdrop-filter|animation:[^;]*blur|transition:[^;]*blur\(/g)) {
      warn(path, `animated backdrop-filter/blur at line ${lineOf(src, m.index)}`);
    }
    if (/outline:\s*none/.test(src)) {
      warn(path, "outline: none is forbidden (use :focus-visible)");
    }
  }
}

// ── Emoji ────────────────────────────────────────────────────
const emojiRe = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/gu;
for (const file of files.filter((f) => /\.(js|html|css)$/.test(f))) {
  const path = rel(file);
  if (EMOJI_ALLOW.has(path) || PENDING.has(path)) continue;
  const src = readFileSync(file, "utf8");
  if (src.match(emojiRe)) warn(path, "emoji found");
}

// ── Icon set integrity ───────────────────────────────────────
const iconDir = join(ROOT, "src/assets/libs/animated-icons/lucide");
let vendoredIcons = new Set();
try {
  vendoredIcons = new Set(
    readdirSync(iconDir)
      .filter((f) => f.endsWith(".svg"))
      .map((f) => f.replace(/\.svg$/, ""))
  );
} catch {
  warn("assets/libs/animated-icons/lucide", "icon directory missing");
}

for (const file of files.filter((f) => /\.(js|html)$/.test(f))) {
  const path = rel(file);
  if (PENDING.has(path)) continue;
  const src = readFileSync(file, "utf8");
  for (const m of src.matchAll(/<hj-icon[^>]*\bname=["']([a-z0-9-]+)["']/g)) {
    if (!vendoredIcons.has(m[1])) {
      warn(path, `hj-icon name "${m[1]}" not vendored (line ${lineOf(src, m.index)})`);
    }
  }
}

// ── Report ───────────────────────────────────────────────────
if (failures.size) {
  console.error(`design checks failed (${failures.size}):`);
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log(`design checks ok (${PENDING.size} pending files)`);
