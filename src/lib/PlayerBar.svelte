<script>
  import { player } from "./player.svelte.js";
  import { library } from "./library.svelte.js";
  import { songMenu } from "./menus.js";
  import { artSrc, fmtTime } from "./urls.js";

  let { showQueue = $bindable(), onplay } = $props();

  let current = $derived(player.current);
  let liked = $derived(current && library.liked.has(current.url));
  const pct = (v, max) => (max > 0 ? Math.min(100, (v / max) * 100) : 0);
</script>

<footer class="player" class:idle={!current}>
  <div class="now" oncontextmenu={(e) => current && songMenu(e, [current], 0)} role="presentation">
    <span class="art">{#if current?.artwork}<img src={artSrc(current.artwork)} alt="" />{/if}</span>
    <span class="meta">
      <span class="title">{current?.title ?? "Nothing playing"}</span>
      <span class="artist" class:error={player.error}>{player.error || current?.artist || ""}</span>
    </span>
    {#if current}
      <button
        class="icon small heart"
        class:on={liked}
        onclick={() => library.toggleLike(current)}
        aria-label={liked ? "Remove from liked songs" : "Add to liked songs"}
        title={liked ? "Remove from liked songs" : "Add to liked songs"}
      >
        <svg viewBox="0 0 24 24"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" /></svg>
      </button>
    {/if}
  </div>

  <div class="center">
    <div class="controls">
      <button class="icon small" class:on={player.shuffle} onclick={() => player.toggleShuffle()} aria-label="Shuffle" title="Shuffle">
        <svg viewBox="0 0 24 24" class="line">
          <path d="M4 17h2.5a4 4 0 0 0 3.3-1.8l4.4-6.4A4 4 0 0 1 17.5 7H20" />
          <path d="M4 7h2.5a4 4 0 0 1 3.3 1.8l.6.9" />
          <path d="M13.6 14.3l.6.9a4 4 0 0 0 3.3 1.8H20" />
          <path d="M17.5 4.5L20 7l-2.5 2.5M17.5 14.5L20 17l-2.5 2.5" />
        </svg>
      </button>
      <button class="icon" onclick={() => player.prev()} disabled={!current} aria-label="Previous">
        <svg viewBox="0 0 24 24"><path d="M7 6v12" class="stroke" /><path d="M18 6.5v11a.5.5 0 0 1-.8.4l-7.4-5.5a.5.5 0 0 1 0-.8l7.4-5.5a.5.5 0 0 1 .8.4z" /></svg>
      </button>
      <button class="icon play" onclick={onplay} aria-label={player.playing ? "Pause" : "Play"}>
        {#if player.loading && current}
          <span class="spinner dark"></span>
        {:else if player.playing}
          <svg viewBox="0 0 24 24"><rect x="7" y="6" width="3.5" height="12" rx="1" /><rect x="13.5" y="6" width="3.5" height="12" rx="1" /></svg>
        {:else}
          <svg viewBox="0 0 24 24"><path d="M8.5 6.3v11.4a.6.6 0 0 0 .9.5l9-5.7a.6.6 0 0 0 0-1L9.4 5.8a.6.6 0 0 0-.9.5z" /></svg>
        {/if}
      </button>
      <button
        class="icon"
        onclick={() => player.next()}
        disabled={!current || (player.upcoming.length === 0 && player.repeat !== "all")}
        aria-label="Next"
      >
        <svg viewBox="0 0 24 24"><path d="M17 6v12" class="stroke" /><path d="M6 6.5v11a.5.5 0 0 0 .8.4l7.4-5.5a.5.5 0 0 0 0-.8L6.8 6.1a.5.5 0 0 0-.8.4z" /></svg>
      </button>
      <button
        class="icon small"
        class:on={player.repeat !== "off"}
        onclick={() => player.cycleRepeat()}
        aria-label="Repeat"
        title={{ off: "Repeat", all: "Repeat all", one: "Repeat one" }[player.repeat]}
      >
        <svg viewBox="0 0 24 24" class="line">
          <path d="M17 3.5l3 3-3 3" />
          <path d="M4 11.5v-1a4 4 0 0 1 4-4h12" />
          <path d="M7 20.5l-3-3 3-3" />
          <path d="M20 12.5v1a4 4 0 0 1-4 4H4" />
          {#if player.repeat === "one"}<path d="M11.2 10.6l1.3-.9v4.8" class="thin" />{/if}
        </svg>
      </button>
    </div>
    <div class="seek">
      <span class="time">{fmtTime(player.time)}</span>
      <input
        type="range"
        min="0"
        max={player.duration || 0}
        step="0.1"
        value={player.time}
        disabled={!current}
        style="--p:{pct(player.time, player.duration)}%"
        oninput={(e) => player.scrub(Number(e.currentTarget.value))}
        onchange={(e) => player.seek(Number(e.currentTarget.value))}
      />
      <span class="time">{fmtTime(player.duration)}</span>
    </div>
  </div>

  <div class="right">
    <button class="icon small" class:on={showQueue} onclick={() => (showQueue = !showQueue)} aria-label="Queue" title="Queue">
      <svg viewBox="0 0 24 24">
        <path d="M4 6h13M4 11h13M4 16h7" class="stroke" />
        <path d="M14.5 14v6a.4.4 0 0 0 .6.3l4.6-3a.4.4 0 0 0 0-.6l-4.6-3a.4.4 0 0 0-.6.3z" />
      </svg>
    </button>
    <button class="icon small" onclick={() => (player.volume = player.volume > 0 ? 0 : 0.8)} aria-label="Mute">
      <svg viewBox="0 0 24 24">
        <path d="M4 9.5h3l4.2-3.6a.5.5 0 0 1 .8.4v11.4a.5.5 0 0 1-.8.4L7 14.5H4a.5.5 0 0 1-.5-.5v-4a.5.5 0 0 1 .5-.5z" />
        {#if player.volume > 0}<path d="M15.5 9a4 4 0 0 1 0 6" class="stroke" />{/if}
        {#if player.volume > 0.5}<path d="M18 6.5a7.5 7.5 0 0 1 0 11" class="stroke" />{/if}
      </svg>
    </button>
    <input type="range" min="0" max="1" step="0.01" bind:value={player.volume} style="--p:{player.volume * 100}%" />
  </div>
</footer>

<style>
  .player {
    display: grid;
    grid-template-columns: minmax(200px, 1fr) minmax(340px, 1.4fr) minmax(170px, 1fr);
    gap: 20px;
    align-items: center;
    padding: 12px 20px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  .player.idle .title {
    color: var(--muted);
  }
  .now {
    display: flex;
    gap: 12px;
    align-items: center;
    min-width: 0;
  }
  .art {
    flex: none;
    width: 52px;
    height: 52px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--surface);
  }
  .art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .title,
  .artist {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title {
    font-weight: 500;
  }
  .artist {
    color: var(--muted);
    font-size: 13px;
  }
  .artist.error {
    color: var(--error);
    white-space: normal;
    user-select: text;
    max-height: 4.2em;
    overflow-y: auto;
  }
  .heart {
    flex: none;
  }
  .heart svg {
    fill: none !important;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linejoin: round;
  }
  .heart.on svg {
    fill: currentColor !important;
  }
  .heart.on::after {
    display: none;
  }
  .center {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .controls {
    display: flex;
    justify-content: center;
    align-items: center;
    gap: 12px;
  }
  .icon {
    position: relative;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    color: var(--text);
  }
  .icon svg {
    width: 20px;
    height: 20px;
    fill: currentColor;
  }
  .icon .stroke,
  .icon svg.line path {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .icon svg.line path.thin {
    stroke-width: 1.5;
  }
  .icon:hover:not(:disabled) {
    background: var(--hover);
  }
  .icon:disabled {
    color: var(--faint);
    cursor: default;
  }
  .icon.small {
    color: var(--muted);
  }
  .icon.small:hover,
  .icon.small.on {
    color: var(--text);
  }
  .icon.small.on::after {
    content: "";
    position: absolute;
    bottom: 1px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: currentColor;
  }
  .icon.small svg {
    width: 18px;
    height: 18px;
  }
  .icon.play {
    width: 36px;
    height: 36px;
    background: var(--text);
    color: #000;
  }
  .icon.play:hover {
    background: #e6e6e6;
  }
  .seek {
    display: grid;
    grid-template-columns: 40px 1fr 40px;
    gap: 10px;
    align-items: center;
  }
  .time {
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .seek .time:first-child {
    text-align: right;
  }
  .right {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 6px;
  }
  .right input {
    width: 100px;
    margin-left: 2px;
  }
  input[type="range"] {
    -webkit-appearance: none;
    appearance: none;
    height: 14px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  input[type="range"]::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--text) var(--p, 0%), rgba(255, 255, 255, 0.16) var(--p, 0%));
  }
  input[type="range"]::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 12px;
    height: 12px;
    margin-top: -4px;
    border-radius: 50%;
    background: var(--text);
    opacity: 0;
  }
  input[type="range"]:hover::-webkit-slider-thumb,
  input[type="range"]:active::-webkit-slider-thumb {
    opacity: 1;
  }
  input[type="range"]:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .spinner {
    display: inline-block;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  .spinner.dark {
    border: 2px solid rgba(0, 0, 0, 0.2);
    border-top-color: #000;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
