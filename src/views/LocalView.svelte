<script>
  import { invoke } from "@tauri-apps/api/core";
  import TrackList from "../lib/TrackList.svelte";
  import { library } from "../lib/library.svelte.js";
  import { player } from "../lib/player.svelte.js";
  import { filterTracks, plural } from "../lib/urls.js";

  let tracks = $state([]);
  let loaded = $state(false);
  let filter = $state("");

  $effect(() => {
    library.version;
    invoke("lib_local_tracks").then((t) => {
      tracks = t;
      loaded = true;
    });
  });

  let shown = $derived(filterTracks(tracks, filter));

  let scanning = $derived(library.scan.state === "started" || library.scan.state === "progress");
</script>

<div class="view">
  <header class="view-head">
    <div>
      <h1>Local files</h1>
      <p class="sub">
        {plural(tracks.length, "song")}
        {#if scanning}
          · scanning{library.scan.total ? ` ${library.scan.done} of ${library.scan.total}` : ""}…
        {/if}
      </p>
    </div>
    <div class="actions">
      {#if tracks.length}
        <button class="btn primary" onclick={() => player.playList(shown, 0)}>Play</button>
        <button class="btn" onclick={() => player.playShuffled(shown)}>Shuffle</button>
      {/if}
      <button class="btn" onclick={() => library.addFolder()}>Add folder</button>
    </div>
  </header>

  {#if library.folders.length}
    <div class="folders">
      {#each library.folders as f}
        <span class="folder" title={f}>
          <span class="path">{f}</span>
          <button aria-label="Remove folder" title="Remove folder" onclick={() => library.removeFolder(f)}>
            <svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7L7 17" /></svg>
          </button>
        </span>
      {/each}
      <button class="btn text" onclick={() => library.rescan()} disabled={scanning}>Rescan</button>
    </div>
  {/if}

  {#if tracks.length > 12}
    <input class="filter" type="text" placeholder="Filter local files" bind:value={filter} spellcheck="false" />
  {/if}

  {#if shown.length}
    <TrackList tracks={shown} />
  {:else if loaded && !library.folders.length}
    <p class="empty">Add a folder with music (MP3, FLAC, M4A, OGG, Opus, WAV) and it shows up here.</p>
  {:else if loaded && filter}
    <p class="empty">No local songs match "{filter}".</p>
  {:else if loaded && !scanning}
    <p class="empty">No music found in these folders.</p>
  {/if}
</div>

<style>
  .folders {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin: 0 8px 14px;
  }
  .folder {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 360px;
    height: 30px;
    padding: 0 4px 0 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--muted);
    font-size: 13px;
  }
  .path {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
  }
  .folder button {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    color: var(--muted);
    flex: none;
  }
  .folder button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .folder svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }
</style>
