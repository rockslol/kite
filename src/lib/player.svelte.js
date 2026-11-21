// Playback: queue, shuffle/repeat, session restore, system media controls.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { streamUrl } from "./urls.js";
import { toasts } from "./ui.svelte.js";

const SAVE_KEY = "kite.session";
const POS_KEY = "kite.position";
const TRACK_FIELDS = ["id", "source", "url", "title", "artist", "album", "duration", "artwork", "preview"];
/** Start loading the next song this many seconds before the current one ends. */
const PRELOAD_BEFORE_END = 25;
/** Paused longer than this, a stream song restarts at the same spot instead of resuming the old
 *  connection (its link may have expired, or the PC slept in between). */
const LONG_PAUSE = 20 * 60 * 1000;
/** How many songs at the top of an opened list are prepared in the background. Kept small:
 *  dozens of lookups in a row make YouTube hand out links that never work. */
const WARM_ROWS = 8;

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

  // Two players: the active one, and a standby that loads the next song near the end of
  // this one, so the switch has no gap (and "next" is instant).
  #audio = new Audio();
  #standby = new Audio();
  #standbyKey = null;
  #token = 0;
  #dragging = false;
  #resumeAt = 0;
  #original = null; // queue order before shuffling, as keys
  #counted = false;
  // Scrobbling: "now playing" once per song, a scrobble once it's been played long enough.
  #announced = false;
  #scrobbled = false;
  #startedAt = 0; // unix seconds when this song started
  #pausedAt = 0;
  #onPlay = [];
  /** Sleep timer: { until: ms, minutes } or { endOfSong: true }, or null. */
  sleep = $state(null);
  sleepLeft = $state(0); // seconds, for the countdown
  #fade = $state(1); // volume factor while the timer fades out
  /** Crossfade length in seconds (0 = off). */
  crossfade = $state(Number(localStorage.getItem("kite.crossfade")) || 0);
  #xf = null; // { old, start, dur } while two songs overlap
  // Volume per output device (Windows tells us when the default device changes).
  #device = null;
  #deviceVolumes = JSON.parse(localStorage.getItem("kite.deviceVolumes") || "{}");
  #xfTimer = null;
  #sleepTick = null;
  #warmToken = 0;
  #warming = null; // the list warmList is working through
  #shuffleStart = new WeakMap(); // list → the song Shuffle will start with, picked (and prepared) ahead
  #retry = { key: null, at: 0 }; // last silent retry, so a broken song isn't retried forever

  constructor() {
    this.#audio.preload = "auto";
    this.#standby.preload = "auto";
    this.#restore();
    this.#wireAudio(this.#audio);
    this.#wireAudio(this.#standby);
    this.#wireMediaControls();
    invoke("audio_device").then((d) => this.#onDevice(d, false)).catch(() => {});
    listen("audio-device", ({ payload }) => this.#onDevice(payload, true));

    $effect.root(() => {
      $effect(() => {
        this.volume;
        this.#fade;
        this.#applyVolume();
      });
      $effect(() => {
        localStorage.setItem("kite.crossfade", String(this.crossfade));
      });
      $effect(() => {
        const v = this.volume;
        if (!this.#device) return;
        this.#deviceVolumes[this.#device] = v;
        localStorage.setItem("kite.deviceVolumes", JSON.stringify(this.#deviceVolumes));
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

  /** Put a song (or several, in order) right after the current one. */
  playNext(tracks) {
    tracks = [tracks].flat();
    if (this.index < 0) return this.playList(tracks, 0);
    const items = tracks.map(toItem);
    this.queue.splice(this.index + 1, 0, ...items);
    this.#original?.push(...items.map((t) => t.key));
  }

  /** Add a song (or several) to the end of the queue. */
  addToQueue(tracks) {
    tracks = [tracks].flat();
    if (this.index < 0) return this.playList(tracks, 0);
    const items = tracks.map(toItem);
    this.queue.push(...items);
    this.#original?.push(...items.map((t) => t.key));
  }

  /** Play `tracks` in random order. */
  playShuffled(tracks) {
    if (!tracks.length) return;
    this.shuffle = true;
    const picked = this.#shuffleStart.get(tracks);
    this.#shuffleStart.delete(tracks);
    this.playList(tracks, picked < tracks.length ? picked : Math.floor(Math.random() * tracks.length));
  }

  /** A list was opened: get the songs most likely to be played first ready (the top two, and
   *  the one Shuffle will start with), so they start without the lookup wait. Then the rest of
   *  the first screenful, one at a time, until another list opens or this one closes. */
  async warmList(tracks) {
    const token = ++this.#warmToken;
    this.#warming = tracks;
    if (!tracks?.length) return;
    const pick = Math.floor(Math.random() * tracks.length);
    this.#shuffleStart.set(tracks, pick);
    const first = [...new Set([tracks[0], tracks[1], tracks[pick]])];
    await Promise.all(first.map((t) => this.#prepare(t)));
    for (const t of tracks.slice(0, WARM_ROWS)) {
      if (token !== this.#warmToken) return;
      if (!first.includes(t)) await this.#prepare(t);
    }
  }

  /** `tracks` was closed: stop preparing it in the background (unless another list took over). */
  stopWarming(tracks) {
    if (tracks === this.#warming) this.#warmToken++;
  }

  #prepare(t) {
    return t?.url && t.source !== "local" ? invoke("prepare", { url: t.url }).catch(() => {}) : Promise.resolve();
  }

  /** The next two songs, so skipping twice in a row is instant too. */
  #warmUpcoming() {
    this.warm(this.queue[this.index + 1]);
    this.warm(this.queue[this.index + 2]);
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
    const cleared = this.queue.slice(this.index + 1);
    if (!cleared.length) return;
    const original = this.#original;
    const keep = new Set(this.queue.slice(0, this.index + 1).map((t) => t.key));
    this.queue = this.queue.slice(0, this.index + 1);
    if (original) this.#original = original.filter((k) => keep.has(k));
    const text = cleared.length === 1 ? "Cleared 1 song from the queue" : `Cleared ${cleared.length} songs from the queue`;
    toasts.undo(text, () => {
      // Put them back after whatever is playing now, skipping any that are already back.
      const have = new Set(this.queue.map((t) => t.key));
      this.queue.splice(this.index + 1, 0, ...cleared.filter((t) => !have.has(t.key)));
      if (original) {
        const now = new Set(this.queue.map((t) => t.key));
        const known = new Set(original);
        this.#original = [...original.filter((k) => now.has(k)), ...[...now].filter((k) => !known.has(k))];
      }
    });
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
    if (!this.#audio.getAttribute("src")) {
      // Still resolving the song: a second press cancels instead of starting over.
      if (this.loading) return this.#cancelLoad();
      return this.#playAt(this.index, this.#resumeAt);
    }
    if (this.error) return this.#playAt(this.index, this.#resumeAt);
    if (!this.#audio.paused) return this.#audio.pause();
    if (this.current.source !== "local" && !this.#audio.ended && Date.now() - this.#pausedAt > LONG_PAUSE) {
      return this.#playAt(this.index, this.#audio.currentTime, true);
    }
    this.#audio.play();
  }

  /** Stops a song that's still loading and keeps where it would have started. */
  #cancelLoad() {
    this.#token++;
    this.loading = false;
    this.playing = false;
    this.#resumeAt = this.time;
    this.#media("paused");
  }

  /** Stop after `minutes`, at the end of this song ("song"), or never (null). */
  setSleep(when) {
    clearInterval(this.#sleepTick);
    this.#fade = 1;
    this.sleepLeft = 0;
    if (!when) return void (this.sleep = null);
    if (when === "song") return void (this.sleep = { endOfSong: true });
    this.sleep = { until: Date.now() + when * 60_000, minutes: when };
    const tick = () => {
      const left = (this.sleep?.until - Date.now()) / 1000;
      this.sleepLeft = Math.max(0, Math.ceil(left));
      // The last 10 seconds fade out, then playback pauses.
      this.#fade = Math.max(0, Math.min(1, left / 10));
      if (left <= 0) {
        if (this.playing) this.#audio.pause();
        this.setSleep(null);
      }
    };
    tick();
    this.#sleepTick = setInterval(tick, 250);
  }

  next(auto = false) {
    if (auto && this.sleep?.endOfSong) {
      // Sleep timer set to the end of the song: stop here instead of going on.
      this.setSleep(null);
      this.playing = false;
      this.#media("paused");
      return;
    }
    if (auto && this.repeat === "one") {
      this.#audio.currentTime = 0;
      this.#audio.play();
    } else if (this.#standbyReady()) {
      this.#swapToStandby();
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

  /** Dev builds only, for test scripts: pretend the pause began `ms` earlier, and reach the
   *  active audio element. */
  _test(ms = 0) {
    if (!import.meta.env.DEV) return null;
    this.#pausedAt -= ms;
    return this.#audio;
  }

  /** The output device changed (or Kite started): use the volume last set on it. */
  #onDevice(d, announce) {
    if (!d?.id) return;
    this.#device = d.id;
    const saved = this.#deviceVolumes[d.id];
    if (typeof saved === "number") this.volume = saved;
    if (announce) toasts.show(`Playing on ${d.name || "another device"} · volume ${Math.round(this.volume * 100)}%`);
  }

  // ---- hooks for the sound engine ------------------------------------------

  /** Both audio elements (the playing one and the standby). */
  audioElements() {
    return [this.#audio, this.#standby];
  }

  /** Speed, and whether pitch follows it ("slowed" and "nightcore" don't keep pitch). */
  setRate(speed, keepPitch) {
    for (const a of [this.#audio, this.#standby]) {
      a.defaultPlaybackRate = speed;
      a.playbackRate = speed;
      a.preservesPitch = keepPitch;
    }
  }

  onPlay(fn) {
    this.#onPlay.push(fn);
  }

  /** Web Audio can only hear streams fetched with CORS. Switching reloads the current song
   *  once, at the same spot. */
  useCors() {
    if (this.#audio.crossOrigin === "anonymous") return;
    this.#clearStandby();
    for (const a of [this.#audio, this.#standby]) a.crossOrigin = "anonymous";
    if (!this.#audio.getAttribute("src") || !this.current) return;
    const at = this.#audio.currentTime;
    if (!this.#audio.paused) return void this.#playAt(this.index, at, true);
    this.#audio.removeAttribute("src");
    this.#audio.load();
    this.#resumeAt = at;
  }

  // ---- internals ---------------------------------------------------------

  /** `resume`: the same song again (after a long pause or a dropped stream), so it isn't
   *  counted as a new play. */
  async #playAt(i, startAt = 0, resume = false) {
    const track = this.queue[i];
    if (!track) return;
    this.#clearStandby();
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
    if (!resume) this.#newSong();
    this.#sendTrack(track);

    try {
      if (track.source !== "local") await invoke("prepare", { url: track.url });
    } catch (e) {
      if (token === this.#token) this.#failed(String(e));
      return;
    }
    if (token !== this.#token) return;

    audio.src = streamUrl(track);
    if (startAt > 0) audio.currentTime = startAt;
    audio.play().catch((e) => {
      if (e.name !== "AbortError" && token === this.#token) this.#failed();
    });

    this.#warmUpcoming();
  }

  /** A song can't play: say so, and move on to the next one. */
  #failed(reason = "This song couldn't be played.") {
    const token = this.#token;
    const track = this.current;
    this.loading = false;
    this.playing = false;
    this.error = reason;
    invoke("log_line", { text: `couldn't play "${track?.title}" (${track?.url}): ${reason}` }).catch(() => {});
    if (this.index + 1 >= this.queue.length) return;
    toasts.show(`Couldn't play ${track?.title ?? "this song"}, skipping`);
    setTimeout(() => {
      if (token === this.#token && this.error) this.#playAt(this.index + 1);
    }, 1500);
  }

  #standbyReady() {
    const next = this.queue[this.index + 1];
    return !!next && this.#standbyKey === next.key && !!this.#standby.getAttribute("src");
  }

  #clearStandby() {
    this.#endCrossfade();
    this.#standbyKey = null;
    this.#standby.pause();
    this.#standby.removeAttribute("src");
    this.#standby.load();
  }

  /** Near the end of a song: load the next one in the standby player. */
  async #preloadNext() {
    const next = this.queue[this.index + 1];
    if (!next || this.repeat === "one") return;
    this.#standbyKey = next.key;
    try {
      if (next.source !== "local") await invoke("prepare", { url: next.url });
    } catch {
      this.#standbyKey = null;
      return;
    }
    if (this.#standbyKey !== next.key) return;
    this.#standby.src = streamUrl(next);
    this.#standby.load();
  }

  /** The song ended (or "next" was pressed) and the next one is already loaded: switch now. */
  #swapToStandby() {
    this.#endCrossfade();
    const old = this.#audio;
    this.#audio = this.#standby;
    this.#standby = old;
    this.#standbyKey = null;
    old.pause();
    old.removeAttribute("src");
    old.load();
    const token = ++this.#token;
    this.index += 1;
    this.error = "";
    this.time = 0;
    this.duration = this.current?.duration || 0;
    this.#newSong();
    this.#sendTrack(this.current);
    this.#audio.currentTime = 0;
    this.#audio.play().catch((e) => {
      if (e.name !== "AbortError" && token === this.#token) this.#failed();
    });
    this.#warmUpcoming();
  }

  // ---- crossfade --------------------------------------------------------------

  /** Volume of both players: the user's volume, the sleep-timer fade, and during a crossfade
   *  an equal-power blend (no dip in the middle). */
  #applyVolume() {
    const v = this.volume * this.#fade;
    if (!this.#xf) {
      this.#audio.volume = v;
      this.#standby.volume = v;
      return;
    }
    const p = Math.min(1, (performance.now() - this.#xf.start) / (this.#xf.dur * 1000));
    this.#audio.volume = v * Math.sin((p * Math.PI) / 2);
    this.#xf.old.volume = v * Math.cos((p * Math.PI) / 2);
    if (p >= 1) this.#endCrossfade();
  }

  /** Whether the current song should fade into the next one now. */
  #shouldCrossfade(a, left) {
    return (
      this.crossfade > 0 &&
      !this.#xf &&
      left <= this.crossfade &&
      left > 0.5 &&
      a.duration > this.crossfade * 2 + 5 &&
      this.repeat !== "one" &&
      !this.sleep?.endOfSong &&
      this.#standbyReady()
    );
  }

  /** The next song (already loaded in the standby player) starts while this one fades out. */
  #crossfadeTo(dur) {
    const old = this.#audio;
    this.#audio = this.#standby;
    this.#standby = old;
    this.#standbyKey = null;
    this.#xf = { old, start: performance.now(), dur };
    const token = ++this.#token;
    this.index += 1;
    this.error = "";
    this.time = 0;
    this.duration = this.current?.duration || 0;
    this.#newSong();
    this.#sendTrack(this.current);
    this.#audio.currentTime = 0;
    this.#applyVolume();
    this.#audio.play().catch((e) => {
      if (e.name !== "AbortError" && token === this.#token) this.#failed();
    });
    this.#xfTimer = setInterval(() => this.#applyVolume(), 50);
    this.#warmUpcoming();
  }

  /** Stop the fading-out song (the fade finished, or the user did something). */
  #endCrossfade() {
    if (!this.#xf) return;
    const { old } = this.#xf;
    this.#xf = null;
    clearInterval(this.#xfTimer);
    old.pause();
    old.removeAttribute("src");
    old.load();
    this.#applyVolume();
  }

  #newSong() {
    this.#counted = false;
    this.#announced = false;
    this.#scrobbled = false;
  }

  #plain(t) {
    return Object.fromEntries(TRACK_FIELDS.map((f) => [f, t[f] ?? null]));
  }

  #stop() {
    this.#token++;
    this.#clearStandby();
    this.#audio.pause();
    this.#audio.removeAttribute("src");
    this.index = this.queue.length ? Math.min(this.index, this.queue.length - 1) : -1;
    this.playing = false;
    this.loading = false;
    this.time = 0;
    this.#media("stopped");
  }

  /** Events from both players; the standby one is ignored until it becomes active. */
  #wireAudio(a) {
    const active = () => a === this.#audio;
    a.addEventListener("playing", () => {
      if (!active()) return;
      for (const fn of this.#onPlay) fn();
      this.playing = true;
      this.loading = false;
      this.#media("playing");
      if (!this.#announced && this.current) {
        this.#announced = true;
        this.#startedAt = Math.floor(Date.now() / 1000 - a.currentTime);
        invoke("scrobble_now_playing", { track: this.#plain(this.current) }).catch(() => {});
      }
    });
    a.addEventListener("pause", () => {
      if (!active()) return;
      this.#endCrossfade();
      this.#pausedAt = Date.now();
      this.playing = false;
      this.#media("paused");
      this.#savePosition();
    });
    a.addEventListener("waiting", () => active() && (this.loading = true));
    a.addEventListener("canplay", () => active() && (this.loading = false));
    a.addEventListener("seeked", () => active() && this.#media(a.paused ? "paused" : "playing"));
    let lastSave = 0;
    a.addEventListener("timeupdate", () => {
      if (!active()) return;
      if (!this.#dragging) this.time = a.currentTime;
      // Close to the end: get the next song ready in the standby player (again, if the queue
      // changed since).
      const left = a.duration - a.currentTime;
      const next = this.queue[this.index + 1];
      if (next && Number.isFinite(left) && left < PRELOAD_BEFORE_END && this.#standbyKey !== next.key && !this.#xf) {
        this.#preloadNext();
      }
      if (Number.isFinite(left) && this.#shouldCrossfade(a, left)) this.#crossfadeTo(Math.min(this.crossfade, left));
      // Counts as a play after 30 s, or halfway through shorter songs.
      if (!this.#counted && this.current && a.currentTime >= Math.min(30, (a.duration || 60) / 2)) {
        this.#counted = true;
        invoke("lib_played", { track: this.#plain(this.current) }).catch(() => {});
      }
      // Scrobbles follow the usual rule: songs over 30 s, once half (or 4 minutes) has played.
      const d = a.duration;
      if (!this.#scrobbled && this.current && d > 30 && a.currentTime >= Math.min(d / 2, 240)) {
        this.#scrobbled = true;
        invoke("scrobble", { track: this.#plain(this.current), startedAt: this.#startedAt }).catch(() => {});
      }
      if (Date.now() - lastSave > 5000) {
        lastSave = Date.now();
        this.#savePosition();
        if (!a.paused) this.#media("playing");
      }
    });
    a.addEventListener("durationchange", () => {
      if (!active()) return;
      if (Number.isFinite(a.duration) && a.duration > 0) {
        this.duration = a.duration;
        if (this.current) this.#sendTrack(this.current, a.duration);
      }
    });
    a.addEventListener("ended", () => active() && this.next(true));
    a.addEventListener("error", () => {
      if (!a.getAttribute("src")) return;
      // A standby that failed to load: forget it, the normal path will try again.
      if (!active()) return void (this.#standbyKey = null);
      const t = this.current;
      if (!t) return;
      // A song that was already playing lost its stream (an expired link, a network blip): load
      // it again at the same spot, once a minute at most, before giving up on it.
      const at = a.currentTime;
      const recent = this.#retry.key === t.key && Date.now() - this.#retry.at < 60_000;
      if (at > 1 && !recent) {
        this.#retry = { key: t.key, at: Date.now() };
        return void this.#playAt(this.index, at, true);
      }
      this.#failed();
    });
  }

  #wireMediaControls() {
    listen("media-control", ({ payload: p }) => {
      switch (p.action) {
        case "play":
          if (!this.playing && !this.loading) this.toggle();
          break;
        case "pause":
          if (this.playing || this.loading) this.toggle();
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
      url: t.url,
      album: t.album || null,
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
