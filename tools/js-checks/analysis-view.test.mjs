import assert from "node:assert/strict";
import { extractedItemsHtml } from "../../src/js/analysis-view.js";

const has = (html, sub) =>
  assert.ok(html.includes(sub), `expected ${JSON.stringify(html)} to contain ${JSON.stringify(sub)}`);
const notHas = (html, sub) =>
  assert.ok(!html.includes(sub), `expected ${JSON.stringify(html)} NOT to contain ${JSON.stringify(sub)}`);

// 1. Tasks render; notes/facts/events are counts only (D-165/D-167).
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

// Non-task items are internal: no titles, content, tags, categories, or
// interactive-looking groups — just a count summary.
has(all, "I've saved 1 note and 1 fact to memory for future context.");
has(all, "I've added 1 event to your calendar.");
notHas(all, "Dentist");
notHas(all, "Clinic");
notHas(all, "Likes");
notHas(all, "preference");
notHas(all, "Untitled note");
assert.ok(!all.includes("n".repeat(121)), "note content must not leak into the summary");
notHas(all, "extracted-cat");
notHas(all, "Invalid Date");

// 2. Legacy string tasks still render as plain titles.
const legacy = extractedItemsHtml({ tasks: ["Buy milk"] });
has(legacy, "Buy milk");
notHas(legacy, "[object Object]");

// 3. Attribute/HTML injection in titles stays inert (escape-first). Non-task
// titles never reach the markup at all.
const evil = extractedItemsHtml({
  tasks: [{ title: `"><img src=x onerror=alert('1')>` }],
  events: [{ title: `'); DROP TABLE tasks;--`, start_at: "nope" }],
});
notHas(evil, "<img");
has(evil, "&lt;img");
has(evil, "&#039;");
notHas(evil, "DROP TABLE");
notHas(evil, "Invalid Date"); // unparseable event dates are not rendered

// 4. Plurals and event-only summaries.
const many = extractedItemsHtml({
  notes: [{ content: "a" }, { content: "b" }],
  facts: [{ content: "c" }, { content: "d" }, { content: "e" }],
  events: [{ title: "One" }, { title: "Two" }],
});
has(many, "I've saved 2 notes and 3 facts to memory for future context.");
has(many, "I've added 2 events to your calendar.");
notHas(many, "One");
notHas(many, "Two");

const eventOnly = extractedItemsHtml({ events: [{ title: "Standup" }] });
has(eventOnly, "I've added 1 event to your calendar.");
notHas(eventOnly, "to memory");
notHas(eventOnly, "Standup");

// 5. Empty / invalid non-task items do not inflate the counts.
const blanks = extractedItemsHtml({
  notes: [{ content: "   " }],
  facts: [{ fact_key: "", content: "" }],
  events: [{ title: "", start_at: "" }],
});
assert.equal(blanks, "");

// 6. Empty / missing → no section content.
assert.equal(extractedItemsHtml({}), "");
assert.equal(extractedItemsHtml(null), "");
assert.equal(extractedItemsHtml(undefined), "");

console.log("analysis-view: all assertions passed");
