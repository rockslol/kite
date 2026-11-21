// Lyrics for the current song: fetched when the lyrics panel is open, parsed from LRC, and the
// current line worked out from the playback position (plus the song's timing nudge).

import { invoke } from "@tauri-apps/api/core";
import { player } from "./player.svelte.js";
import { toasts } from "./ui.svelte.js";

const FIELDS = ["id", "source", "url", "title", "artist", "album", "duration", "artwork", "preview", "artist_id", "album_id"];

/** Parses LRC text into `[{ t, text }]` sorted by time, plus the file's own `[offset:]` in ms. */
export function parseLrc(text) {
  const lines = [];
  let fileOffset = 0;
  for (const raw of text.split(/\r?\n/)) {
    const off = raw.match(/^\s*\[offset:\s*([+-]?\d+)\s*\]/i);
    if (off) {
      fileOffset = Number(off[1]);
      continue;
    }
    // A line can carry several timestamps: "[00:12.00][01:30.50]Chorus".
    const stamps = [...raw.matchAll(/\[(\d+):(\d+(?:[.:]\d+)?)\]/g)];
    if (!stamps.length) continue;
    // Word timings ("<00:12.30>word") are dropped for now: line by line only.
    const words = raw.slice(stamps.at(-1).index + stamps.at(-1)[0].length).replace(/<\d+:\d+(?:[.:]\d+)?>/g, "").trim();
    for (const s of stamps) lines.push({ t: Number(s[1]) * 60 + Number(s[2].replace(":", ".")), text: words });
  }
  lines.sort((a, b) => a.t - b.t);
  return { lines, fileOffset };
}

class Lyrics {
  /** "idle" | "loading" | "synced" | "plain" | "instrumental" | "none" | "error" */
  state = $state("idle");
  lines = $state([]);
  plain = $state("");
  source = $state("");
  error = $state("");
  /** The user's nudge for this song, ms. Positive = lyrics come earlier. */
  offset = $state(0);
  #fileOffset = 0;
  #url = null;
  #token = 0;

  /** Index of the line being sung now (-1 before the first). */
  current = $derived.by(() => {
    if (this.state !== "synced") return -1;
    const at = player.time + (this.offset + this.#fileOffset) / 1000;
    let lo = 0;
    let hi = this.lines.length - 1;
    let found = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (this.lines[mid].t <= at) {
        found = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return found;
  });

  /** Loads lyrics for `track` unless they're already loaded. */
  async load(track) {
    if (!track) {
      this.#url = null;
      this.state = "idle";
      return;
    }
    if (track.url === this.#url && this.state !== "error") return;
    this.#url = track.url;
    const token = ++this.#token;
    this.state = "loading";
    this.lines = [];
    this.plain = "";
    try {
      const l = await invoke("lyrics", { track: Object.fromEntries(FIELDS.map((f) => [f, track[f] ?? null])) });
      if (token !== this.#token) return;
      this.offset = l.offset_ms;
      this.source = l.source;
      if (l.synced) {
        const { lines, fileOffset } = parseLrc(l.synced);
        this.lines = lines;
        this.#fileOffset = fileOffset;
        this.state = lines.length ? "synced" : "none";
      } else if (l.plain) {
        this.plain = l.plain;
        this.state = "plain";
      } else {
        this.state = l.instrumental ? "instrumental" : "none";
      }
    } catch (e) {
      if (token !== this.#token) return;
      this.error = String(e);
      this.state = "error";
    }
  }

  /** Jump the song to a line. */
  seekTo(i) {
    const line = this.lines[i];
    if (line) player.seek(Math.max(0, line.t - (this.offset + this.#fileOffset) / 1000));
  }

  /** Move the lyrics earlier (+) or later (-) and remember it for this song. */
  nudge(ms) {
    if (!this.#url) return;
    this.offset = Math.max(-30000, Math.min(30000, this.offset + ms));
    invoke("lyrics_offset", { url: this.#url, offsetMs: this.offset }).catch((e) => toasts.show(String(e)));
  }
}

export const lyrics = new Lyrics();
