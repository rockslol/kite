// Small pieces of shared UI state: navigation, the right-click menu and toasts.

class Nav {
  /** { name: "search" | "liked" | "history" | "local" | "playlist", id?: number } */
  view = $state(JSON.parse(localStorage.getItem("hum.view") || '{"name":"search"}'));

  go(view) {
    this.view = view;
    localStorage.setItem("hum.view", JSON.stringify(view));
  }

  is(name, id) {
    return this.view.name === name && (id === undefined || this.view.id === id);
  }
}

class Menu {
  /** { x, y, items: [{ label, action, danger? }] } or null */
  current = $state(null);

  open(e, items) {
    e?.preventDefault?.();
    this.current = { x: e.clientX, y: e.clientY, items };
  }

  /** Replace the open menu's items in place (for "Add to playlist…" style follow-ups). */
  swap(items) {
    if (this.current) this.current = { ...this.current, items };
  }

  close() {
    this.current = null;
  }
}

class Toasts {
  list = $state([]);
  #id = 0;

  show(text) {
    const id = ++this.#id;
    this.list = [...this.list, { id, text }];
    setTimeout(() => (this.list = this.list.filter((t) => t.id !== id)), 2600);
  }
}

export const nav = new Nav();
export const menu = new Menu();
export const toasts = new Toasts();
