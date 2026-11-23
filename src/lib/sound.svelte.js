// Sound: equalizer, reverb, 8D, mono and balance through Web Audio, plus speed and pitch on the
// audio elements themselves. The Web Audio graph is only built once something needs it, so
// plain listening stays as light as before.
//
//   audio (×2) ─► input ─► 10-band EQ ─► dry ──────────────┐
//                                     └► convolver ► wet ──┴► panner ─► mono ─► limiter ─► out
//                                                     8D LFO ┘

import { untrack } from "svelte";
import { player } from "./player.svelte.js";

const KEY = "kite.sound";
/** Equalizer per output device: { "Speakers (…)": { eqOn, eq, eqPreset } }. */
const EQ_KEY = "kite.outputEq";
export const BANDS = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

export const EQ_PRESETS = {
  Flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  "Bass boost": [7, 6, 4.5, 2, 0, 0, 0, 0, 0, 0],
  "Treble boost": [0, 0, 0, 0, 0, 0, 1.5, 3.5, 5, 6],
  Vocal: [-2, -2, -1, 1, 3.5, 4, 3, 1.5, 0, -1],
  Rock: [4.5, 3.5, 2, 0, -1, -0.5, 1, 2.5, 3.5, 4],
  Pop: [-1, 0, 1.5, 3, 4, 3, 1.5, 0, -0.5, -1],
  Electronic: [5, 4, 1.5, 0, -1.5, 1, 0, 1.5, 4, 5],
  "Hip-hop": [5, 4.5, 2, 3, 0, -1, 1, 0, 1.5, 2.5],
  Acoustic: [3.5, 3, 2.5, 1, 1.5, 1.5, 2.5, 3, 2.5, 1.5],
  "Late night": [3, 2, 1, 0, -1, -1, 0, 1, 2, 2.5],
};

/** One-click looks: speed, pitch, reverb and 8D together. */
export const EFFECTS = {
  Normal: { speed: 1, keepPitch: true, reverb: 0, eightD: false },
  Slowed: { speed: 0.85, keepPitch: false, reverb: 0, eightD: false },
  "Slowed + reverb": { speed: 0.85, keepPitch: false, reverb: 0.35, eightD: false },
  Nightcore: { speed: 1.25, keepPitch: false, reverb: 0, eightD: false },
  "Sped up": { speed: 1.2, keepPitch: true, reverb: 0, eightD: false },
  "8D": { speed: 1, keepPitch: true, reverb: 0.15, eightD: true },
};

const DEFAULTS = {
  eqOn: false,
  eq: EQ_PRESETS.Flat.slice(),
  eqPreset: "Flat",
  speed: 1,
  keepPitch: true,
  reverb: 0,
  eightD: false,
  mono: false,
  balance: 0,
  level: false,
};

// ---- volume levelling ---------------------------------------------------------------
/** Where songs are brought to (dBFS of the weighted signal; streaming services aim near here). */
const TARGET_DB = -18;
const MIN_GAIN_DB = -12;
const MAX_GAIN_DB = 6;
const LOUDNESS_KEY = "kite.loudness";
const db2lin = (db) => Math.pow(10, db / 20);

/** Measured loudness per song (by stream URL), so a song starts at the right level next time. */
const remembered = (() => {
  try {
    return new Map(JSON.parse(localStorage.getItem(LOUDNESS_KEY) || "[]"));
  } catch {
    return new Map();
  }
})();
function remember(key, db) {
  remembered.delete(key);
  remembered.set(key, Math.round(db * 10) / 10);
  while (remembered.size > 3000) remembered.delete(remembered.keys().next().value);
  localStorage.setItem(LOUDNESS_KEY, JSON.stringify([...remembered]));
}

function load() {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) || "{}") };
  } catch {
    return { ...DEFAULTS };
  }
}

/** A stereo reverb tail: decaying noise, generated instead of shipping an impulse file. */
function impulse(ctx, seconds = 2.6, decay = 3) {
  const len = Math.floor(ctx.sampleRate * seconds);
  const buf = ctx.createBuffer(2, len, ctx.sampleRate);
  for (let c = 0; c < 2; c++) {
    const d = buf.getChannelData(c);
    for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, decay);
  }
  return buf;
}

class Sound {
  s = $state(load());

  /** Whether anything needs the Web Audio graph (speed and pitch don't). */
  needsGraph = $derived(
    (this.s.eqOn && this.s.eq.some((g) => g !== 0)) ||
      this.s.reverb > 0 ||
      this.s.eightD ||
      this.s.mono ||
      this.s.balance !== 0 ||
      this.s.level ||
      (player.crossfade > 0 && player.crossfadeSmart)
  );
  /** Anything changed from plain playback (for the "on" dot on the Sound button). */
  active = $derived(this.needsGraph || this.s.speed !== 1);

  #ctx = null;
  #nodes = null;
  #connected = new WeakSet();

  constructor() {
    $effect.root(() => {
      $effect(() => {
        localStorage.setItem(KEY, JSON.stringify(this.s));
      });
      // Speed and pitch live on the audio elements.
      $effect(() => {
        const { speed, keepPitch } = this.s;
        player.setRate(speed, keepPitch);
      });
      // Build the graph the first time it's needed, then keep it in step with the settings.
      $effect(() => {
        if (this.needsGraph) untrack(() => this.#build());
        if (this.#nodes) this.#apply();
      });
      // Each output keeps its own equalizer: switching to the headphones brings theirs back.
      const eqs = (() => {
        try {
          return JSON.parse(localStorage.getItem(EQ_KEY) || "{}");
        } catch {
          return {};
        }
      })();
      let prev = null;
      $effect(() => {
        const name = player.outputName;
        untrack(() => {
          // The one we're leaving keeps what it had, even if it was never changed while on it.
          if (prev && prev !== name) eqs[prev] = { eqOn: this.s.eqOn, eq: this.s.eq.slice(), eqPreset: this.s.eqPreset };
          prev = name || prev;
          const saved = name && eqs[name];
          if (saved) this.s = { ...this.s, eqOn: saved.eqOn, eq: saved.eq.slice(), eqPreset: saved.eqPreset };
        });
      });
      // Not tracking the name here: on a switch, the effect above loads first.
      $effect(() => {
        const { eqOn, eq, eqPreset } = this.s;
        const name = untrack(() => player.outputName);
        if (!name) return;
        eqs[name] = { eqOn, eq: eq.slice(), eqPreset };
        localStorage.setItem(EQ_KEY, JSON.stringify(eqs));
      });
    });
    // A context made before any click starts suspended; wake it whenever music plays.
    player.onPlay(() => this.#ctx?.state === "suspended" && this.#ctx.resume());
    player.setQuietProbe((el) => this.#quiet(el));
  }

  #build() {
    if (this.#ctx) return;
    // Streams must be fetched with CORS for Web Audio to hear them (the song reloads once).
    player.useCors();
    const ctx = (this.#ctx = new AudioContext({ latencyHint: "playback" }));
    const input = ctx.createGain();
    const filters = BANDS.map((f, i) => {
      const b = ctx.createBiquadFilter();
      b.type = i === 0 ? "lowshelf" : i === BANDS.length - 1 ? "highshelf" : "peaking";
      b.frequency.value = f;
      b.Q.value = 1.1;
      return b;
    });
    filters.reduce((a, b) => (a.connect(b), b), input);
    const last = filters.at(-1);
    const dry = ctx.createGain();
    const wet = ctx.createGain();
    const reverb = ctx.createConvolver();
    reverb.buffer = impulse(ctx);
    const panner = ctx.createStereoPanner();
    last.connect(dry).connect(panner);
    // The reverb branch is only connected while reverb is up (convolving costs CPU even at 0).
    reverb.connect(wet).connect(panner);
    // 8D: a slow oscillator swings the panner left and right.
    const lfo = ctx.createOscillator();
    lfo.frequency.value = 0.12;
    const depth = ctx.createGain();
    depth.gain.value = 0;
    lfo.connect(depth).connect(panner.pan);
    lfo.start();
    // Mono: one channel in, the same sound on both speakers.
    const mono = ctx.createGain();
    mono.channelInterpretation = "speakers";
    // A limiter that only acts near clipping, since EQ boosts can push past full scale.
    const limiter = ctx.createDynamicsCompressor();
    limiter.threshold.value = -1;
    limiter.knee.value = 0;
    limiter.ratio.value = 20;
    limiter.attack.value = 0.002;
    limiter.release.value = 0.15;
    const master = ctx.createGain();
    panner.connect(mono).connect(limiter).connect(master).connect(ctx.destination);
    this.#nodes = { input, filters, last, reverb, reverbOn: false, dry, wet, panner, depth, mono, master };
    for (const el of player.audioElements()) this.#connect(el);
    // Out of the speakers or headphones picked in Kite, like the plain players.
    player.onSink((id) => ctx.setSinkId?.(id).catch((e) => console.warn("output", e)));
  }

  /** Dev builds only: build the graph and send nothing to the speakers, so test scripts can
   *  play songs (and measure them) without a sound. */
  _silence(on = true) {
    if (!import.meta.env.DEV) return;
    this.#build();
    this.#nodes.master.gain.value = on ? 0 : 1;
  }

  /** Each player gets its own levelling gain and a meter that hears the song before it. */
  #meters = [];
  #levelTimer = null;
  #levelOn = false;
  #measuring = false;

  #connect(el) {
    if (this.#connected.has(el)) return;
    this.#connected.add(el);
    const ctx = this.#ctx;
    const src = ctx.createMediaElementSource(el);
    const gain = ctx.createGain();
    src.connect(gain).connect(this.#nodes.input);
    // A rough K-weighting (how loud it sounds, not just how big the waveform is): cut the
    // deep bass, lift the upper mids, then measure.
    const hp = ctx.createBiquadFilter();
    hp.type = "highpass";
    hp.frequency.value = 60;
    const shelf = ctx.createBiquadFilter();
    shelf.type = "highshelf";
    shelf.frequency.value = 1500;
    shelf.gain.value = 4;
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 4096;
    src.connect(hp).connect(shelf).connect(analyser);
    this.#meters.push({ el, gain, analyser, buf: new Float32Array(analyser.fftSize), key: "", sum: 0, n: 0, st: null });
  }

  /** How loud a player is right now and on average this song (dB), for the DJ crossfade.
   *  The average needs 5 seconds of sound first. */
  #quiet(el) {
    const m = this.#meters.find((x) => x.el === el);
    if (!m || m.st == null) return null;
    return { now: 10 * Math.log10(m.st + 1e-12), avg: m.n >= 50 ? 10 * Math.log10(m.sum / m.n) : null };
  }

  /** Every 100 ms: measure what each playing song sounds like (for the DJ crossfade) and, with
   *  levelling on, steer its gain toward the common level. Quiet passages (below -50 dB) don't
   *  count toward the average, so a soft intro doesn't get pushed up. */
  #level() {
    const t = this.#ctx.currentTime;
    for (const m of this.#meters) {
      const key = m.el.currentSrc || m.el.src;
      if (key !== m.key) {
        // A new song in this player: start from what we measured last time, if anything.
        m.key = key;
        m.sum = 0;
        m.n = 0;
        m.st = null;
        const known = remembered.get(key);
        if (this.s.level) m.gain.gain.setTargetAtTime(known === undefined ? 1 : db2lin(this.#gainFor(known)), t, 0.05);
      }
      if (!key || m.el.paused || m.el.volume < 0.05) continue;
      m.analyser.getFloatTimeDomainData(m.buf);
      let ms = 0;
      for (const x of m.buf) ms += x * x;
      ms /= m.buf.length;
      // The player's own volume (and crossfades) come before the meter: take them back out.
      const db = 10 * Math.log10(ms + 1e-12) - 20 * Math.log10(m.el.volume);
      const power = Math.pow(10, db / 10);
      // The last second or so, quiet parts included.
      m.st = m.st == null ? power : m.st * 0.8 + power * 0.2;
      if (db < -50) continue;
      m.sum += power;
      m.n++;
      if (!this.s.level || m.n < 20) continue; // two seconds before trusting it
      const loud = 10 * Math.log10(m.sum / m.n);
      m.gain.gain.setTargetAtTime(db2lin(this.#gainFor(loud)), t, 1.5);
      if (m.n % 50 === 0) remember(key, loud);
    }
  }

  #gainFor(loudDb) {
    return Math.max(MIN_GAIN_DB, Math.min(MAX_GAIN_DB, TARGET_DB - loudDb));
  }

  #apply() {
    const n = this.#nodes;
    const { filters, dry, wet, panner, depth, mono } = n;
    if ((this.s.reverb > 0) !== n.reverbOn) {
      n.reverbOn = this.s.reverb > 0;
      n.reverbOn ? n.last.connect(n.reverb) : n.last.disconnect(n.reverb);
    }
    const t = this.#ctx.currentTime;
    const smooth = (param, v) => param.setTargetAtTime(v, t, 0.03);
    filters.forEach((f, i) => smooth(f.gain, this.s.eqOn ? this.s.eq[i] : 0));
    smooth(dry.gain, 1 - this.s.reverb * 0.4);
    smooth(wet.gain, this.s.reverb * 1.2);
    // With 8D on, the oscillator moves the pan; balance shifts the middle it swings around.
    smooth(panner.pan, this.s.balance);
    smooth(depth.gain, this.s.eightD ? 0.85 : 0);
    mono.channelCount = this.s.mono ? 1 : 2;
    mono.channelCountMode = this.s.mono ? "explicit" : "max";
    // Measuring runs while levelling or the DJ crossfade needs it; with levelling off, every
    // player is at plain volume. Only an on/off change restarts it (EQ drags mustn't reset
    // the measurements).
    const measure = this.s.level || (player.crossfade > 0 && player.crossfadeSmart);
    if (measure !== this.#measuring) {
      this.#measuring = measure;
      clearInterval(this.#levelTimer);
      if (measure) this.#levelTimer = setInterval(() => this.#level(), 100);
    }
    if (this.s.level !== this.#levelOn) {
      this.#levelOn = this.s.level;
      if (this.s.level) for (const m of this.#meters) m.key = ""; // start from remembered levels
      else for (const m of this.#meters) m.gain.gain.setTargetAtTime(1, t, 0.1);
    }
  }

  set(patch) {
    this.s = { ...this.s, ...patch };
  }

  setBand(i, gain) {
    const eq = this.s.eq.slice();
    eq[i] = gain;
    this.set({ eq, eqPreset: "Custom", eqOn: true });
  }

  eqPreset(name) {
    this.set({ eq: EQ_PRESETS[name].slice(), eqPreset: name, eqOn: name !== "Flat" || this.s.eqOn });
  }

  effect(name) {
    this.set({ ...EFFECTS[name] });
  }

  /** The effect preset the current settings match, if any. */
  get effectName() {
    return Object.entries(EFFECTS).find(([, e]) => Object.entries(e).every(([k, v]) => this.s[k] === v))?.[0] ?? null;
  }

  reset() {
    this.s = { ...DEFAULTS, eq: EQ_PRESETS.Flat.slice() };
  }
}

export const sound = new Sound();
