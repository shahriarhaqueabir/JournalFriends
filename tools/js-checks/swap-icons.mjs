// One-off: swap static Lucide <i data-lucide="X"></i> for <hj-icon name="X">.
// Preserves any existing attributes (class/title/aria) on the <i>. Adds
// aria-hidden unless the tag already carried aria-label/title.
import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = process.cwd();
const TARGETS = process.argv.slice(2);
if (!TARGETS.length) {
  console.error("usage: node swap-icons.mjs <file...>");
  process.exit(1);
}

const re = /<i\b([^>]*?)\bdata-lucide="([a-z0-9-]+)"([^>]*)><\/i>/g;

let total = 0;
for (const t of TARGETS) {
  const src = readFileSync(t, "utf8");
  const out = src.replace(re, (_m, pre, name, post) => {
    const attrs = `${pre} ${post}`.replace(/\s+/g, " ").trim();
    const hasLabel = /aria-label=|title=/.test(attrs);
    const keep = attrs.replace(/\bdata-lucide="[^"]*"/, "").replace(/\s+/g, " ").trim();
    const a11y = hasLabel ? "" : ' aria-hidden="true"';
    total++;
    return `<hj-icon name="${name}" size="16"${a11y}${keep ? " " + keep : ""}></hj-icon>`;
  });
  if (out !== src) writeFileSync(t, out);
}
console.log(`swapped ${total} icons`);
