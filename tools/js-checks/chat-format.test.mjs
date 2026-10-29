import assert from "node:assert/strict";
import { formatAiText } from "../../src/js/chat-format.js";

const has = (html, sub) =>
  assert.ok(html.includes(sub), `expected ${JSON.stringify(html)} to contain ${JSON.stringify(sub)}`);
const notHas = (html, sub) =>
  assert.ok(!html.includes(sub), `expected ${JSON.stringify(html)} NOT to contain ${JSON.stringify(sub)}`);

// inline tokens
const inlineSample = formatAiText("see **bold**, *it*, and `code`");
has(inlineSample, "<strong>bold</strong>");
has(inlineSample, "<em>it</em>");
has(inlineSample, '<code class="inline-code">code</code>');

// paragraphs vs single breaks
has(formatAiText("one\ntwo"), "<p>one<br>two</p>");
has(formatAiText("a\n\nb"), "<p>a</p><p>b</p>");

// lists
has(formatAiText("- a\n- b"), "<ul><li>a</li><li>b</li></ul>");
has(formatAiText("1. a\n2. b"), "<ol><li>a</li><li>b</li></ol>");
has(formatAiText("- a\n- b\n\nnext"), "<p>next</p>");

// fenced blocks: inner stays escaped, no inline parsing inside
const fenced = formatAiText("```js\nconst x = 1 < 2 > 0;\n**not bold**\n```");
has(fenced, "<pre>const x = 1 &lt; 2 &gt; 0;\n**not bold**</pre>");
notHas(fenced, "<strong>");

// jump links
has(formatAiText("[Event: Standup](event:abc-123)"), "ai-link event-link");
has(formatAiText("[Task: Water](task:t1)"), "ai-link task-link");
has(formatAiText("[Journal: Day 1](journal:j9)"), "ai-link journal-link");

// XSS inertness — escape-first: raw tags never survive, entities are preserved as inert text.
const xss = formatAiText('<script>alert(1)</script> and [Event: <img src=x onerror=alert(1)>](event:bad)');
notHas(xss, "<script>");
has(xss, "&lt;script&gt;");
notHas(xss, "<img");
has(xss, "&lt;img");
// markup-tainted link titles never become jump links — they render as plain (escaped) text.
notHas(xss, "ai-link");
notHas(xss, "data-id=\"bad\"");

console.log("chat-format: all assertions passed");