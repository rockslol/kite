<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";
  import TrackList from "../lib/TrackList.svelte";
  import Cards from "../lib/Cards.svelte";
  import { player } from "../lib/player.svelte.js";
  import { library } from "../lib/library.svelte.js";
  import { artSrc } from "../lib/urls.js";

  let { id } = $props();

  let a = $state(null);
  let error = $state("");
  let allSongs = $state(null); // the full list, once "Show all" is pressed
  let loadingAll = $state(false);
  let showAbout = $state(false);
  let warmed = null;
  onDestroy(() => player.stopWarming(warmed));

  $effect(() => {
    const want = id;
    a = null;
    error = "";
    allSongs = null;
    invoke("ytm_artist", { id: want })
      .then((r) => {
        if (want !== id) return;
        a = r;
        player.warmList((warmed = a.songs));
      })
      .catch((e) => want === id && (error = String(e)));
  });

  let songs = $derived(allSongs ?? a?.songs ?? []);

  async function showAll() {
    if (!a?.all_songs) return;
    loadingAll = true;
    allSongs = await invoke("ytm_playlist_songs", { id: a.all_songs }).catch(() => null);
    loadingAll = false;
  }
</script>

<div class="view">
  {#if error}
    <p class="empty error">{error}</p>
  {:else if !a}
    <p class="empty">Loading…</p>
  {:else}
    <header class="hero">
      <div class="pic">{#if a.artwork}<img src={artSrc(a.artwork)} alt="" />{/if}</div>
      <div class="who">
        <h1>{a.name}</h1>
        {#if a.audience}<p class="sub">{a.audience}</p>{/if}
        <div class="actions">
          {#if songs.length}
            <button class="btn primary" onclick={() => player.playList(songs, 0)}>Play</button>
            <button class="btn" onclick={() => player.playShuffled(songs)}>Shuffle</button>
          {/if}
          {#if a.radio}
            <button class="btn" onclick={() => player.startRadio({ playlistId: a.radio, title: a.name })}>Radio</button>
          {/if}
          <button class="btn" class:on={library.isFollowed(a.id)} onclick={() => library.toggleFollow(a)}>
            {library.isFollowed(a.id) ? "Following" : "Follow"}
          </button>
        </div>
      </div>
    </header>

    {#if songs.length}
      <div class="head">
        <h2 class="section">{allSongs ? "Songs" : "Top songs"}</h2>
        {#if !allSongs && a.all_songs}
          <button class="btn text" onclick={showAll} disabled={loadingAll}>{loadingAll ? "Loading…" : "Show all"}</button>
        {/if}
      </div>
      <TrackList tracks={songs} />
    {/if}

    {#if a.albums.length}
      <h2 class="section">Albums</h2>
      <Cards cards={a.albums} />
    {/if}
    {#if a.singles.length}
      <h2 class="section">Singles and EPs</h2>
      <Cards cards={a.singles} />
    {/if}
    {#if a.similar.length}
      <h2 class="section">Fans also like</h2>
      <Cards cards={a.similar} round />
    {/if}
    {#if a.about}
      <h2 class="section">About</h2>
      <p class="about" class:open={showAbout}>{a.about}</p>
      {#if a.about.length > 300}
        <button class="btn text more" onclick={() => (showAbout = !showAbout)}>{showAbout ? "Less" : "More"}</button>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .hero {
    display: flex;
    gap: 22px;
    align-items: center;
    margin: 10px 8px 22px;
  }
  .pic {
    flex: none;
    width: 150px;
    height: 150px;
    border-radius: 50%;
    overflow: hidden;
    background: var(--surface);
  }
  .pic img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .who {
    min-width: 0;
  }
  h1 {
    margin: 0;
    font-size: 32px;
    font-weight: 700;
    line-height: 1.1;
  }
  .who .actions {
    margin-top: 14px;
  }
  .btn.on {
    background: var(--accent-soft);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-right: 8px;
  }
  .about {
    margin: 0 8px;
    max-width: 720px;
    color: var(--muted);
    line-height: 1.55;
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
    user-select: text;
  }
  .about.open {
    display: block;
  }
  .more {
    margin: 4px 0 12px;
  }
</style>
