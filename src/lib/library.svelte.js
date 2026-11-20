// Frontend side of the library: liked songs, playlists, local folders, scanning state.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { SvelteSet } from "svelte/reactivity";
import { toasts } from "./ui.svelte.js";

const FIELDS = ["id", "source", "url", "title", "artist", "album", "duration", "artwork", "preview"];

/** Strip UI-only fields (queue keys, item ids) before sending a track to the backend. */
export const plain = (t) => Object.fromEntries(FIELDS.map((f) => [f, t[f] ?? null]));

class Library {
  liked = new SvelteSet();
  playlists = $state([]);
  folders = $state([]);
  scan = $state({ state: "idle" });
  /** Bumped on every backend change, so views can refetch. */
  version = $state(0);

  constructor() {
    this.refresh();
    listen("library-changed", ({ payload }) => {
      if (payload === "likes") this.#loadLikes();
      if (payload === "playlists") this.#loadPlaylists();
      if (payload === "local") this.#loadFolders();
      this.version++;
    });
    listen("scan", ({ payload }) => {
      this.scan = payload;
      if (payload.state === "done" && payload.updated > 0) {
        toasts.show(`Local files updated: ${payload.total} songs`);
      }
    });
    invoke("lib_scanning").then((s) => s && (this.scan = { state: "started" }));
  }

  refresh() {
    this.#loadLikes();
    this.#loadPlaylists();
    this.#loadFolders();
  }

  async #loadLikes() {
    const urls = await invoke("lib_liked_urls").catch(() => []);
    this.liked.clear();
    for (const u of urls) this.liked.add(u);
  }
  async #loadPlaylists() {
    this.playlists = await invoke("lib_playlists").catch(() => []);
  }
  async #loadFolders() {
    this.folders = await invoke("lib_folders").catch(() => []);
  }

  isLiked(track) {
    return !!track && this.liked.has(track.url);
  }

  async toggleLike(track) {
    if (!track) return;
    const liked = !this.liked.has(track.url);
    // Update right away; the backend event confirms it.
    liked ? this.liked.add(track.url) : this.liked.delete(track.url);
    try {
      await invoke("lib_like", { track: plain(track), liked });
    } catch (e) {
      // Saving failed: put the heart back the way it was.
      liked ? this.liked.delete(track.url) : this.liked.add(track.url);
      toasts.show(String(e));
    }
  }

  async createPlaylist(name = "New playlist", tracks = []) {
    const id = await invoke("lib_playlist_create", { name });
    if (tracks.length) await invoke("lib_playlist_add", { id, tracks: tracks.map(plain) });
    return id;
  }

  async addToPlaylist(id, tracks) {
    const list = this.playlists.find((p) => p.id === id);
    const added = await invoke("lib_playlist_add", { id, tracks: tracks.map(plain) }).catch((e) => {
      toasts.show(String(e));
      return null;
    });
    if (added === null) return;
    const name = list?.name ?? "playlist";
    if (added === 0) toasts.show(`Already in ${name}`);
    else toasts.show(tracks.length === 1 ? `Added to ${name}` : `Added ${added} to ${name}`);
  }

  renamePlaylist(id, name) {
    return invoke("lib_playlist_rename", { id, name }).catch((e) => toasts.show(String(e)));
  }

  deletePlaylist(id) {
    return invoke("lib_playlist_delete", { id });
  }

  async addFolder() {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ directory: true, title: "Add a music folder" });
    if (path) await invoke("lib_add_folder", { path });
    this.#loadFolders();
  }

  async removeFolder(path) {
    await invoke("lib_remove_folder", { path });
    this.#loadFolders();
  }

  rescan() {
    return invoke("lib_rescan");
  }
}

export const library = new Library();
