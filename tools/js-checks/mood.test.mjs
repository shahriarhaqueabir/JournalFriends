import assert from "node:assert/strict";
import { localDateKey, makeKey, moodColor, moodLabel, renderMoodChips } from "../../src/js/tabs/mood.js";

const has = (html, sub) =>
  assert.ok(html.includes(sub), `expected ${JSON.stringify(html)} to contain ${JSON.stringify(sub)}`);
const notHas = (html, sub) =>
  assert.ok(!html.includes(sub), `expected ${JSON.stringify(html)} NOT to contain ${JSON.stringify(sub)}`);

assert.equal(localDateKey(new Date(2026, 8, 5)), "2026-09-05");
assert.equal(makeKey(2026, 8, 5), "2026-09-05");
assert.equal(moodColor("calm").length, 7);
assert.equal(moodColor("nope"), moodColor("unknown-defensive")); // unknown maps to the fallback
assert.equal(moodLabel("focused"), "Focused");

const chips = renderMoodChips("sad");
has(chips, 'data-mood="sad"');
has(chips, "mood-chip selected");
notHas(chips, "<script");
assert.equal(renderMoodChips("sad").trim().startsWith("<button"), true);
console.log("mood: all assertions passed");