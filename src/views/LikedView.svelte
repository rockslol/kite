<script>
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import TrackList from "../lib/TrackList.svelte";
  import { library } from "../lib/library.svelte.js";
  import { player } from "../lib/player.svelte.js";
  import { fmtTotal, plural } from "../lib/urls.js";

  let tracks = $state([]);
  let loaded = $state(false);

  let warmed = null; // the list handed to warmList, to stop it when this page closes
  onDestroy(() => player.stopWarming(warmed));

  $effect(() => {
    library.version; // refetch on library changes
    invoke("lib_liked").then((t) => {
      tracks = t;
      // Only on opening: refetches after a like or an edit shouldn't start more lookups.
      if (!loaded) player.warmList((warmed = tracks));
      loaded = true;
    });
  });
</script>

<div class="view">
  <header class="view-head">
    <div>
      <h1>Liked songs</h1>
      <p class="sub">{plural(tracks.length, "song")}{tracks.length ? ` · ${fmtTotal(tracks)}` : ""}</p>
    </div>
    {#if tracks.length}
      <div class="actions">
        <button class="btn primary" onclick={() => player.playList(tracks, 0)}>Play</button>
        <button class="btn" onclick={() => player.playShuffled(tracks)}>Shuffle</button>
      </div>
    {/if}
  </header>

  {#if tracks.length}
    <TrackList {tracks} />
  {:else if loaded}
    <p class="empty">Songs you like show up here. Use the heart next to any song.</p>
  {/if}
</div>
