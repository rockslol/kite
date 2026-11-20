// Search state lives here so it survives switching views.

import { invoke } from "@tauri-apps/api/core";
import { player } from "./player.svelte.js";
import { importer, isMusicLink } from "./importer.svelte.js";

class Search {
  source = $state(localStorage.getItem("hum.source") || "ytmusic");
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
    if (isMusicLink(q)) return this.#openLink(q);
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

  /** A pasted link: play a song, import a playlist. */
  async #openLink(url) {
    const token = ++this.#token;
    this.searching = true;
    this.error = "";
    this.libraryHits = [];
    try {
      const r = await invoke("open_link", { url });
      if (token !== this.#token) return;
      if (r.kind === "list") {
        this.query = "";
        importer.importLink(url);
      } else {
        this.results = [r.track];
        this.searchedFor = url;
        player.playList(this.results, 0);
      }
    } catch (e) {
      if (token === this.#token) this.error = String(e);
    } finally {
      if (token === this.#token) this.searching = false;
    }
  }

  pickSource(id) {
    if (this.source === id) return;
    this.source = id;
    localStorage.setItem("hum.source", id);
    if (this.query.trim()) this.run();
  }
}

export const search = new Search();
