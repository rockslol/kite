<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";
  import TrackList from "../lib/TrackList.svelte";
  import { player } from "../lib/player.svelte.js";
  import { library } from "../lib/library.svelte.js";
  import { menu, nav, toasts } from "../lib/ui.svelte.js";
  import { artSrc, fmtTotal, plural } from "../lib/urls.js";

  let { id } = $props();
  // Chart and other YouTube Music playlists (VL… ids) open here too.
  let playlist = $derived(id.startsWith("VL"));

  let al = $state(null);
  let error = $state("");
  let warmed = null;
  onDestroy(() => player.stopWarming(warmed));

  $effect(() => {
    const want = id;
    al = null;
    error = "";
    invoke("ytm_album", { id: want })
      .then((r) => {
        if (want !== id) return;
        al = r;
        player.warmList((warmed = al.tracks));
      })
      .catch((e) => want === id && (error = String(e)));
  });

  /** Keep the album: as a new playlist named after it, or added to one you have. */
  function save(e) {
    menu.open(e, [
      {
        label: `New playlist "${al.title}"`,
        action: async () => {
          const pid = await library.createPlaylist(al.title, al.tracks);
          toasts.show(`Saved ${al.title} as a playlist`);
          nav.go({ name: "playlist", id: pid });
        },
      },
      ...library.playlists.map((p) => ({ label: `Add to ${p.name}`, action: () => library.addToPlaylist(p.id, al.tracks) })),
    ]);
  }
</script>

<div class="view">
  {#if error}
    <p class="empty error">{error}</p>
  {:else if !al}
    <p class="empty">Loading…</p>
  {:else}
    <header class="hero">
      <div class="cover">{#if al.artwork}<img src={artSrc(al.artwork)} alt="" />{/if}</div>
      <div class="what">
        <p class="kind">{[al.kind, al.year].filter(Boolean).join(" · ")}</p>
        <h1>{al.title}</h1>
        <p class="sub">
          {#if al.artist_id}
            <button class="artist" onclick={() => nav.go({ name: "artist", id: al.artist_id })}>{al.artist}</button> ·
          {:else if al.artist}
            {al.artist} ·
          {/if}
          {plural(al.tracks.length, "song")}{al.tracks.length ? ` · ${fmtTotal(al.tracks)}` : ""}
        </p>
        <div class="actions">
          {#if al.tracks.length}
            <button class="btn primary" onclick={() => player.playList(al.tracks, 0)}>Play</button>
            <button class="btn" onclick={() => player.playShuffled(al.tracks)}>Shuffle</button>
            <button class="btn" onclick={save}>Save…</button>
          {/if}
        </div>
      </div>
    </header>
    <TrackList tracks={al.tracks} showAlbum={playlist} />
  {/if}
</div>

<style>
  .hero {
    display: flex;
    gap: 22px;
    align-items: flex-end;
    margin: 10px 8px 22px;
  }
  .cover {
    flex: none;
    width: 170px;
    height: 170px;
    border-radius: 10px;
    overflow: hidden;
    background: var(--surface);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
  }
  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .what {
    min-width: 0;
  }
  .kind {
    margin: 0 0 4px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
  }
  h1 {
    margin: 0;
    font-size: 30px;
    font-weight: 700;
    line-height: 1.15;
  }
  .artist {
    color: var(--text);
    font-weight: 500;
  }
  .artist:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .what .actions {
    margin-top: 14px;
  }
</style>
