import assert from "node:assert/strict";

// Tag categorization pure logic test
function groupTasksByTag(tasks) {
  const tagMap = new Map();
  const untagged = [];
  tasks.forEach(task => {
    const tag = (task.project || "").trim();
    if (!tag) {
      untagged.push(task);
    } else {
      if (!tagMap.has(tag)) tagMap.set(tag, []);
      tagMap.get(tag).push(task);
    }
  });
  return {
    tags: Array.from(tagMap.keys()).sort((a, b) => a.localeCompare(b)),
    tagMap,
    untagged
  };
}

const mockTasks = [
  { id: "1", title: "Task 1", project: "Work", status: "todo" },
  { id: "2", title: "Task 2", project: "", status: "in_progress" },
  { id: "3", title: "Task 3", project: null, status: "done" },
  { id: "4", title: "Task 4", project: "Personal", status: "todo" },
  { id: "5", title: "Task 5", project: "Work", status: "done" },
];

const { tags, tagMap, untagged } = groupTasksByTag(mockTasks);

assert.deepEqual(tags, ["Personal", "Work"], "tags sorted alphabetically");
assert.equal(tagMap.get("Work").length, 2, "2 tasks in Work tag");
assert.equal(tagMap.get("Personal").length, 1, "1 task in Personal tag");
assert.equal(untagged.length, 2, "2 untagged tasks (empty string and null)");

console.log("task-tags: all assertions passed");