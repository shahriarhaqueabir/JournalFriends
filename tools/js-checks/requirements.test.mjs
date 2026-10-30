import assert from "node:assert/strict";
import {
  requirementRows,
  requirementsTableHtml,
} from "../../src/js/requirements.js";

const full = requirementRows({
  engineLabel: "llama.cpp",
  devices: [{ id: "CUDA0", name: "NVIDIA GeForce RTX 3060" }],
  msvcRuntime: true,
  chatModel: "Qwythos-9B.gguf",
  embedModel: "nomic-embed.gguf",
});

assert.equal(full.length, 6, "six requirement rows");
assert.deepEqual(
  full.map((r) => r.key),
  ["engine", "webview2", "driver", "msvc", "chat_model", "embed_model"],
  "stable row order",
);
assert.equal(full[0].label, "llama.cpp + accelerator libraries");
assert.equal(full[0].bundled, true);
assert.equal(full[0].state, "included");
assert.equal(full[2].state, "detected");
assert.ok(full[2].detail.includes("RTX 3060"), "device name shown");
assert.equal(full[3].state, "detected");

const bare = requirementRows({
  engineLabel: "llama.cpp",
  devices: [],
  msvcRuntime: false,
  chatModel: "",
  embedModel: "",
});
assert.equal(bare[2].state, "missing");
assert.ok(bare[2].detail.includes("CPU"), "no-GPU detail mentions CPU");
assert.equal(bare[3].state, "missing");
assert.ok(bare[3].detail.includes("cannot start"));
assert.equal(bare[4].detail, "Not imported");
assert.ok(bare[5].detail.includes("semantic search"));

const html = requirementsTableHtml([
  {
    key: "engine",
    label: 'evil <script>alert("x")</script>',
    bundled: true,
    state: "included",
    detail: "Included",
  },
  {
    key: "driver",
    label: "GPU driver",
    bundled: false,
    state: "missing",
    detail: "Not detected",
  },
]);
assert.ok(
  html.includes('class="data-table requirements-table"'),
  "table class",
);
assert.ok(html.includes("req-missing"), "missing state class");
assert.ok(html.includes("req-included"), "included state class");
assert.ok(!html.includes("<script"), "no tag smuggling");
assert.ok(html.includes("&lt;script&gt;"), "label escaped");
assert.ok(html.includes(">Included<"), "bundled column rendered");
assert.ok(html.includes(">Not bundled<"), "external column rendered");
