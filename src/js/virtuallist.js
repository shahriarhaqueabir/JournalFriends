/**
 * Simple VirtualList implementation for Arnold.
 * Recycles DOM nodes to handle thousands of entries smoothly.
 */
export class VirtualList {
  constructor(container, options) {
    this.container = container;
    this.rowHeight = options.rowHeight || 80;
    this.renderItem = options.renderItem;
    this.onLoadMore = options.onLoadMore;

    this.data = [];
    this.visibleNodes = new Map(); // id -> node
    this.pool = [];

    // Save existing empty state or create default
    const existingEmpty = container.querySelector(
      ".empty-state, .entry-list-empty",
    );
    if (existingEmpty) {
      this.emptyStateEl = existingEmpty.cloneNode(true);
    } else {
      this.emptyStateEl = document.createElement("div");
      this.emptyStateEl.className = "entry-list-empty";
      this.emptyStateEl.textContent = "No entries yet.";
    }

    // Clear container html to remove static layout flow elements
    this.container.innerHTML = "";
    this.container.style.position = "relative";
    this.container.style.overflowY = "auto";

    this.scroller = document.createElement("div");
    this.scroller.style.width = "100%";
    this.scroller.style.position = "absolute";
    this.scroller.style.top = "0";
    this.scroller.style.left = "0";
    this.scroller.style.pointerEvents = "none";
    this.container.appendChild(this.scroller);

    this.emptyStateEl.style.display = "none";
    this.container.appendChild(this.emptyStateEl);

    this.container.addEventListener("scroll", () => this.render());

    // Resize observer to re-render when container size changes
    if (typeof ResizeObserver !== "undefined") {
      this.resizeObserver = new ResizeObserver(() => this.render());
      this.resizeObserver.observe(this.container);
    }
  }

  setData(newData) {
    this.data = newData;
    if (this.emptyStateEl) {
      this.emptyStateEl.style.display = newData.length === 0 ? "block" : "none";
    }
    this.updateHeight();
    this.render();
  }

  appendData(newData) {
    this.data = this.data.concat(newData);
    if (this.emptyStateEl) {
      this.emptyStateEl.style.display =
        this.data.length === 0 ? "block" : "none";
    }
    this.updateHeight();
    this.render();
  }

  updateHeight() {
    this.scroller.style.height = `${this.data.length * this.rowHeight}px`;
  }

  render() {
    if (this.data.length === 0) {
      for (const [index, node] of this.visibleNodes) {
        node.style.display = "none";
        this.pool.push(node);
        this.visibleNodes.delete(index);
      }
      return;
    }

    const scrollTop = this.container.scrollTop;
    const containerHeight = this.container.clientHeight;

    const startIndex = Math.floor(scrollTop / this.rowHeight);
    const endIndex = Math.min(
      this.data.length - 1,
      Math.ceil((scrollTop + containerHeight) / this.rowHeight) + 1,
    );

    // Threshold for loading more
    if (this.onLoadMore && endIndex > this.data.length - 5) {
      this.onLoadMore();
    }

    const visibleIndices = new Set();
    for (let i = startIndex; i <= endIndex; i++) {
      visibleIndices.add(i);
    }

    // Remove out-of-view nodes
    for (const [index, node] of this.visibleNodes) {
      if (!visibleIndices.has(index)) {
        node.style.display = "none";
        this.pool.push(node);
        this.visibleNodes.delete(index);
      }
    }

    // Add/Update in-view nodes
    for (let i = startIndex; i <= endIndex; i++) {
      let node = this.visibleNodes.get(i);
      if (!node) {
        node = this.pool.pop() || this.createNode();
        node.style.display = "block";
        this.visibleNodes.set(i, node);
        this.container.appendChild(node);
      }

      node.style.transform = `translateY(${i * this.rowHeight}px)`;
      this.renderItem(node, this.data[i], i);
    }
  }

  createNode() {
    const node = document.createElement("div");
    node.style.position = "absolute";
    node.style.top = "0";
    node.style.left = "0";
    node.style.width = "100%";
    node.style.height = `${this.rowHeight}px`;
    return node;
  }

  destroy() {
    if (this.resizeObserver) {
      this.resizeObserver.disconnect();
    }
  }
}
