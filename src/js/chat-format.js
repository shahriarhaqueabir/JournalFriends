import { escapeHtml } from "./sanitize.js";

const FENCE_RE = /```[\s\S]*?```/g;
const FENCE_TOK = /^[\uFFFF](\d+)[\uFFFF]$/;
const BULLET = /^[-*·]\s+(.+)$/;
const NUMBERED = /^\d+\.\s+(.+)$/;

export function formatAiText(text) {
    const escaped = escapeHtml(text);

    // 1. Extract fenced code blocks (backticks survive escaping).
    const cells = [];
    const withoutFences = escaped.replace(FENCE_RE, (m) => {
        const lang = /^```(\w*)/.exec(m.trim())?.[1] || "";
        const body = m.replace(/^```\w*[ \t]*\r?\n?/, "").replace(/```$/, "").replace(/[\r\n]+$/, "");
        const idx = cells.length;
        cells.push(
            `<div class="code-block"><span class="code-chip">${escapeHtml(lang || "</>")}</span><pre>${body}</pre></div>`
        );
        return `\uFFFF${idx}\uFFFF`;
    });

    // 2. Inline tokens on the escaped remainder (bold before italic).
    const inline = withoutFences
        .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
        .replace(/\*([^*\n]+)\*/g, "<em>$1</em>")
        .replace(/`([^`\n]+)`/g, '<code class="inline-code">$1</code>');

    // 3. Block structure: paragraphs, lists, fence tokens.
    const out = [];
    const lines = inline.split("\n");
    let i = 0;
    while (i < lines.length) {
        const line = lines[i];
        if (line.trim() === "") {
            i++;
            continue;
        }

        const fence = FENCE_TOK.exec(line.trim());
        if (fence) {
            out.push(cells[Number(fence[1])]);
            i++;
            continue;
        }

        const bullet = BULLET.exec(line.trim());
        if (bullet) {
            const items = [bullet[1]];
            while (i + 1 < lines.length) {
                const m = BULLET.exec(lines[i + 1].trim());
                if (!m) break;
                items.push(m[1]);
                i++;
            }
            out.push(`<ul>${items.map((it) => `<li>${it}</li>`).join("")}</ul>`);
            i++;
            continue;
        }

        const numbered = NUMBERED.exec(line.trim());
        if (numbered) {
            const items = [numbered[1]];
            while (i + 1 < lines.length) {
                const m = NUMBERED.exec(lines[i + 1].trim());
                if (!m) break;
                items.push(m[1]);
                i++;
            }
            out.push(`<ol>${items.map((it) => `<li>${it}</li>`).join("")}</ol>`);
            i++;
            continue;
        }

        const para = [line];
        while (i + 1 < lines.length) {
            const nxt = lines[i + 1];
            if (!nxt.trim() || BULLET.test(nxt.trim()) || NUMBERED.test(nxt.trim()) || FENCE_TOK.test(nxt.trim())) break;
            para.push(nxt);
            i++;
        }
        out.push(`<p>${para.join("<br>")}</p>`);
        i++;
    }

    return buildLinks(out.join(""));
}

function buildLinks(html) {
    // Titles containing escaped markup chars (&lt;, &gt;, "quot", '039) were attacker HTML
    // after escaping — never turn them into jump links; keep them as plain escaped text.
    const clean = (title) => !/&lt;|&gt;|&quot;|&#039;/.test(title);
    return html
        .replace(/\[Event: ([^\]]+)\]\(event:([a-zA-Z0-9-]+)\)/g, (m, title, id) =>
            clean(title)
                ? `<span class="ai-link event-link" data-id="${id}" title="Jump to event: ${title}">${title}</span>`
                : m)
        .replace(/\[Task: ([^\]]+)\]\(task:([a-zA-Z0-9-]+)\)/g, (m, title, id) =>
            clean(title)
                ? `<span class="ai-link task-link" data-id="${id}" title="Jump to task: ${title}">${title}</span>`
                : m)
        .replace(/\[Journal: ([^\]]+)\]\(journal:([a-zA-Z0-9-]+)\)/g, (m, title, id) =>
            clean(title)
                ? `<span class="ai-link journal-link" data-id="${id}" title="Jump to journal: ${title}">${title}</span>`
                : m);
}