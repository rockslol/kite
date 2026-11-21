// Small pieces of shared UI state: navigation, the right-click menu and toasts.

class Nav {
  /** { name: "home" | "search" | "liked" | "history" | "local" | "playlist" | "artist" | "album", id? } */
  view = $state(JSON.parse(localStorage.getItem("kite.view") || '{"name":"home"}'));

  /** Earlier pages, for Back. */
  history = $state([]);

  go(view) {
    const same = view.name === this.view.name && view.id === this.view.id;
    if (!same) this.history = [...this.history.slice(-49), this.view];
    this.#show(view);
  }

  back() {
    const prev = this.history.at(-1);
    if (!prev) return;
    this.history = this.history.slice(0, -1);
    this.#show(prev);
  }

  #show(view) {
    this.view = view;
    localStorage.setItem("kite.view", JSON.stringify(view));
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

  /** `action` ({ label, run }) adds a button, e.g. Undo; those toasts stay a little longer. */
  show(text, action = null) {
    const id = ++this.#id;
    this.list = [...this.list, { id, text, action }];
    setTimeout(() => this.dismiss(id), action ? 6000 : 2600);
  }

  dismiss(id) {
    this.list = this.list.filter((t) => t.id !== id);
  }

  /** A toast with an Undo button. */
  undo(text, run) {
    this.show(text, { label: "Undo", run });
  }
}

export const nav = new Nav();
export const menu = new Menu();
export const toasts = new Toasts();
