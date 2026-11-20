<script>
  // A list of songs: click plays the list from that song, right-click opens the song menu,
  // the heart likes it. Songs can be dragged onto a playlist in the sidebar, and reordered
  // when `onmove` is given.
  import { player } from "./player.svelte.js";
  import { library } from "./library.svelte.js";
  import { songMenu } from "./menus.js";
  import { artSrc, fmtTime } from "./urls.js";

  let {
    tracks,
    extraMenu = null, // (track, index) => extra menu items
    onmove = null, // (index, toIndex) => void
    onhover = null, // (track) => void
    detail = null, // (track) => text shown instead of the duration
  } = $props();

  let dragFrom = $state(-1);
  let dropAt = $state(-1);

  function dragStart(e, i) {
    const t = tracks[i];
    e.dataTransfer.effectAllowed = onmove ? "copyMove" : "copy";
    e.dataTransfer.setData("application/x-hum-tracks", JSON.stringify([t]));
    e.dataTransfer.setData("text/plain", `${t.artist} - ${t.title}`);
    dragFrom = i;
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
    hoverTimer = setTimeout(() => onhover(t), 350);
  }
</script>

<ul class="list" ondrop={drop}>
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
        role="button"
        tabindex="0"
        onclick={() => player.playList(tracks, i)}
        onkeydown={(e) => e.key === "Enter" && player.playList(tracks, i)}
        oncontextmenu={(e) => songMenu(e, tracks, i, extraMenu?.(t, i) ?? [])}
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
          <span class="artist">{t.artist}{#if t.album && t.album !== t.title}<span class="album">{` · ${t.album}`}</span>{/if}</span>
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
          {detail ? detail(t) : fmtTime(t.duration)}
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
    color: var(--text);
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
</style>
