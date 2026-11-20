<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { player } from "./lib/player.svelte.js";
  import QueuePanel from "./lib/QueuePanel.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";

  const SOURCES = [
    { id: "youtube", label: "YouTube" },
    { id: "soundcloud", label: "SoundCloud" },
  ];

  let tool = $state({ state: "checking" });

  let source = $state(localStorage.getItem("source") || "youtube");
  let query = $state("");
  let results = $state([]);
  let searching = $state(false);
  let searchError = $state("");
  let searchedFor = $state("");
  let searchInput;
  let searchToken = 0;

  let showQueue = $state(localStorage.getItem("showQueue") === "1");
  let menu = $state(null);

  let current = $derived(player.current);

  $effect(() => localStorage.setItem("source", source));
  $effect(() => localStorage.setItem("showQueue", showQueue ? "1" : "0"));

  onMount(() => {
    invoke("tool_status").then((s) => (tool = s));
    const unlisten = listen("tool-status", (e) => (tool = e.payload));
    return () => unlisten.then((f) => f());
  });

  async function search() {
    const q = query.trim();
    if (!q) return;
    const token = ++searchToken;
    searching = true;
    searchError = "";
    try {
      const found = await invoke("search", { source, query: q });
      if (token !== searchToken) return;
      results = found;
      searchedFor = q;
      player.warm(found[0]);
    } catch (e) {
      if (token === searchToken) searchError = String(e);
    } finally {
      if (token === searchToken) searching = false;
    }
  }

  function pickSource(id) {
    if (source === id) return;
    source = id;
    if (query.trim()) search();
  }

  // Resting on a song for a moment starts resolving it, so a click plays sooner.
  let hoverTimer;
  function hoverRow(t) {
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => player.warm(t), 350);
  }

  function openMenu(e, items) {
    menu = { x: e.clientX, y: e.clientY, items };
  }

  function resultMenu(e, i) {
    e.preventDefault();
    const t = results[i];
    openMenu(e, [
      { label: "Play", action: () => player.playList(results, i) },
      { label: "Play next", action: () => player.playNext(t) },
      { label: "Add to queue", action: () => player.addToQueue(t) },
    ]);
  }

  function toggle() {
    if (!current && results.length) player.playList(results, 0);
    else player.toggle();
  }

  function fmt(s) {
    if (!Number.isFinite(s) || s <= 0) return "0:00";
    s = Math.floor(s);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = String(s % 60).padStart(2, "0");
    return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
  }

  function onKey(e) {
    const typing = e.target instanceof HTMLInputElement && e.target.type === "text";
    if ((e.ctrlKey && e.key === "f") || (!typing && e.key === "/")) {
      e.preventDefault();
      searchInput?.focus();
      searchInput?.select();
    } else if (typing) {
      if (e.key === "Escape") e.target.blur();
    } else if (e.key === " ") {
      e.preventDefault();
      toggle();
    } else if (e.ctrlKey && e.key === "ArrowRight") {
      player.next();
    } else if (e.ctrlKey && e.key === "ArrowLeft") {
      player.prev();
    } else if (e.key === "ArrowRight") {
      player.seekBy(5);
    } else if (e.key === "ArrowLeft") {
      player.seekBy(-5);
    } else if (e.key === "q" && !e.ctrlKey) {
      showQueue = !showQueue;
    }
  }

  const pct = (v, max) => (max > 0 ? Math.min(100, (v / max) * 100) : 0);
</script>

<svelte:window onkeydown={onKey} oncontextmenu={(e) => e.target instanceof HTMLInputElement || e.preventDefault()} />

<div class="app">
  <header>
    <form class="search" onsubmit={(e) => (e.preventDefault(), search())}>
      <svg viewBox="0 0 24 24" class="glass" aria-hidden="true">
        <circle cx="11" cy="11" r="6.5" /><path d="M16 16l4 4" />
      </svg>
      <input
        bind:this={searchInput}
        bind:value={query}
        type="text"
        placeholder="Search songs"
        spellcheck="false"
        autocomplete="off"
      />
      {#if searching}<span class="spinner"></span>{/if}
    </form>
    <div class="segmented" role="tablist">
      {#each SOURCES as s}
        <button role="tab" aria-selected={source === s.id} class:on={source === s.id} onclick={() => pickSource(s.id)}>
          {s.label}
        </button>
      {/each}
    </div>
  </header>

  {#if tool.state === "downloading"}
    <div class="notice">Getting the stream tools ready. This only happens once.</div>
  {:else if tool.state === "error"}
    <div class="notice error">{tool.message}</div>
  {/if}

  <div class="body" class:with-queue={showQueue}>
    <main>
      {#if searchError}
        <p class="empty error">{searchError}</p>
      {:else if results.length}
        <ul class="results">
          {#each results as t, i (t.id)}
            {@const isCurrent = current && current.url === t.url}
            <li>
              <button
                class="row"
                class:current={isCurrent}
                onclick={() => player.playList(results, i)}
                oncontextmenu={(e) => resultMenu(e, i)}
                onpointerenter={() => hoverRow(t)}
                onpointerleave={() => clearTimeout(hoverTimer)}
              >
                <span class="num">
                  {#if isCurrent && player.playing}
                    <span class="bars"><i></i><i></i><i></i></span>
                  {:else}
                    {i + 1}
                  {/if}
                </span>
                <span class="art">{#if t.artwork}<img src={t.artwork} alt="" loading="lazy" />{/if}</span>
                <span class="meta">
                  <span class="title">{t.title}</span>
                  <span class="artist">{t.artist}</span>
                </span>
                <span class="dur">{#if t.preview}<span class="tag" title="SoundCloud Go+ track, only a 30 second preview plays">Preview</span>{/if}{fmt(t.duration)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {:else if searchedFor && !searching}
        <p class="empty">Nothing found for "{searchedFor}".</p>
      {:else if !searching}
        <p class="empty">Search for a song to start.</p>
      {/if}
    </main>

    {#if showQueue}
      <QueuePanel oncontext={openMenu} />
    {/if}
  </div>

  <footer class="player" class:idle={!current}>
    <div class="now">
      <span class="art big">{#if current?.artwork}<img src={current.artwork} alt="" />{/if}</span>
      <span class="meta">
        <span class="title">{current?.title ?? "Nothing playing"}</span>
        <span class="artist" class:error={player.error}>{player.error || current?.artist || ""}</span>
      </span>
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
        <button class="icon play" onclick={toggle} aria-label={player.playing ? "Pause" : "Play"}>
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
        <span class="time">{fmt(player.time)}</span>
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
        <span class="time">{fmt(player.duration)}</span>
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
      <input
        type="range"
        min="0"
        max="1"
        step="0.01"
        bind:value={player.volume}
        style="--p:{player.volume * 100}%"
      />
    </div>
  </footer>
</div>

<ContextMenu {menu} onclose={() => (menu = null)} />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  header {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 16px 20px 12px;
  }

  .search {
    position: relative;
    flex: 1;
    max-width: 560px;
  }
  .search input {
    width: 100%;
    height: 38px;
    padding: 0 36px 0 38px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    outline: none;
    user-select: text;
  }
  .search input:focus {
    border-color: rgba(255, 255, 255, 0.22);
  }
  .search input::placeholder {
    color: var(--faint);
  }
  .glass {
    position: absolute;
    left: 12px;
    top: 10px;
    width: 18px;
    height: 18px;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.8;
    stroke-linecap: round;
  }
  .search .spinner {
    position: absolute;
    right: 12px;
    top: 11px;
  }

  .segmented {
    display: flex;
    padding: 3px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  .segmented button {
    height: 30px;
    padding: 0 14px;
    border-radius: 7px;
    color: var(--muted);
    font-weight: 500;
  }
  .segmented button:hover {
    color: var(--text);
  }
  .segmented button.on {
    background: var(--active);
    color: var(--text);
  }

  .notice {
    margin: 0 20px 8px;
    padding: 10px 14px;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--muted);
  }
  .error {
    color: var(--error) !important;
  }
  .artist.error {
    white-space: normal;
    user-select: text;
    max-height: 4.2em;
    overflow-y: auto;
  }

  .body {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr;
    min-height: 0;
  }
  .body.with-queue {
    grid-template-columns: 1fr 320px;
  }

  main {
    overflow-y: auto;
    padding: 0 12px 12px;
    min-height: 0;
  }
  .empty {
    margin: 22vh 0 0;
    text-align: center;
    color: var(--muted);
  }

  .results {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: grid;
    grid-template-columns: 32px 44px 1fr auto;
    gap: 12px;
    align-items: center;
    width: 100%;
    padding: 6px 12px 6px 8px;
    border-radius: 10px;
    text-align: left;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.current {
    background: var(--active);
  }
  .num {
    text-align: center;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .art {
    display: block;
    width: 44px;
    height: 44px;
    border-radius: 6px;
    background: var(--surface);
    overflow: hidden;
  }
  .art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .art.big {
    width: 52px;
    height: 52px;
    flex: none;
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
  .dur,
  .time {
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .tag {
    margin-right: 10px;
    padding: 2px 6px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--muted);
    font-size: 11px;
  }

  .bars {
    display: inline-flex;
    gap: 2px;
    align-items: flex-end;
    height: 12px;
  }
  .bars i {
    width: 3px;
    background: var(--text);
    border-radius: 1px;
    animation: bounce 0.9s ease-in-out infinite;
  }
  .bars i:nth-child(2) {
    animation-delay: -0.3s;
  }
  .bars i:nth-child(3) {
    animation-delay: -0.6s;
  }
  @keyframes bounce {
    0%,
    100% {
      height: 4px;
    }
    50% {
      height: 12px;
    }
  }

  .player {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) minmax(340px, 1.4fr) minmax(170px, 1fr);
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
  .icon.small:hover {
    color: var(--text);
  }
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
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-top-color: var(--text);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  .spinner.dark {
    border-color: rgba(0, 0, 0, 0.2);
    border-top-color: #000;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
