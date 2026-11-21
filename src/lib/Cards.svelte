<script>
  // A row of album, single or artist cards (artists get round pictures). Click opens the page.
  import { nav } from "./ui.svelte.js";
  import { artSrc } from "./urls.js";

  let { cards, round = false } = $props();

  const open = (c) => nav.go({ name: c.id.startsWith("UC") ? "artist" : "album", id: c.id });
</script>

<div class="cards">
  {#each cards as c (c.id)}
    <button class="card" onclick={() => open(c)} title={c.title}>
      <span class="art" class:round>{#if c.artwork}<img src={artSrc(c.artwork)} alt="" loading="lazy" draggable="false" />{/if}</span>
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
