// D-148 guard: fails on any top-level snake_case key in an invoke() payload.
// Top-level command args must be camelCase (Tauri does exact single-key lookup,
// no case fallback). Nested struct keys (depth >= 2) are exempt — they follow
// serde field names (snake_case).
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../src/js");
const snake = /^[a-z][a-z0-9]*(_[a-z0-9]+)+$/;

function scan(text) {
  const out = [];
  const call = /\binvoke\s*\(\s*/g;
  let m;
  while ((m = call.exec(text)) !== null) {
    let i = m.index + m[0].length;
    if (i >= text.length) continue;
    const q = text[i];
    if (q !== '"' && q !== "'" && q !== "`") continue;
    i += 1;
    while (i < text.length && text[i] !== q) i += 1;
    i += 1;
    while (i < text.length && /\s/.test(text[i])) i += 1;
    if (text[i] !== ",") continue;
    i += 1;
    while (i < text.length && /\s/.test(text[i])) i += 1;
    if (text[i] !== "{") continue;

    // Extract the balanced payload object { i .. j }.
    let depth = 0;
    let instr = null;
    let j = i;
    for (; j < text.length; j++) {
      const c = text[j];
      if (instr) {
        if (c === "\\") { j += 1; continue; }
        if (c === instr) instr = null;
      } else if (c === '"' || c === "'" || c === "`") {
        instr = c;
      } else if (c === "{" || c === "[" || c === "(") {
        depth += 1;
      } else if (c === "}" || c === "]" || c === ")") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    const obj = text.slice(i + 1, j);
    const invokeLine = text.slice(0, m.index).split("\n").length;

    // Walk top-level entries only (d2 tracks nesting inside the payload).
    let d2 = 0;
    let s = null;
    let token = "";
    for (let k = 0; k < obj.length; k++) {
      const ch = obj[k];
      if (s) {
        token += ch;
        if (ch === "\\") { token += obj[k + 1] ?? ""; k += 1; continue; }
        if (ch === s) s = null;
        continue;
      }
      if (ch === '"' || ch === "'" || ch === "`") { s = ch; token += ch; continue; }
      if (ch === "{" || ch === "[" || ch === "(") { d2 += 1; token += ch; continue; }
      if (ch === "}" || ch === "]" || ch === ")") { d2 -= 1; token += ch; continue; }
      if (d2 > 0) { token += ch; continue; }
      if (ch === ",") {
        const key = token.trim().replace(/^["'`]|["'`]$/g, "");
        if (snake.test(key)) out.push({ key, line: invokeLine });
        token = "";
        continue;
      }
      if (ch === ":") {
        const key = token.trim().replace(/^["'`]|["'`]$/g, "");
        if (snake.test(key)) out.push({ key, line: invokeLine });
        token = "";
        // Skip the value until the next top-level comma.
        let vd = 0;
        let iv = null;
        k += 1;
        for (; k < obj.length; k++) {
          const c2 = obj[k];
          if (iv) {
            if (c2 === "\\") { k += 1; continue; }
            if (c2 === iv) iv = null;
          } else if (c2 === '"' || c2 === "'" || c2 === "`") {
            iv = c2;
          } else if (c2 === "{" || c2 === "[" || c2 === "(") {
            vd += 1;
          } else if (c2 === "}" || c2 === "]" || c2 === ")") {
            vd -= 1;
          } else if (c2 === "," && vd === 0) {
            break;
          }
        }
        token = "";
        continue;
      }
      token += ch;
    }
  }
  return out;
}

const files = [];
(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.endsWith(".js")) files.push(p);
  }
})(root);

const violations = [];
let siteCount = 0;
for (const f of files.sort()) {
  const text = fs.readFileSync(f, "utf8");
  // Count payload-bearing invoke sites (command + object literal) for the
  // vacuous-pass guard below.
  siteCount += (text.match(/\binvoke\s*\(\s*["'`][^"'`]+["'`]\s*,\s*\{/g) || []).length;
  for (const v of scan(text)) {
    violations.push(`${path.relative(process.cwd(), f).replace(/\\/g, "/")}:${v.line}: ${v.key}`);
  }
}

assert.ok(
  siteCount >= 50,
  `vacuous guard: only ${siteCount} payload-bearing invoke sites found (expected >= 50)`,
);
assert.deepEqual(
  violations,
  [],
  `top-level snake_case invoke keys found (fix to camelCase):\n${violations.join("\n")}`,
);
console.log(`ipc-conventions: all assertions passed (${siteCount} invoke sites scanned)`);
