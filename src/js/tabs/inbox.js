import { invoke } from "../ipc.js";
import { recordDiagnostic } from "../diagnostics.js";
import { escapeHtml } from "../sanitize.js";

let currentFilter = "pending";

export async function initInbox() {
    window.refreshInbox = refreshInbox;
    const container = document.getElementById("inbox-items-container");
    if (!container) return;

    // Filter switching
    document.querySelectorAll(".inbox-filter").forEach(btn => {
        btn.addEventListener("click", () => {
            document.querySelectorAll(".inbox-filter").forEach(b => b.classList.remove("active"));
            btn.classList.add("active");
            currentFilter = btn.dataset.filter;
            refreshInbox();
        });
    });

    // Clear processed
    document.getElementById("btn-clear-inbox")?.addEventListener("click", async () => {
        console.log("Clear processed not implemented yet");
    });

    refreshInbox();
}

export async function refreshInbox() {
    const container = document.getElementById("inbox-items-container");
    if (!container) return;

    try {
        const items = await invoke("proposed_list", { status: currentFilter, limit: 50 });
        renderInbox(items);
        updateBadge(items.length);
    } catch (e) {
        console.error("Failed to fetch inbox items", e);
        container.innerHTML = `<div class="error-state">Failed to load inbox: ${e}</div>`;
    }
}

function renderInbox(items) {
    const container = document.getElementById("inbox-items-container");
    if (!items || items.length === 0) {
        container.innerHTML = '<div class="list-empty">No items to review.</div>';
        return;
    }

    container.innerHTML = items.map(item => {
        const typeClass = `type-${item.item_type.split('_')[0]}`;
        const data = JSON.parse(item.proposed_data || '{}');

        let detailHtml = '';
        if (item.item_type === 'event') {
            detailHtml = `
                <div class="proposal-detail">
                    <span>📅 ${new Date(data.start_at).toLocaleString()}</span>
                    ${data.location ? `<span>📍 ${escapeHtml(data.location)}</span>` : ''}
                </div>
            `;
        } else if (item.item_type === 'note') {
            detailHtml = `
                <div class="proposal-detail note-content">
                    ${escapeHtml(data.content)}
                </div>
            `;
        }

        const sourceLinkHtml = item.source_entry_id ? `
            <button class="btn-ghost-sm btn-view-source" data-entry-id="${item.source_entry_id}">
                <hj-icon name="external-link" size="16" aria-hidden="true"></hj-icon>
                View Source Entry
            </button>
        ` : '';

        return `
            <div class="proposal-card" data-id="${escapeHtml(item.id)}">
                <div class="proposal-header">
                    <span class="proposal-type-badge ${escapeHtml(typeClass)}">${escapeHtml(item.item_type.replace('_', ' '))}</span>
                    <span class="entry-date">${escapeHtml(new Date(item.created_at).toLocaleString())}</span>
                </div>
                <div class="proposal-title">${escapeHtml(item.proposed_title)}</div>
                ${detailHtml}
                ${item.source_snippet ? `<div class="proposal-provenance">"...${escapeHtml(item.source_snippet)}..."</div>` : ''}
                <div class="proposal-actions">
                    ${sourceLinkHtml}
                    ${item.status === 'pending' ? `
                        <button class="btn-ghost btn-dismiss" data-id="${item.id}">Dismiss</button>
                        <button class="btn-primary btn-accept" data-id="${item.id}">Accept</button>
                    ` : `<span class="status-label">${item.status}</span>`}
                </div>
            </div>
        `;
    }).join('');

    // Wire up actions
    container.querySelectorAll(".btn-accept").forEach(btn => {
        btn.addEventListener("click", () => handleAction(btn.dataset.id, "accept"));
    });
    container.querySelectorAll(".btn-dismiss").forEach(btn => {
        btn.addEventListener("click", () => handleAction(btn.dataset.id, "dismiss"));
    });

    // Wire up source links
    container.querySelectorAll(".btn-view-source").forEach(btn => {
        btn.addEventListener("click", () => {
            const entryId = btn.dataset.entryId;
            // Switch to journal tab
            const journalTab = document.querySelector('[data-tab="journal"]');
            if (journalTab) journalTab.click();
            // Load entry
            if (window.loadJournalEntry) window.loadJournalEntry(entryId);
        });
    });

    if (window.lucide) {
        lucide.createIcons();
    }
}

async function handleAction(id, action) {
    try {
        recordDiagnostic("inbox:action", { id, action });
        await invoke("proposed_action", { id, action });
        refreshInbox();
    } catch (e) {
        console.error(`Failed to ${action} proposal`, e);
        alert(`Error: ${e}`);
    }
}

function updateBadge(count) {
    const badge = document.getElementById("inbox-badge");
    if (!badge) return;

    if (currentFilter === "pending" && count > 0) {
        badge.textContent = count;
        badge.className = "badge-count";
    } else {
        badge.className = "hidden";
    }
}
