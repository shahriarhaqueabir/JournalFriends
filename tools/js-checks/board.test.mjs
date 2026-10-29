import assert from "node:assert/strict";
import { BOARD_LAYOUTS, columnStatusForTask, dropStatusFor } from "../../src/js/tabs/board-model.js";

assert.deepEqual(
  BOARD_LAYOUTS.inprogress.map(c => c.status),
  ["todo", "in_progress", "done", "archived"],
  "in-progress board columns"
);
assert.deepEqual(
  BOARD_LAYOUTS.completed.map(c => c.status),
  ["archived"],
  "completed board column"
);

assert.equal(columnStatusForTask("todo"), "todo", "todo -> To Do");
assert.equal(columnStatusForTask("in_progress"), "in_progress", "in_progress -> In Progress");
assert.equal(columnStatusForTask({ status: "in_progress", archived: false }), "in_progress", "in_progress object -> In Progress");
assert.equal(columnStatusForTask("done"), "done", "done -> Done");
assert.equal(columnStatusForTask("cancelled"), null, "cancelled hidden");

// Archived is now a flag with status staying 'done'.
assert.equal(columnStatusForTask({ status: "done", archived: true }), "archived", "done+archived -> Archived");
assert.equal(columnStatusForTask({ status: "todo", archived: true }), "archived", "todo+archived -> Archived");
assert.equal(columnStatusForTask({ status: "done", archived: false }), "done", "done, not archived -> Done");
assert.equal(columnStatusForTask({ status: "todo", archived: false }), "todo", "todo, not archived -> To Do");
assert.equal(columnStatusForTask({ status: "in_progress", archived: true }), "archived", "archived flag wins");

assert.equal(dropStatusFor("todo"), "todo", "drop on To Do normalizes");
assert.equal(dropStatusFor("done"), "done", "drop on Done");
assert.equal(dropStatusFor("archived"), "archived", "drop on Archived");

// Round-trip: drop on each column reproduces the status that maps back to it.
for (const col of ["todo", "in_progress", "done", "archived"]) {
  assert.equal(columnStatusForTask(dropStatusFor(col)), col, `round-trip ${col}`);
}
console.log("board-model: all assertions passed");