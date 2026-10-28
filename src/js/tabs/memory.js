import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
import { renderPersonaForm, collectPersonaAnswers, savePersonaAnswers, hasPersonaAnswers } from "../persona.js";

const RELATIONS = ["related-to", "part-of", "contradicts", "cause-of", "reminds-of"];

let items = [];
let currentKind = "";
let currentCategory = "";
let searchTerm = "";
let detailId = null;

export async function initMemory() {
    const view = document.getElementById("memory-view");
    if (!view) return;

    document.getElementById("btn-new-memory-item").addEventListener("click", () => openForm(null));
    document.getElementById("memory-search").addEventListener("input", (e) => {
        searchTerm = e.target.value.trim().toLowerCase();
        refresh();
    });
    document.querySelectorAll("[data-kind]").forEach((btn) => {
        btn.addEventListener("click", () => {
            currentKind = btn.dataset.kind;
            document.querySelectorAll("[data-kind]").forEach((b) => b.classList.toggle("active", b === btn));
            refresh();
        });
    });
    document.querySelectorAll(".cat-btn").forEach((btn) => {
        btn.addEventListener("click", () => {
            currentCategory = btn.dataset.cat;
            document.querySelectorAll(".cat-btn").forEach((b) => b.classList.toggle("active", b === btn));
            refresh();
        });
    });
    document.getElementById("memory-grid").addEventListener("click", onGridClick);
    document.getElementById("memory-detail").addEventListener("click", onDetailClick);
    document.getElementById("memory-form").addEventListener("click", onFormClick);
    document.getElementById("btn-memory-form-close").addEventListener("click", () => closeForm());
    wirePersona();

    await refresh();
    await maybeShowPersonaCta();
}

async function maybeShowPersonaCta() {
    const cta = document.getElementById("persona-cta");
    if (!cta) return;
    let answered;
    try {
        answered = await hasPersonaAnswers();
    } catch {
        answered = true;
    }
    if (!answered) cta.classList.remove("hidden");
}

function wirePersona() {
    renderPersonaForm(document.getElementById("memory-persona-form"));

    document.getElementById("btn-persona-cta")?.addEventListener("click", () =>
        openPersonaLightbox());
    document.getElementById("btn-persona-cta-dismiss")?.addEventListener("click", () => {
        const cta = document.getElementById("persona-cta");
        if (cta) cta.classList.add("hidden");
    });
    document.getElementById("btn-persona-lightbox-close")?.addEventListener("click", closePersonaLightbox);
    document.getElementById("btn-persona-lightbox-cancel")?.addEventListener("click", closePersonaLightbox);
    document.getElementById("btn-persona-lightbox-save")?.addEventListener("click", async () => {
        await savePersonaAnswers(collectPersonaAnswers());
        const cta = document.getElementById("persona-cta");
        if (cta) cta.classList.add("hidden");
        closePersonaLightbox();
        await refresh();
    });
}

function openPersonaLightbox() {
    const overlay = document.getElementById("persona-lightbox");
    if (overlay) overlay.classList.remove("hidden");
}

function closePersonaLightbox() {
    const overlay = document.getElementById("persona-lightbox");
    if (overlay) overlay.classList.add("hidden");
}

async function refresh() {
    try {
        items = await invoke("memory_list", {
            kind: currentKind || null,
            category: currentCategory || null,
            search: searchTerm || null,
        });
    } catch {
        return;
    }
    renderGrid();
    const countEl = document.getElementById("memory-count");
    if (countEl) countEl.textContent = `${items.length} item${items.length === 1 ? "" : "s"}`;
}

function parseTags(raw) {
    try {
        const parsed = JSON.parse(raw || "[]");
        return Array.isArray(parsed) ? parsed.join(", ") : "";
    } catch {
        return "";
    }
}

function renderGrid() {
    const grid = document.getElementById("memory-grid");
    if (!items.length) {
        grid.innerHTML = `<div class="empty-state"><div class="empty-state-icon">🧠</div><p>No memory items yet.<br><small>Add one manually or let the AI propose items from your journal.</small></p></div>`;
        return;
    }
    grid.innerHTML = items.map((i) => `
        <article class="memory-card" data-id="${escapeHtml(i.id)}" tabindex="0">
            <div class="memory-card-header">
                <span class="memory-kind-badge kind-${escapeHtml(i.kind)}">${escapeHtml(i.kind)}</span>
                ${i.item_date ? `<span class="memory-card-date">${escapeHtml(i.item_date)}</span>` : ""}
            </div>
            <h4 class="memory-card-title">${escapeHtml(i.title)}</h4>
            <p class="memory-card-content">${escapeHtml(i.content)}</p>
            <div class="memory-card-footer">
                <span class="memory-cat-chip">${escapeHtml(i.category)}</span>
                ${i.tags && i.tags !== "[]" ? `<span class="memory-tags">${escapeHtml(parseTags(i.tags))}</span>` : ""}
                <span class="memory-conf">${Math.round((i.confidence || 1) * 100)}%</span>
            </div>
        </article>
    `).join("");
}

function onGridClick(e) {
    const card = e.target.closest(".memory-card");
    if (card) openDetail(card.dataset.id);
}

async function openDetail(id) {
    let detail;
    try {
        detail = await invoke("memory_get", { id });
    } catch {
        return;
    }
    detailId = id;
    const { item, links } = detail;
    const panel = document.getElementById("memory-detail-content");
    const linkRows = (links || []).map((l) => `
        <div class="memory-link-row" data-link-id="${escapeHtml(l.id)}">
            <span class="memory-link-rel">${l.direction === "out" ? "→" : "←"} ${escapeHtml(l.relation)}</span>
            <button class="memory-link-target" data-link-open="${escapeHtml(l.other_id)}">${escapeHtml(l.other_title)}</button>
            <button class="btn-icon memory-link-remove" data-link-remove="${escapeHtml(l.id)}" title="Remove link">×</button>
        </div>
    `).join("");

    panel.innerHTML = `
        <header class="panel-header">
            <h3>${escapeHtml(item.title)}</h3>
            <button id="btn-close-memory-detail" class="btn-icon">×</button>
        </header>
        <div class="panel-content">
            <div class="memory-detail-meta">
                <span class="memory-kind-badge kind-${escapeHtml(item.kind)}">${escapeHtml(item.kind)}</span>
                <span class="memory-cat-chip">${escapeHtml(item.category)}</span>
                <span class="memory-conf">${Math.round((item.confidence || 1) * 100)}% confident</span>
            </div>
            <p class="memory-detail-content">${escapeHtml(item.content)}</p>
            ${item.item_date ? `<div class="memory-detail-meta-line">Date: <strong>${escapeHtml(item.item_date)}</strong></div>` : ""}
            ${item.tags && item.tags !== "[]" ? `<div class="memory-detail-meta-line">Tags: ${escapeHtml(parseTags(item.tags))}</div>` : ""}
            <div class="memory-detail-meta-line">Provenance: <strong>${escapeHtml(item.provenance)}</strong>${item.created_by ? ` by <strong>${escapeHtml(item.created_by)}</strong>` : ""}</div>
            ${item.source_entry_id ? `<div class="memory-detail-meta-line">Source entry: <strong>${escapeHtml(item.source_entry_id)}</strong></div>` : ""}
            <div class="memory-detail-meta-line">Updated: <strong>${escapeHtml(new Date(item.updated_at).toLocaleString())}</strong></div>

            <div class="memory-connections">
                <h4>Connections</h4>
                ${linkRows.length ? linkRows : '<p class="memory-muted">No connections yet.</p>'}
                <div class="memory-link-add">
                    <select id="memory-link-relation">
                        ${RELATIONS.map((r) => `<option value="${r}">${r.replace("-", " ")}</option>`).join("")}
                    </select>
                    <input type="text" id="memory-link-search-input" placeholder="Search item to link..." />
                    <button id="btn-memory-link-search" class="btn-ghost-sm">Search</button>
                    <div id="memory-link-results" class="mini-search-results hidden"></div>
                </div>
            </div>
        </div>
        <footer class="panel-footer">
            <button id="btn-memory-edit" class="btn-ghost">Edit</button>
            <button id="btn-memory-delete" class="btn-danger">Delete</button>
        </footer>
    `;
    document.getElementById("memory-detail").classList.remove("hidden");
    document.getElementById("btn-memory-edit").addEventListener("click", () => startEdit(item));
    document.getElementById("btn-memory-delete").addEventListener("click", () => deleteItem(item.id));
}

function onDetailClick(e) {
    if (e.target.id === "btn-close-memory-detail") return closeDetail();
    const removeBtn = e.target.closest("[data-link-remove]");
    if (removeBtn) return removeLink(removeBtn.dataset.linkRemove);
    const openBtn = e.target.closest("[data-link-open]");
    if (openBtn) return openDetail(openBtn.dataset.linkOpen);
    if (e.target.id === "btn-memory-link-search") return doLinkSearch();
    const pick = e.target.closest("[data-link-pick]");
    if (pick) return addLink(pick.dataset.linkPick, pick.dataset.linkTitle);
}

async function doLinkSearch() {
    const input = document.getElementById("memory-link-search-input");
    const box = document.getElementById("memory-link-results");
    if (!input || !box) return;
    const term = input.value.trim();
    if (!term) return;
    let hits = [];
    try {
        hits = await invoke("memory_list", { kind: null, category: null, search: term });
    } catch {
        return;
    }
    hits = hits.filter((h) => h.id !== detailId);
    if (!hits.length) {
        box.innerHTML = '<div class="mini-search-empty">No matches.</div>';
    } else {
        box.innerHTML = hits.map((h) => `
            <button class="mini-search-result" data-link-pick="${escapeHtml(h.id)}" data-link-title="${escapeHtml(h.title)}">
                ${escapeHtml(h.title)} <span class="memory-cat-chip">${escapeHtml(h.category)}</span>
            </button>
        `).join("");
    }
    box.classList.remove("hidden");
}

async function addLink(targetId) {
    const relation = document.getElementById("memory-link-relation").value;
    try {
        await invoke("memory_link", { sourceId: detailId, targetId, relation, createdBy: "user" });
    } catch {
        return;
    }
    const box = document.getElementById("memory-link-results");
    if (box) box.classList.add("hidden");
    openDetail(detailId);
}

async function removeLink(linkId) {
    try {
        await invoke("memory_unlink", { linkId });
    } catch {
        return;
    }
    openDetail(detailId);
}

function startEdit(item) {
    openForm(item);
}

function openForm(item) {
    const overlay = document.getElementById("memory-form");
    document.getElementById("memory-form-title").textContent = item ? "Edit Memory Item" : "Add Memory Item";
    document.getElementById("memory-form-id").value = item ? item.id : "";
    document.getElementById("memory-form-kind").value = item ? item.kind : "information";
    document.getElementById("memory-form-title-input").value = item ? item.title : "";
    document.getElementById("memory-form-content").value = item ? item.content : "";
    document.getElementById("memory-form-category").value = item ? item.category : "preference";
    document.getElementById("memory-form-tags").value = item ? parseTags(item.tags) : "";
    document.getElementById("memory-form-item-date").value = item && item.item_date ? item.item_date : "";
    document.getElementById("memory-form-confidence").value = item ? String(item.confidence ?? 1) : "1";
    document.getElementById("btn-memory-form-delete").classList.toggle("hidden", !item);
    overlay.classList.remove("hidden");
}

async function onFormClick(e) {
    if (e.target.id === "btn-memory-form-save") return saveForm();
    if (e.target.id === "btn-memory-form-cancel") return closeForm();
    if (e.target.id === "btn-memory-form-delete") {
        const id = document.getElementById("memory-form-id").value;
        if (id) await deleteItem(id);
    }
}

async function saveForm() {
    const id = document.getElementById("memory-form-id").value;
    const title = document.getElementById("memory-form-title-input").value.trim();
    const content = document.getElementById("memory-form-content").value.trim();
    if (!title || !content) {
        alert("Title and content are required.");
        return;
    }
    const tags = document.getElementById("memory-form-tags").value
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);
    const rawDate = document.getElementById("memory-form-item-date").value;
    const confidence = Math.min(1, Math.max(0, Number(document.getElementById("memory-form-confidence").value) || 1));
    const payload = {
        id: id || "",
        title,
        content,
        kind: document.getElementById("memory-form-kind").value,
        category: document.getElementById("memory-form-category").value,
        tags: JSON.stringify(tags),
        item_date: rawDate || null,
        confidence,
    };
    try {
        if (id) {
            await invoke("memory_update", { input: payload });
        } else {
            await invoke("memory_create", { input: payload });
        }
    } catch {
        return;
    }
    closeForm();
    await refresh();
    if (id) {
        openDetail(id);
    }
}

async function deleteItem(id) {
    if (!confirm("Permanently delete this memory item and its connections?")) return;
    try {
        await invoke("memory_delete", { id });
    } catch {
        return;
    }
    closeDetail();
    closeForm();
    detailId = null;
    await refresh();
}

function closeDetail() {
    const panel = document.getElementById("memory-detail");
    panel.classList.add("hidden");
    document.getElementById("memory-detail-content").innerHTML = "";
    detailId = null;
}

function closeForm() {
    document.getElementById("memory-form").classList.add("hidden");
}
