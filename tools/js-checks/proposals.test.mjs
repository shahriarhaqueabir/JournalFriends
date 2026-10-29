import assert from "node:assert/strict";
import { proposalCardHtml } from "../../src/js/proposals.js";

const card = proposalCardHtml({
  id: 'a<b"c',
  item_type: "task",
  proposed_title: "Pay <bill> on Monday",
  source_text: "From your entry: 'must <pay> the bill'",
  confidence: 0.9,
});

assert.ok(card.includes("ai-proposal-card"), "card class present");
assert.ok(card.includes("&lt;bill&gt;"), "title escaped");
assert.ok(!card.includes("<bill>"), "raw title absent");
assert.ok(card.includes('data-id="a&lt;b&quot;c"'), "id attribute escaped");
assert.ok(!card.includes('data-id="a<b'), "raw id attribute absent");
assert.ok(!card.includes("<script"), "no tag smuggling");
assert.ok(card.includes(">Create<"), "untagged proposals default to a Create action");

function labelFor(proposed_data) {
  return proposalCardHtml({
    id: "p1",
    item_type: "event",
    proposed_title: "Dentist",
    proposed_data,
  });
}

assert.ok(labelFor('{"type":"event_create","title":"Dentist"}').includes(">Create<"));
assert.ok(labelFor('{"type":"event_update","id":"e1"}').includes(">Update<"));
assert.ok(labelFor('{"type":"event_delete","id":"e1"}').includes(">Delete<"));
assert.ok(labelFor('{"type":"note_update","id":"n1"}').includes(">Update<"));
assert.ok(labelFor('{"type":"note_delete","id":"n1"}').includes(">Delete<"));
assert.ok(labelFor('{"type":"task_complete","id":"t1"}').includes(">Complete<"));
assert.ok(labelFor("not json").includes(">Create<"), "malformed payload falls back to Create");
assert.ok(labelFor(undefined).includes(">Create<"), "missing payload falls back to Create");
console.log("proposals: all assertions passed");