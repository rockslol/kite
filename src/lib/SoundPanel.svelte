<script>
  // Sound settings beside the list: one-click effects, speed and pitch, reverb, 8D, the
  // equalizer, balance and mono, and the sleep timer.
  import { player } from "./player.svelte.js";
  import { sound, BANDS, EQ_PRESETS, EFFECTS } from "./sound.svelte.js";

  const label = (f) => (f >= 1000 ? `${f / 1000}k` : `${f}`);
  const db = (g) => `${g > 0 ? "+" : ""}${g.toFixed(1)} dB`;
  const clock = (s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  const SLEEP = [
    [15, "15 min"],
    [30, "30 min"],
    [60, "1 hour"],
    ["song", "End of song"],
  ];
</script>

<aside>
  <div class="head">
    <h2>Sound</h2>
    {#if sound.active}<button class="text" onclick={() => sound.reset()}>Reset</button>{/if}
  </div>

  <div class="scroll">
    <p class="label">Effects</p>
    <div class="chips">
      {#each Object.keys(EFFECTS) as name}
        <button class="chip" class:on={sound.effectName === name} onclick={() => sound.effect(name)}>{name}</button>
      {/each}
    </div>

    <div class="field">
      <span>Speed</span>
      <input type="range" min="0.5" max="1.5" step="0.05" value={sound.s.speed} oninput={(e) => sound.set({ speed: +e.currentTarget.value })} />
      <span class="num">{sound.s.speed.toFixed(2)}×</span>
    </div>
    <label class="check">
      <input type="checkbox" checked={sound.s.keepPitch} onchange={(e) => sound.set({ keepPitch: e.currentTarget.checked })} />
      Keep the pitch when changing speed
    </label>
    <div class="field">
      <span>Reverb</span>
      <input type="range" min="0" max="1" step="0.05" value={sound.s.reverb} oninput={(e) => sound.set({ reverb: +e.currentTarget.value })} />
      <span class="num">{Math.round(sound.s.reverb * 100)}%</span>
    </div>
    <label class="check">
      <input type="checkbox" checked={sound.s.eightD} onchange={(e) => sound.set({ eightD: e.currentTarget.checked })} />
      8D (the sound circles around you; best with headphones)
    </label>

    <div class="row">
      <p class="label">Equalizer</p>
      <label class="check inline">
        <input type="checkbox" checked={sound.s.eqOn} onchange={(e) => sound.set({ eqOn: e.currentTarget.checked })} />
        On
      </label>
    </div>
    <div class="chips">
      {#each Object.keys(EQ_PRESETS) as name}
        <button class="chip" class:on={sound.s.eqOn && sound.s.eqPreset === name} onclick={() => sound.eqPreset(name)}>{name}</button>
      {/each}
    </div>
    <div class="eq" class:off={!sound.s.eqOn}>
      {#each BANDS as f, i}
        <div class="band" title="{label(f)} Hz: {db(sound.s.eq[i])}">
          <input
            type="range"
            min="-12"
            max="12"
            step="0.5"
            value={sound.s.eq[i]}
            oninput={(e) => sound.setBand(i, +e.currentTarget.value)}
            ondblclick={() => sound.setBand(i, 0)}
            aria-label="{label(f)} Hz"
          />
          <span>{label(f)}</span>
        </div>
      {/each}
    </div>

    <p class="label">Listening</p>
    <div class="field">
      <span>Balance</span>
      <input
        type="range"
        min="-1"
        max="1"
        step="0.05"
        value={sound.s.balance}
        oninput={(e) => sound.set({ balance: +e.currentTarget.value })}
        ondblclick={() => sound.set({ balance: 0 })}
      />
      <span class="num">{sound.s.balance === 0 ? "Center" : sound.s.balance < 0 ? `L ${Math.round(-sound.s.balance * 100)}` : `R ${Math.round(sound.s.balance * 100)}`}</span>
    </div>
    <label class="check">
      <input type="checkbox" checked={sound.s.mono} onchange={(e) => sound.set({ mono: e.currentTarget.checked })} />
      Mono (the same sound in both ears)
    </label>

    <p class="label">Sleep timer</p>
    <div class="chips">
      {#each SLEEP as [when, text]}
        {@const on = when === "song" ? !!player.sleep?.endOfSong : player.sleep?.minutes === when}
        <button class="chip" class:on onclick={() => player.setSleep(when)}>{text}</button>
      {/each}
      {#if player.sleep}<button class="chip" onclick={() => player.setSleep(null)}>Off</button>{/if}
    </div>
    {#if player.sleep?.until}
      <p class="hint">Stops in {clock(player.sleepLeft)}</p>
    {:else if player.sleep?.endOfSong}
      <p class="hint">Stops when this song ends</p>
    {/if}
  </div>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--border);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 34px;
    padding: 4px 16px 8px;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .text {
    color: var(--muted);
    font-size: 13px;
  }
  .text:hover {
    color: var(--text);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 16px 16px;
  }
  .label {
    margin: 14px 0 8px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 13px;
    color: var(--muted);
    font-size: 12px;
  }
  .chip:hover {
    color: var(--text);
  }
  .chip.on {
    background: var(--accent-soft);
    border-color: transparent;
    color: var(--text);
  }
  .field {
    display: grid;
    grid-template-columns: 58px 1fr 52px;
    gap: 8px;
    align-items: center;
    margin-top: 12px;
    font-size: 13px;
  }
  .num {
    text-align: right;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-top: 8px;
    color: var(--muted);
    font-size: 12px;
  }
  .check.inline {
    margin: 0;
  }
  .eq {
    display: grid;
    grid-template-columns: repeat(10, 1fr);
    margin-top: 12px;
    transition: opacity 0.2s;
  }
  .eq.off {
    opacity: 0.4;
  }
  .band {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    color: var(--faint);
  }
  .band input {
    writing-mode: vertical-lr;
    direction: rtl;
    width: 18px;
    height: 110px;
  }
  input[type="range"] {
    accent-color: var(--text);
    margin: 0;
  }
  input[type="checkbox"] {
    accent-color: var(--text);
    margin: 0;
  }
  .hint {
    margin: 8px 0 0;
    color: var(--muted);
    font-size: 12px;
  }
</style>
