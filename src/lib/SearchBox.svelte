<script>
  // The search field in the header. Focused and empty it lists recent searches; while typing
  // it suggests finished queries from YouTube Music and, on the YouTube Music tab, the top
  // matching artists, albums and songs. Arrow keys move through the list, Enter picks.
  import { invoke } from "@tauri-apps/api/core";
  import { search } from "./search.svelte.js";
  import { isMusicLink } from "./importer.svelte.js";
  import { player } from "./player.svelte.js";
  import { nav } from "./ui.svelte.js";
  import { artSrc } from "./urls.js";

  let input = $state();
  let open = $state(false);
  let active = $state(-1);
  let found = $state({ for: "", queries: [], cards: [], songs: [] });
  let timer;
  let token = 0;

  export function focus() {
    input?.focus();
    input?.select();
  }

  // What the list shows, flattened so the arrow keys can walk it.
  let items = $derived.by(() => {
    const q = search.query.trim();
    if (!q) return search.recent.map((text) => ({ kind: "recent", text }));
    if (isMusicLink(q) || found.for !== q) return [];
    const rich = search.source === "ytmusic";
    return [
      ...found.queries.slice(0, 6).map((text) => ({ kind: "query", text })),
      ...(rich ? found.cards.slice(0, 2).map((card) => ({ kind: "card", card })) : []),
      ...(rich ? found.songs.slice(0, 2).map((song) => ({ kind: "song", song })) : []),
    ];
  });

  function typed() {
    open = true;
    active = -1;
    clearTimeout(timer);
    const q = search.query.trim();
    if (!q || isMusicLink(q)) return;
    timer = setTimeout(async () => {
      const t = ++token;
      const r = await invoke("ytm_suggestions", { query: q }).catch(() => null);
      if (t === token && r) found = { for: q, ...r };
    }, 120);
  }

  function close() {
    open = false;
    active = -1;
    clearTimeout(timer);
    token++;
  }

  function run() {
    close();
    nav.go({ name: "search" });
    search.run();
  }

  function pick(item) {
    if (item.kind === "recent" || item.kind === "query") {
      search.query = item.text;
      run();
    } else if (item.kind === "card") {
      close();
      input.blur();
      nav.go({ name: item.card.id.startsWith("UC") ? "artist" : "album", id: item.card.id });
    } else if (item.kind === "song") {
      close();
      input.blur();
      player.playList([item.song], 0);
    }
  }

  function key(e) {
    if (e.key === "ArrowDown" && items.length) {
      e.preventDefault();
      open = true;
      active = (active + 1) % items.length;
    } else if (e.key === "ArrowUp" && items.length) {
      e.preventDefault();
      active = active <= 0 ? items.length - 1 : active - 1;
    } else if (e.key === "Escape" && open && items.length) {
      e.stopPropagation();
      close();
    }
  }

  function submit(e) {
    e.preventDefault();
    if (open && active >= 0 && items[active]) pick(items[active]);
    else run();
  }
</script>

<form class="search" onsubmit={submit}>
  <svg viewBox="0 0 24 24" class="glass" aria-hidden="true">
    <circle cx="11" cy="11" r="6.5" /><path d="M16 16l4 4" />
  </svg>
  <input
    bind:this={input}
    bind:value={search.query}
    type="text"
    placeholder="Search songs, or paste a link"
    spellcheck="false"
    autocomplete="off"
    role="combobox"
    aria-expanded={open && items.length > 0}
    aria-controls="search-suggestions"
    onfocus={() => (nav.go({ name: "search" }), (open = true))}
    onblur={close}
    oninput={typed}
    onkeydown={key}
  />
  {#if search.searching}<span class="spinner"></span>{/if}

  {#if open && items.length}
    <!-- Clicks keep the focus in the field (pointerdown is cancelled), so blur doesn't close the list first. -->
    <div class="list" id="search-suggestions" role="listbox" tabindex="-1" onpointerdown={(e) => e.preventDefault()}>
      {#if items[0].kind === "recent"}
        <div class="head">
          <span>Recent searches</span>
          <button type="button" class="clear" onclick={() => search.clearRecent()}>Clear</button>
        </div>
      {/if}
      {#each items as item, i}
        <div class="item" class:on={i === active} role="option" aria-selected={i === active} tabindex="-1" onclick={() => pick(item)} onpointerenter={() => (active = i)} onkeydown={() => {}}>
          {#if item.kind === "recent" || item.kind === "query"}
            <svg viewBox="0 0 24 24" class="icon" aria-hidden="true">
              {#if item.kind === "recent"}<circle cx="12" cy="12" r="8" /><path d="M12 8v4l3 2" />{:else}<circle cx="11" cy="11" r="6.5" /><path d="M16 16l4 4" />{/if}
            </svg>
            <span class="text">{item.text}</span>
            {#if item.kind === "recent"}
              <button
                type="button"
                class="remove"
                aria-label="Remove from recent searches"
                title="Remove"
                onclick={(e) => (e.stopPropagation(), search.removeRecent(item.text))}
              >
                <svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7L7 17" /></svg>
              </button>
            {/if}
          {:else}
            {@const art = item.kind === "card" ? item.card.artwork : item.song.artwork}
            <span class="art" class:round={item.kind === "card" && item.card.id.startsWith("UC")}>
              {#if art}<img src={artSrc(art)} alt="" />{/if}
            </span>
            <span class="two">
              <span class="text">{item.kind === "card" ? item.card.title : item.song.title}</span>
              <span class="sub">
                {item.kind === "card"
                  ? item.card.id.startsWith("UC")
                    ? "Artist"
                    : item.card.subtitle
                  : `Song · ${item.song.artist}`}
              </span>
            </span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</form>

<style>
  .search {
    position: relative;
    flex: 1;
    max-width: 520px;
  }
  input {
    width: 100%;
    height: 38px;
    padding: 0 36px 0 38px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    outline: none;
    user-select: text;
  }
  input:focus {
    border-color: rgba(255, 255, 255, 0.22);
  }
  input::placeholder {
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
  .spinner {
    position: absolute;
    right: 12px;
    top: 11px;
    width: 16px;
    height: 16px;
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-top-color: var(--text);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .list {
    position: absolute;
    top: 44px;
    left: 0;
    right: 0;
    z-index: 45;
    padding: 4px;
    background: #141416;
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px 4px;
    color: var(--faint);
    font-size: 12px;
    font-weight: 500;
  }
  .clear {
    color: var(--muted);
    font-size: 12px;
  }
  .clear:hover {
    color: var(--text);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 34px;
    padding: 4px 10px;
    border-radius: 6px;
    cursor: pointer;
  }
  .item.on {
    background: var(--hover);
  }
  .icon {
    flex: none;
    width: 16px;
    height: 16px;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.8;
    stroke-linecap: round;
  }
  .text,
  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .two {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .sub {
    color: var(--muted);
    font-size: 12px;
  }
  .art {
    flex: none;
    width: 36px;
    height: 36px;
    border-radius: 5px;
    overflow: hidden;
    background: var(--surface);
  }
  .art.round {
    border-radius: 50%;
  }
  .art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .remove {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 5px;
    color: var(--faint);
    visibility: hidden;
  }
  .item:hover .remove,
  .item.on .remove {
    visibility: visible;
  }
  .remove:hover {
    color: var(--text);
    background: var(--hover);
  }
  .remove svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }
</style>
