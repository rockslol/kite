// Playlist imports: progress, results, and syncing imported playlists.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { nav, toasts } from "./ui.svelte.js";
import { plural } from "./urls.js";

let seq = 0;

class Importer {
  /** Running jobs: { job, label, state: "reading" | "matching", done, total } */
  jobs = $state([]);
  /** Last import result per playlist id, for the "not found" list. */
  results = $state({});
  dialogOpen = $state(false);

  constructor() {
    listen("import", ({ payload: p }) => {
      const job = this.jobs.find((j) => j.job === p.job);
      if (!job) return;
      if (p.state === "matching") Object.assign(job, { state: "matching", done: p.done, total: p.total });
      else if (p.state === "reading") job.state = "reading";
    });
  }

  get busy() {
    return this.jobs.length > 0;
  }

  /** Import a playlist link into a new playlist. Resolves to the result, or null on failure. */
  async importLink(url) {
    return this.#run(url, null, "Reading the playlist…");
  }

  /** Add songs that are new at the source of an imported playlist. */
  async sync(playlistId, name) {
    const r = await this.#run("", playlistId, `Syncing ${name}…`);
    if (r) toasts.show(r.added ? `Added ${plural(r.added, "new song")}` : `${name} is up to date`);
    return r;
  }

  async #run(url, playlistId, label) {
    const job = `job${++seq}`;
    this.jobs = [...this.jobs, { job, label, state: "reading", done: 0, total: 0 }];
    try {
      const r = await invoke("import_link", { url, job, playlistId });
      this.results = { ...this.results, [r.playlist_id]: r };
      if (playlistId === null) {
        const found = r.total - r.missing.length;
        toasts.show(
          r.missing.length ? `Imported ${found} of ${r.total} songs into ${r.name}` : `Imported ${plural(found, "song")} into ${r.name}`
        );
        nav.go({ name: "playlist", id: r.playlist_id });
      }
      return r;
    } catch (e) {
      toasts.show(String(e));
      return null;
    } finally {
      this.jobs = this.jobs.filter((j) => j.job !== job);
    }
  }
}

export const importer = new Importer();

/** True for links that look like music links we can open. */
export function isMusicLink(text) {
  return /^(https?:\/\/)?([\w-]+\.)*(spotify\.com|spotify\.link|deezer\.com|deezer\.page\.link|youtube\.com|youtu\.be|soundcloud\.com)\//i.test(
    text.trim()
  );
}
