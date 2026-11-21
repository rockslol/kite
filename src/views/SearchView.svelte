<script>
  import TrackList from "../lib/TrackList.svelte";
  import { search } from "../lib/search.svelte.js";
  import { player } from "../lib/player.svelte.js";
</script>

<div class="view">
  {#if search.libraryHits.length && search.searchedFor}
    <h2 class="section">In your library</h2>
    <TrackList tracks={search.libraryHits} />
    <h2 class="section">{{ ytmusic: "YouTube Music", youtube: "YouTube", soundcloud: "SoundCloud" }[search.source]}</h2>
  {/if}

  {#if search.error}
    <p class="empty error">{search.error}</p>
  {:else if search.results.length}
    <div class="bar">
      <button class="btn primary" onclick={() => player.playList(search.results, 0)}>Play all</button>
      <button class="btn" onclick={() => player.playShuffled(search.results)}>Shuffle</button>
    </div>

<style>
  .bar {
    display: flex;
    gap: 8px;
    margin: 4px 8px 10px;
  }
</style>
    <TrackList tracks={search.results} />
  {:else if search.searchedFor && !search.searching}
    <p class="empty">Nothing found for "{search.searchedFor}".</p>
  {:else if !search.searching}
    <p class="empty">Search for a song to start.</p>
  {/if}
</div>
