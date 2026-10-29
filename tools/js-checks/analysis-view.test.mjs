import assert from "node:assert/strict";
import { extractedItemsHtml } from "../../src/js/analysis-view.js";

const has = (html, sub) =>
  assert.ok(html.includes(sub), `expected ${JSON.stringify(html)} to contain ${JSON.stringify(sub)}`);
const notHas = (html, sub) =>
  assert.ok(!html.includes(sub), `expected ${JSON.stringify(html)} NOT to contain ${JSON.stringify(sub)}`);

// 1. Structured tasks (post-f5bb6c8 wire shape) — never [object Object].
const all = extractedItemsHtml({
  id: "e1",
  tasks: [{ title: "Fix <b>bug</b>", due_date: "2026-10-10", priority: "high" }],
  events: [{ title: "Dentist", start_at: "2026-10-10T14:00:00Z", location: "Clinic" }],
  notes: [{ title: "", content: "n".repeat(400), tags: [] }],
  facts: [{ fact_key: "k", content: "Likes <tea>", category: "preference" }],
});
notHas(all, "[object Object]"); // THE regression this module exists for
has(all, "Fix &lt;b&gt;bug&lt;/b&gt;");
notHas(all, "<b>bug</b>");
has(all, "2026-10-10");
has(all, "high");
has(all, "Dentist");
has(all, "Clinic");
notHas(all, "Invalid Date"); // bad/missing dates must not leak "Invalid Date"
has(all, "Untitled note"); // empty note title fallback
has(all, "Likes &lt;tea&gt;");
has(all, "preference");
assert.ok(!all.includes("n".repeat(121)), "note snippet must be truncated");
has(all, "…");

// 2. Legacy string tasks still render as plain titles.
const legacy = extractedItemsHtml({ tasks: ["Buy milk"] });
has(legacy, "Buy milk");
notHas(legacy, "[object Object]");

// 3. Attribute/HTML injection in titles stays inert (escape-first).
const evil = extractedItemsHtml({
  tasks: [{ title: `"><img src=x onerror=alert(1)>` }],
  events: [{ title: `'); DROP TABLE tasks;--`, start_at: "nope" }],
});
notHas(evil, "<img");
has(evil, "&lt;img");
has(evil, "&#039;");
notHas(evil, "Invalid Date"); // unparseable start_at falls back to the raw string

// 4. Empty / missing → no section content.
assert.equal(extractedItemsHtml({}), "");
assert.equal(extractedItemsHtml(null), "");
assert.equal(extractedItemsHtml(undefined), "");

console.log("analysis-view: all assertions passed");
