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
};

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
    (this.s.eqOn && this.s.eq.some((g) => g !== 0)) || this.s.reverb > 0 || this.s.eightD || this.s.mono || this.s.balance !== 0
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
    });
    // A context made before any click starts suspended; wake it whenever music plays.
    player.onPlay(() => this.#ctx?.state === "suspended" && this.#ctx.resume());
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
    panner.connect(mono).connect(limiter).connect(ctx.destination);
    this.#nodes = { input, filters, last, reverb, reverbOn: false, dry, wet, panner, depth, mono };
    for (const el of player.audioElements()) this.#connect(el);
  }

  #connect(el) {
    if (this.#connected.has(el)) return;
    this.#connected.add(el);
    this.#ctx.createMediaElementSource(el).connect(this.#nodes.input);
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
