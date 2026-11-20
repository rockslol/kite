// Search state lives here so it survives switching views.

import { invoke } from "@tauri-apps/api/core";
import { player } from "./player.svelte.js";

class Search {
  source = $state(localStorage.getItem("source") || "youtube");
  query = $state("");
  results = $state([]);
  libraryHits = $state([]);
  searching = $state(false);
  error = $state("");
  searchedFor = $state("");
  #token = 0;

  async run() {
    const q = this.query.trim();
    if (!q) return;
    const token = ++this.#token;
    this.searching = true;
    this.error = "";
    invoke("lib_search", { query: q })
      .then((hits) => token === this.#token && (this.libraryHits = hits))
      .catch(() => {});
    try {
      const found = await invoke("search", { source: this.source, query: q });
      if (token !== this.#token) return;
      this.results = found;
      this.searchedFor = q;
      player.warm(found[0]);
    } catch (e) {
      if (token === this.#token) this.error = String(e);
    } finally {
      if (token === this.#token) this.searching = false;
    }
  }

  pickSource(id) {
    if (this.source === id) return;
    this.source = id;
    localStorage.setItem("source", id);
    if (this.query.trim()) this.run();
  }
}

export const search = new Search();
