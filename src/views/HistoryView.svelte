<script>
  import { invoke } from "@tauri-apps/api/core";
  import TrackList from "../lib/TrackList.svelte";
  import { library } from "../lib/library.svelte.js";
  import { menu } from "../lib/ui.svelte.js";
  import { plural } from "../lib/urls.js";

  const MODES = [
    { id: "recent", label: "Recent" },
    { id: "top", label: "Most played" },
  ];

  let mode = $state(localStorage.getItem("kite.historyMode") || "recent");
  let tracks = $state([]);
  let loaded = $state(false);

  $effect(() => {
    library.version;
    const order = mode;
    localStorage.setItem("kite.historyMode", order);
    invoke("lib_history", { order, limit: 200 }).then((t) => {
      if (order !== mode) return;
      tracks = t;
      loaded = true;
    });
  });

  function ago(unix) {
    const s = Date.now() / 1000 - unix;
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    const d = Math.floor(s / 86400);
    return d === 1 ? "yesterday" : d < 30 ? `${d} days ago` : new Date(unix * 1000).toLocaleDateString();
  }

  function clearMenu(e) {
    menu.open(e, [
      {
        label: "Clear all history",
        danger: true,
        action: () => invoke("lib_clear_history"),
      },
      { label: "Cancel", action: () => {} },
    ]);
  }
</script>

<div class="view">
  <header class="view-head">
    <div>
      <h1>Recently played</h1>
      <p class="sub">{plural(tracks.length, "song")}</p>
    </div>
    <div class="actions">
      <div class="segmented" role="tablist">
        {#each MODES as m}
          <button role="tab" class:on={mode === m.id} onclick={() => (mode = m.id)}>{m.label}</button>
        {/each}
      </div>
      {#if tracks.length}
        <button class="btn text" onclick={clearMenu}>Clear</button>
      {/if}
    </div>
  </header>

  {#if tracks.length}
    <TrackList {tracks} detail={(t) => (mode === "top" ? plural(t.plays, "play") : ago(t.last_played))} />
  {:else if loaded}
    <p class="empty">Songs you play for more than 30 seconds show up here.</p>
  {/if}
</div>
