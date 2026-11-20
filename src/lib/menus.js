// Right-click menus for songs, shared by every list.

import { player } from "./player.svelte.js";
import { library } from "./library.svelte.js";
import { menu, nav } from "./ui.svelte.js";

/**
 * @param list   the list the song is in (for "Play", which plays the list from that song)
 * @param index  the song's position in `list`
 * @param extra  more items, e.g. "Remove from this playlist"
 */
export function songMenu(e, list, index, extra = []) {
  const t = list[index];
  menu.open(e, [
    { label: "Play", action: () => player.playList(list, index) },
    { label: "Play next", action: () => player.playNext(t) },
    { label: "Add to queue", action: () => player.addToQueue(t) },
    { label: library.isLiked(t) ? "Remove from liked songs" : "Add to liked songs", action: () => library.toggleLike(t) },
    { label: "Add to playlist…", keepOpen: true, action: () => playlistPicker([t]) },
    ...extra,
  ]);
}

/** Replaces the open menu with a list of playlists to add `tracks` to. */
export function playlistPicker(tracks) {
  menu.swap([
    {
      label: "New playlist",
      action: async () => {
        const id = await library.createPlaylist(tracks.length === 1 ? tracks[0].title : "New playlist", tracks);
        nav.go({ name: "playlist", id, rename: true });
      },
    },
    ...library.playlists.map((p) => ({ label: p.name, action: () => library.addToPlaylist(p.id, tracks) })),
  ]);
}
