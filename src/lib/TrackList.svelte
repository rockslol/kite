<script>
  // A list of songs: click plays the list from that song, right-click opens the song menu,
  // the heart likes it. Ctrl+click and Shift+click select several songs (Ctrl+A all, Esc
  // clears); the menu and dragging then act on all of them. Songs can be dragged onto a
  // playlist in the sidebar, and reordered when `onmove` is given.
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { player } from "./player.svelte.js";
  import { library } from "./library.svelte.js";
  import { songMenu, songsMenu } from "./menus.js";
  import { hasPages, openArtist, openAlbum } from "./discover.js";
  import { artSrc, fmtTime } from "./urls.js";

  let {
    tracks,
    extraMenu = null, // (tracks) => extra menu items, for one song or a selection
    onmove = null, // (index, toIndex) => void
    onhover = (t) => player.warm(t), // (track) => void; by default the song gets ready to play
    detail = null, // (track) => text shown instead of the duration
    showAlbum = true, // off on an album's own page, where every row is the same album
  } = $props();

  let dragFrom = $state(-1);
  let dropAt = $state(-1);
  let listEl = $state();

  // ---- selection (by index, so it's dropped whenever the list changes)
  const selected = new SvelteSet();
  let anchor = -1;
  $effect(() => {
    tracks;
    untrack(() => {
      selected.clear();
      anchor = -1;
    });
  });
  const chosen = () => tracks.filter((_, i) => selected.has(i));

  function click(e, i) {
    if (e.ctrlKey || e.metaKey) {
      selected.has(i) ? selected.delete(i) : selected.add(i);
      anchor = i;
    } else if (e.shiftKey) {
      const from = anchor >= 0 ? anchor : i;
      selected.clear();
      for (let k = Math.min(from, i); k <= Math.max(from, i); k++) selected.add(k);
    } else {
      selected.clear();
      anchor = i;
      player.playList(tracks, i);
    }
  }

  function key(e, i) {
    if (e.key === "Enter") player.playList(tracks, i);
    else if (e.key === "a" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      tracks.forEach((_, k) => selected.add(k));
    }
  }

  function contextMenu(e, i) {
    if (selected.has(i) && selected.size > 1) {
      const list = chosen();
      return songsMenu(e, list, extraMenu?.(list) ?? []);
    }
    selected.clear();
    anchor = i;
    songMenu(e, tracks, i, extraMenu?.([tracks[i]]) ?? []);
  }

  function outside(e) {
    if (selected.size && !listEl?.contains(e.target) && !e.target.closest?.(".menu, .toasts")) selected.clear();
  }

  function dragStart(e, i) {
    const t = tracks[i];
    const many = selected.has(i) && selected.size > 1;
    const list = many ? chosen() : [t];
    e.dataTransfer.effectAllowed = onmove && !many ? "copyMove" : "copy";
    e.dataTransfer.setData("application/x-kite-tracks", JSON.stringify(list));
    e.dataTransfer.setData("text/plain", list.map((x) => `${x.artist} - ${x.title}`).join("\n"));
    // A selection is only dragged onto playlists; reordering moves one song at a time.
    dragFrom = many ? -1 : i;
  }
  function dragOver(e, i) {
    if (!onmove || dragFrom < 0) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    const r = e.currentTarget.getBoundingClientRect();
    dropAt = e.clientY < r.top + r.height / 2 ? i : i + 1;
  }
  function drop(e) {
    if (!onmove || dragFrom < 0 || dropAt < 0) return;
    e.preventDefault();
    const to = dragFrom < dropAt ? dropAt - 1 : dropAt;
    if (to !== dragFrom) onmove(dragFrom, to);
    dragFrom = dropAt = -1;
  }
  function dragEnd() {
    dragFrom = dropAt = -1;
  }

  let hoverTimer;
  function enter(t) {
    if (!onhover) return;
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => onhover(t), 150);
  }
</script>

<svelte:window
  onpointerdown={outside}
  onkeydown={(e) => e.key === "Escape" && selected.size && selected.clear()}
/>

<ul class="list" ondrop={drop} bind:this={listEl}>
  {#each tracks as t, i (t.item_id ?? t.url + i)}
    {@const isCurrent = player.current?.url === t.url}
    {@const liked = library.liked.has(t.url)}
    <li
      class:drop-before={dropAt === i}
      class:drop-after={dropAt === i + 1 && i === tracks.length - 1}
      class:dragging={dragFrom === i && onmove}
      draggable="true"
      ondragstart={(e) => dragStart(e, i)}
      ondragover={(e) => dragOver(e, i)}
      ondragend={dragEnd}
    >
      <div
        class="row"
        class:current={isCurrent}
        class:selected={selected.has(i)}
        role="button"
        tabindex="0"
        aria-pressed={selected.has(i)}
        onclick={(e) => click(e, i)}
        onkeydown={(e) => key(e, i)}
        oncontextmenu={(e) => contextMenu(e, i)}
        onpointerenter={() => enter(t)}
        onpointerleave={() => clearTimeout(hoverTimer)}
      >
        <span class="num">
          {#if isCurrent && player.playing}
            <span class="bars"><i></i><i></i><i></i></span>
          {:else}
            {i + 1}
          {/if}
        </span>
        <span class="art">{#if t.artwork}<img src={artSrc(t.artwork)} alt="" loading="lazy" draggable="false" />{/if}</span>
        <span class="meta">
          <span class="title">{t.title}</span>
          <span class="artist">
            {#if hasPages(t)}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <span class="link" onclick={(e) => (e.stopPropagation(), openArtist(t))}>{t.artist}</span>
            {:else}
              {t.artist}
            {/if}
            {#if showAlbum && t.album && t.album !== t.title}
              <span class="album"
                >{" · "}{#if hasPages(t)}<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions --><span
                    class="link"
                    onclick={(e) => (e.stopPropagation(), openAlbum(t))}>{t.album}</span
                  >{:else}{t.album}{/if}</span
              >
            {/if}
          </span>
        </span>
        <button
          class="heart"
          class:on={liked}
          aria-label={liked ? "Remove from liked songs" : "Add to liked songs"}
          onclick={(e) => (e.stopPropagation(), library.toggleLike(t))}
        >
          <svg viewBox="0 0 24 24"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" /></svg>
        </button>
        <span class="dur">
          {#if t.preview}<span class="tag" title="SoundCloud Go+ track, only a 30 second preview plays">Preview</span>{/if}
          {detail ? detail(t) : t.duration ? fmtTime(t.duration) : ""}
        </span>
      </div>
    </li>
  {/each}
</ul>

<style>
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    border-top: 2px solid transparent;
    border-bottom: 2px solid transparent;
  }
  li.drop-before {
    border-top-color: var(--text);
  }
  li.drop-after {
    border-bottom-color: var(--text);
  }
  li.dragging {
    opacity: 0.4;
  }
  .row {
    display: grid;
    grid-template-columns: 32px 44px 1fr 32px auto;
    gap: 12px;
    align-items: center;
    padding: 6px 12px 6px 8px;
    border-radius: 10px;
    cursor: default;
    outline: none;
  }
  .row:hover,
  .row:focus-visible {
    background: var(--hover);
  }
  .row.current {
    background: var(--accent-softer);
  }
  .row.selected,
  .row.selected:hover {
    background: var(--active);
    box-shadow: inset 0 0 0 1px var(--border);
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
  .album {
    color: var(--faint);
  }
  .link {
    cursor: pointer;
  }
  .link:hover {
    color: var(--text);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .heart {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 8px;
    color: var(--muted);
    visibility: hidden;
  }
  .row:hover .heart,
  .heart.on {
    visibility: visible;
  }
  .heart:hover {
    color: var(--text);
  }
  .heart svg {
    width: 17px;
    height: 17px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linejoin: round;
  }
  .heart.on {
    color: var(--accent-text);
  }
  .heart.on svg {
    fill: currentColor;
  }
  .dur {
    min-width: 40px;
    text-align: right;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
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
  /* Animated with transform, not height: the compositor scales the bars without the page
     redoing layout and paint every frame. */
  .bars i {
    width: 3px;
    height: 12px;
    background: var(--grad);
    border-radius: 1px;
    transform-origin: bottom;
    will-change: transform;
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
      transform: scaleY(0.33);
    }
    50% {
      transform: scaleY(1);
    }
  }
</style>
