// Opening artist and album pages from any song. Songs from YouTube Music carry the page ids;
// others (library, local files) are looked up by name.

import { invoke } from "@tauri-apps/api/core";
import { nav, toasts } from "./ui.svelte.js";

/** Whether a song can have YouTube Music pages (SoundCloud uploaders aren't YouTube artists). */
export const hasPages = (t) => !!t && t.source !== "soundcloud" && !!t.artist;

export async function openArtist(t, id = t?.artist_id) {
  try {
    id ??= await invoke("ytm_find_artist", { name: t.artist });
    nav.go({ name: "artist", id });
  } catch (e) {
    toasts.show(String(e));
  }
}

export async function openAlbum(t) {
  try {
    const id = t.album_id ?? (await invoke("ytm_find_album", { artist: t.artist, album: t.album }));
    nav.go({ name: "album", id });
  } catch (e) {
    toasts.show(String(e));
  }
}
