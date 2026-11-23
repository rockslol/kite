<script>
  import { player } from "./player.svelte.js";
  import { menu } from "./ui.svelte.js";
  import { artSrc, fmtTotal, plural } from "./urls.js";

  /** Played songs shown before "Show all". */
  const SHOW_PLAYED = 3;

  let dragKey = $state(null);
  let dropAt = $state(-1);
  let allPlayed = $state(false);

  let shownPlayed = $derived(allPlayed ? player.played : player.played.slice(-SHOW_PLAYED));
  // Songs in the queue from the current one on, and how long they still play.
  let summary = $derived.by(() => {
    const n = player.upcoming.length + (player.current ? 1 : 0);
    if (!n) return "";
    const left = player.timeLeft;
    return `${plural(n, "song")}${left >= 30 ? ` · ${fmtTotal([{ duration: left }])} left` : ""}`;
  });

  function onDragStart(e, key) {
    dragKey = key;
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", key);
  }
  function onDragOver(e, i) {
    if (!dragKey) return;
    e.preventDefault();
    const r = e.currentTarget.getBoundingClientRect();
    dropAt = e.clientY < r.top + r.height / 2 ? i : i + 1;
  }
  function onDrop(e) {
    e.preventDefault();
    if (dragKey && dropAt >= 0) {
      const from = player.upcoming.findIndex((t) => t.key === dragKey);
      player.moveUpcoming(dragKey, from >= 0 && from < dropAt ? dropAt - 1 : dropAt);
    }
    dragKey = null;
    dropAt = -1;
  }
  function onDragEnd() {
    dragKey = null;
    dropAt = -1;
  }
</script>

<aside>
  <div class="head">
    <div class="titles">
      <h2>Queue</h2>
      {#if summary}<p class="summary">{summary}</p>{/if}
    </div>
    <div class="actions">
      {#if player.duplicates.length}
        <button class="text" onclick={() => player.removeDuplicates()} title="Take out songs that are already in the queue">
          Remove duplicates
        </button>
      {/if}
      {#if player.upcoming.length}
        <button class="text" onclick={() => player.clearUpcoming()}>Clear</button>
      {/if}
    </div>
  </div>

  <div class="scroll">
    {#if player.played.length}
      <div class="label-row">
        <p class="label">Played</p>
        {#if player.played.length > SHOW_PLAYED}
          <button class="text small" onclick={() => (allPlayed = !allPlayed)}>
            {allPlayed ? "Show less" : `Show all ${player.played.length}`}
          </button>
        {/if}
      </div>
      <div class="played">
        {#each shownPlayed as t (t.key)}
          {@render item(t, -1, false, true)}
        {/each}
      </div>
    {/if}

    {#if player.current}
      <p class="label">Now playing</p>
      {@render item(player.current, -1, true)}
    {/if}

    {#if player.upcoming.length}
      <p class="label">Next up</p>
      <ul ondrop={onDrop} ondragover={(e) => dragKey && e.preventDefault()}>
        {#each player.upcoming as t, i (t.key)}
          <li
            draggable="true"
            class:dragging={dragKey === t.key}
            class:drop-before={dropAt === i}
            class:drop-after={dropAt === i + 1 && i === player.upcoming.length - 1}
            ondragstart={(e) => onDragStart(e, t.key)}
            ondragover={(e) => onDragOver(e, i)}
            ondragend={onDragEnd}
          >
            {@render item(t, i, false)}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty">Nothing up next. Right-click a song to add it.</p>
    {/if}
  </div>
</aside>

{#snippet item(t, i, isCurrent, wasPlayed = false)}
  <div
    class="item"
    class:current={isCurrent}
    class:was-played={wasPlayed}
    title={wasPlayed ? "Double-click to play it again" : undefined}
    role="button"
    tabindex="0"
    ondblclick={() => !isCurrent && player.jump(t.key)}
    onkeydown={(e) => e.key === "Enter" && !isCurrent && player.jump(t.key)}
    oncontextmenu={(e) => {
      e.preventDefault();
      menu.open(e, [
        ...(isCurrent ? [] : [{ label: wasPlayed ? "Play again" : "Play now", action: () => player.jump(t.key) }]),
        { label: "Remove from queue", action: () => player.remove(t.key) },
      ]);
    }}
  >
    <span class="art">{#if t.artwork}<img src={artSrc(t.artwork)} alt="" loading="lazy" draggable="false" />{/if}</span>
    <span class="meta">
      <span class="title">{t.title}</span>
      <span class="artist">{t.artist}</span>
    </span>
    {#if !isCurrent}
      <button class="x" aria-label="Remove" onclick={(e) => (e.stopPropagation(), player.remove(t.key))}>
        <svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7L7 17" /></svg>
      </button>
    {/if}
  </div>
{/snippet}

<style>
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--border);
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 16px 8px;
  }
  .titles {
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .summary {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    gap: 12px;
    flex-shrink: 0;
    padding-top: 1px;
  }
  .label-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding-right: 8px;
  }
  .text.small {
    font-size: 12px;
  }
  .item.was-played {
    opacity: 0.6;
  }
  .item.was-played:hover,
  .item.was-played:focus-visible {
    opacity: 1;
  }
  .text {
    color: var(--muted);
    font-size: 13px;
  }
  .text:hover {
    color: var(--text);
  }
  .scroll {
    overflow-y: auto;
    padding: 0 8px 12px;
  }
  .label {
    margin: 10px 8px 4px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
  }
  .empty {
    margin: 12px 8px;
    color: var(--faint);
    font-size: 13px;
  }
  ul {
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
  .item {
    display: grid;
    grid-template-columns: 36px 1fr auto;
    gap: 10px;
    align-items: center;
    padding: 5px 8px;
    border-radius: 8px;
    cursor: default;
    outline: none;
  }
  .item:hover,
  .item:focus-visible {
    background: var(--hover);
  }
  .item.current {
    background: var(--accent-soft);
  }
  .art {
    width: 36px;
    height: 36px;
    border-radius: 5px;
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
    font-size: 13px;
    font-weight: 500;
  }
  .artist {
    font-size: 12px;
    color: var(--muted);
  }
  .x {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    color: var(--muted);
    visibility: hidden;
  }
  .item:hover .x {
    visibility: visible;
  }
  .x:hover {
    color: var(--text);
    background: var(--hover);
  }
  .x svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }
</style>
