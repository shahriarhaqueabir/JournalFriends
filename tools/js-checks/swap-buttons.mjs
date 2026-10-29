// One-off: convert simple <button class="btn-X" otherAttrs>Text</button> to
// <hj-button variant="Y" otherAttrs>Text</hj-button>. Only converts buttons
// whose class attribute is EXACTLY one known variant (no extra classes) so
// class-toggled controls (hidden, vault-button, copy-btn) are never touched.
import { readFileSync, writeFileSync } from "node:fs";

const MAP = { "btn-primary": ["primary"], "btn-secondary": ["secondary"], "btn-ghost": ["ghost"], "btn-ghost-sm": ["ghost", "sm"], "btn-danger": ["danger"] };

for (const file of process.argv.slice(2)) {
  const src = readFileSync(file, "utf8");
  const out = src.replace(/<button\b([^>]*?)class="(btn-[a-z-]+)"([^>]*)>([\s\S]*?)<\/button>/g, (m, pre, cls, post, inner) => {
    const v = MAP[cls];
    if (!v || /class="[^"]*\s/.test(`class="${cls}"`)) return m; // unknown or multi-class
    const attrs = `${pre} ${post}`.replace(/\s+/g, " ").trim();
    const variant = v[0];
    const size = v[1] ? ` size="${v[1]}"` : "";
    return `<hj-button variant="${variant}"${size}${attrs ? " " + attrs : ""}>${inner}</hj-button>`;
  });
  if (out !== src) { writeFileSync(file, out); console.log(`updated ${file}`); }
}
