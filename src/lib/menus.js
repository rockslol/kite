// Right-click menus for songs, shared by every list.

import { player } from "./player.svelte.js";
import { library } from "./library.svelte.js";
import { menu, nav, songInfo } from "./ui.svelte.js";
import { hasPages, openArtist, openAlbum } from "./discover.js";

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
    ...(t.source === "youtube" ? [{ label: "Start radio", action: () => player.startRadio({ track: t, title: t.title }) }] : []),
    ...(hasPages(t) ? [{ label: "Go to artist", action: () => openArtist(t) }] : []),
    ...(hasPages(t) && t.album ? [{ label: "Go to album", action: () => openAlbum(t) }] : []),
    { label: "Song info", action: () => songInfo.open(t) },
    ...extra,
  ]);
}

/** The menu for several selected songs. */
export function songsMenu(e, tracks, extra = []) {
  const allLiked = tracks.every((t) => library.isLiked(t));
  menu.open(e, [
    { label: `Play ${tracks.length} songs`, action: () => player.playList(tracks, 0) },
    { label: "Play next", action: () => player.playNext(tracks) },
    { label: "Add to queue", action: () => player.addToQueue(tracks) },
    { label: allLiked ? "Remove from liked songs" : "Add to liked songs", action: () => library.setLiked(tracks, !allLiked) },
    { label: "Add to playlist…", keepOpen: true, action: () => playlistPicker(tracks) },
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
