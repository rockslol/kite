<script>
  // A grid of album, single, playlist or artist cards (artists get round pictures). Click opens
  // the page, unless a card brings its own `play`. A card's `badge` ("New") shows on the cover.
  import { nav } from "./ui.svelte.js";
  import { artSrc } from "./urls.js";

  // `rows`: show only that many full rows; cards that don't fit the width are left out.
  let { cards, round = false, rows = 0 } = $props();

  // Same numbers as the grid below: columns of at least 140 px with 12 px between them.
  let width = $state(0);
  let shown = $derived(rows && width ? cards.slice(0, Math.max(1, Math.floor((width + 12) / 152)) * rows) : cards);

  // UC… artists; MPREb_… albums and VL… playlists both open on the album page.
  const open = (c) => (c.play ? c.play() : nav.go({ name: c.id.startsWith("UC") ? "artist" : "album", id: c.id }));
</script>

<div class="cards" bind:clientWidth={width}>
  {#each shown as c (c.id)}
    <button class="card" onclick={() => open(c)} title={c.title}>
      <span class="art" class:round>
        {#if c.artwork}<img src={artSrc(c.artwork)} alt="" loading="lazy" draggable="false" />{/if}
        {#if c.badge}<span class="badge">{c.badge}</span>{/if}
      </span>
      <span class="name">{c.title}</span>
      {#if c.subtitle}<span class="sub">{c.subtitle}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 14px 12px;
    margin: 0 8px 18px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
    border-radius: 12px;
    text-align: left;
    min-width: 0;
  }
  .card:hover {
    background: var(--hover);
  }
  .art {
    display: block;
    aspect-ratio: 1;
    margin-bottom: 6px;
    border-radius: 8px;
    overflow: hidden;
    background: var(--surface);
  }
  .art {
    position: relative;
  }
  .badge {
    position: absolute;
    left: 8px;
    top: 8px;
    padding: 2px 7px;
    border-radius: 6px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 11px;
    font-weight: 600;
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
  .name,
  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font-weight: 500;
  }
  .sub {
    color: var(--muted);
    font-size: 12px;
  }
</style>
