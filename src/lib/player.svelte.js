// Playback: queue, shuffle/repeat, session restore, system media controls.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { streamUrl } from "./urls.js";

const SAVE_KEY = "kite.session";
const POS_KEY = "kite.position";
const TRACK_FIELDS = ["id", "source", "url", "title", "artist", "album", "duration", "artwork", "preview"];

let seq = 0;
const newKey = () => `${Date.now().toString(36)}${(seq++).toString(36)}`;
const toItem = (t) => {
  const item = { key: newKey() };
  for (const f of TRACK_FIELDS) item[f] = t[f] ?? null;
  return item;
};

function shuffled(list) {
  const a = list.slice();
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

class Player {
  queue = $state([]);
  index = $state(-1);
  playing = $state(false);
  loading = $state(false);
  time = $state(0);
  duration = $state(0);
  volume = $state(0.8);
  shuffle = $state(false);
  repeat = $state("off"); // "off" | "all" | "one"
  error = $state("");

  current = $derived(this.queue[this.index]);
  upcoming = $derived(this.queue.slice(this.index + 1));

  #audio = new Audio();
  #token = 0;
  #dragging = false;
  #resumeAt = 0;
  #original = null; // queue order before shuffling, as keys
  #counted = false;

  constructor() {
    this.#audio.preload = "auto";
    this.#restore();
    this.#wireAudio();
    this.#wireMediaControls();

    $effect.root(() => {
      $effect(() => {
        this.#audio.volume = this.volume;
      });
      $effect(() => this.#save());
    });
    addEventListener("beforeunload", () => this.#savePosition());
  }

  // ---- queue -------------------------------------------------------------

  /** Replace the queue with `tracks` and play `tracks[i]`. */
  playList(tracks, i) {
    let items = tracks.map(toItem);
    const chosen = items[i];
    this.#original = items.map((t) => t.key);
    if (this.shuffle) items = [chosen, ...shuffled(items.filter((t) => t !== chosen))];
    this.queue = items;
    this.#playAt(items.indexOf(chosen));
  }

  playNext(track) {
    if (this.index < 0) return this.playList([track], 0);
    const item = toItem(track);
    this.queue.splice(this.index + 1, 0, item);
    this.#original?.push(item.key);
  }

  addToQueue(track) {
    if (this.index < 0) return this.playList([track], 0);
    const item = toItem(track);
    this.queue.push(item);
    this.#original?.push(item.key);
  }

  /** Play `tracks` in random order. */
  playShuffled(tracks) {
    if (!tracks.length) return;
    this.shuffle = true;
    this.playList(tracks, Math.floor(Math.random() * tracks.length));
  }

  /** Resolve a track in the background so it starts faster if it's played. */
  warm(track) {
    if (track?.url && track.source !== "local") invoke("prepare", { url: track.url }).catch(() => {});
  }

  jump(key) {
    const i = this.queue.findIndex((t) => t.key === key);
    if (i >= 0) this.#playAt(i);
  }

  remove(key) {
    const i = this.queue.findIndex((t) => t.key === key);
    if (i < 0) return;
    if (this.#original) this.#original = this.#original.filter((k) => k !== key);
    if (i === this.index) {
      this.queue.splice(i, 1);
      if (i < this.queue.length) this.#playAt(i);
      else this.#stop();
      return;
    }
    this.queue.splice(i, 1);
    if (i < this.index) this.index--;
  }

  /** Move an upcoming item to position `to` within the upcoming list. */
  moveUpcoming(key, to) {
    const from = this.queue.findIndex((t) => t.key === key);
    if (from <= this.index) return;
    const [item] = this.queue.splice(from, 1);
    const target = Math.max(this.index + 1, Math.min(this.queue.length, this.index + 1 + to));
    this.queue.splice(target, 0, item);
  }

  clearUpcoming() {
    const keep = new Set(this.queue.slice(0, this.index + 1).map((t) => t.key));
    this.queue = this.queue.slice(0, this.index + 1);
    if (this.#original) this.#original = this.#original.filter((k) => keep.has(k));
  }

  toggleShuffle() {
    this.shuffle = !this.shuffle;
    const cur = this.current;
    if (this.shuffle) {
      this.#original = this.queue.map((t) => t.key);
      const rest = shuffled(this.queue.filter((t) => t !== cur));
      this.queue = cur ? [cur, ...rest] : rest;
      this.index = cur ? 0 : -1;
    } else if (this.#original) {
      const byKey = new Map(this.queue.map((t) => [t.key, t]));
      const restored = this.#original.map((k) => byKey.get(k)).filter(Boolean);
      for (const t of this.queue) if (!restored.includes(t)) restored.push(t);
      this.queue = restored;
      this.index = cur ? restored.indexOf(cur) : -1;
    }
  }

  cycleRepeat() {
    this.repeat = { off: "all", all: "one", one: "off" }[this.repeat];
  }

  // ---- transport ---------------------------------------------------------

  toggle() {
    if (!this.current) return;
    if (this.error || !this.#audio.src) return this.#playAt(this.index, this.#resumeAt);
    this.#audio.paused ? this.#audio.play() : this.#audio.pause();
  }

  next(auto = false) {
    if (auto && this.repeat === "one") {
      this.#audio.currentTime = 0;
      this.#audio.play();
    } else if (this.index + 1 < this.queue.length) {
      this.#playAt(this.index + 1);
    } else if (this.repeat === "all" && this.queue.length) {
      this.#playAt(0);
    } else if (auto) {
      this.playing = false;
      this.#media("paused");
    }
  }

  prev() {
    if (this.#audio.currentTime > 3 || this.index <= 0) this.seek(0);
    else this.#playAt(this.index - 1);
  }

  /** While the user drags the seek bar: move the thumb without seeking. */
  scrub(t) {
    this.#dragging = true;
    this.time = t;
  }

  seek(t) {
    this.#dragging = false;
    t = Math.max(0, Math.min(t, this.duration || t));
    this.time = t;
    if (this.#audio.src) this.#audio.currentTime = t;
    else this.#resumeAt = t;
  }

  seekBy(delta) {
    this.seek(this.#audio.currentTime + delta);
  }

  // ---- internals ---------------------------------------------------------

  async #playAt(i, startAt = 0) {
    const track = this.queue[i];
    if (!track) return;
    this.index = i;
    const token = ++this.#token;
    const audio = this.#audio;
    audio.pause();
    audio.removeAttribute("src");
    audio.load();
    this.error = "";
    this.loading = true;
    this.time = startAt;
    this.duration = track.duration || 0;
    this.#resumeAt = 0;
    this.#counted = false;
    this.#sendTrack(track);

    try {
      if (track.source !== "local") await invoke("prepare", { url: track.url });
    } catch (e) {
      if (token === this.#token) {
        this.loading = false;
        this.error = String(e);
      }
      return;
    }
    if (token !== this.#token) return;

    audio.src = streamUrl(track);
    if (startAt > 0) audio.currentTime = startAt;
    audio.play().catch((e) => {
      if (e.name !== "AbortError" && token === this.#token) {
        this.loading = false;
        this.error = "This song couldn't be played.";
      }
    });

    this.warm(this.queue[i + 1]);
  }

  #stop() {
    this.#token++;
    this.#audio.pause();
    this.#audio.removeAttribute("src");
    this.index = this.queue.length ? Math.min(this.index, this.queue.length - 1) : -1;
    this.playing = false;
    this.loading = false;
    this.time = 0;
    this.#media("stopped");
  }

  #wireAudio() {
    const a = this.#audio;
    a.addEventListener("playing", () => {
      this.playing = true;
      this.loading = false;
      this.#media("playing");
    });
    a.addEventListener("pause", () => {
      this.playing = false;
      this.#media("paused");
      this.#savePosition();
    });
    a.addEventListener("waiting", () => (this.loading = true));
    a.addEventListener("canplay", () => (this.loading = false));
    a.addEventListener("seeked", () => this.#media(a.paused ? "paused" : "playing"));
    let lastSave = 0;
    a.addEventListener("timeupdate", () => {
      if (!this.#dragging) this.time = a.currentTime;
      // Counts as a play after 30 s, or halfway through shorter songs.
      if (!this.#counted && this.current && a.currentTime >= Math.min(30, (a.duration || 60) / 2)) {
        this.#counted = true;
        const t = this.current;
        const track = Object.fromEntries(TRACK_FIELDS.map((f) => [f, t[f] ?? null]));
        invoke("lib_played", { track }).catch(() => {});
      }
      if (Date.now() - lastSave > 5000) {
        lastSave = Date.now();
        this.#savePosition();
        if (!a.paused) this.#media("playing");
      }
    });
    a.addEventListener("durationchange", () => {
      if (Number.isFinite(a.duration) && a.duration > 0) {
        this.duration = a.duration;
        if (this.current) this.#sendTrack(this.current, a.duration);
      }
    });
    a.addEventListener("ended", () => this.next(true));
    a.addEventListener("error", () => {
      if (!a.getAttribute("src") || !this.current) return;
      this.loading = false;
      this.playing = false;
      this.error = "This song couldn't be played.";
    });
  }

  #wireMediaControls() {
    listen("media-control", ({ payload: p }) => {
      switch (p.action) {
        case "play":
          if (!this.playing) this.toggle();
          break;
        case "pause":
          if (this.playing) this.toggle();
          break;
        case "toggle":
          this.toggle();
          break;
        case "next":
          this.next();
          break;
        case "previous":
          this.prev();
          break;
        case "seek":
          this.seek(p.position);
          break;
        case "seekBy":
          this.seekBy(p.delta);
          break;
      }
    });
  }

  #sendTrack(t, duration = t.duration) {
    invoke("tray_tooltip", { text: t.artist ? `${t.title} · ${t.artist}` : t.title }).catch(() => {});
    invoke("media_track", {
      title: t.title,
      artist: t.artist || "",
      artwork: t.artwork,
      duration: duration || null,
    }).catch(() => {});
  }

  #media(state) {
    invoke("media_state", { state, position: this.#audio.currentTime || this.time || 0 }).catch(() => {});
  }

  #save() {
    const data = {
      queue: this.queue.slice(0, 1000).map((t) => ({ ...t })),
      index: this.index,
      volume: this.volume,
      shuffle: this.shuffle,
      repeat: this.repeat,
      original: this.#original,
    };
    localStorage.setItem(SAVE_KEY, JSON.stringify(data));
  }

  #savePosition() {
    localStorage.setItem(POS_KEY, String(this.#audio.currentTime || this.time || 0));
  }

  #restore() {
    try {
      const s = JSON.parse(localStorage.getItem(SAVE_KEY) || "null");
      if (!s) return;
      this.queue = Array.isArray(s.queue) ? s.queue : [];
      this.index = Number.isInteger(s.index) && s.index < this.queue.length ? s.index : -1;
      this.volume = typeof s.volume === "number" ? s.volume : 0.8;
      this.shuffle = !!s.shuffle;
      this.repeat = ["off", "all", "one"].includes(s.repeat) ? s.repeat : "off";
      this.#original = Array.isArray(s.original) ? s.original : null;
      if (this.current) {
        this.#resumeAt = Number(localStorage.getItem(POS_KEY)) || 0;
        this.time = this.#resumeAt;
        this.duration = this.current.duration || 0;
        this.#sendTrack(this.current);
      }
    } catch {
      localStorage.removeItem(SAVE_KEY);
    }
  }
}

export const player = new Player();
