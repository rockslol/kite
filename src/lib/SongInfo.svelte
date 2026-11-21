<script>
  // "Song info" from the song menu: what Kite knows about a song (plays, likes, playlists) and,
  // for YouTube songs, what YouTube says (kind, release or upload date, views).
  import { invoke } from "@tauri-apps/api/core";
  import { songInfo, toasts } from "./ui.svelte.js";
  import { hasPages, openArtist, openAlbum } from "./discover.js";
  import { artSrc, fmtTime, plural } from "./urls.js";

  let stats = $state(null);
  let yt = $state(null);
  let ytError = $state("");

  $effect(() => {
    const t = songInfo.track;
    stats = null;
    yt = null;
    ytError = "";
    if (!t) return;
    invoke("lib_track_stats", { url: t.url })
      .then((s) => songInfo.track === t && (stats = s))
      .catch(() => {});
    if (t.source === "youtube") {
      invoke("ytm_song_info", { videoId: t.id })
        .then((r) => songInfo.track === t && (yt = r))
        .catch((e) => songInfo.track === t && (ytError = String(e)));
    }
  });

  const day = (d) => new Date(d).toLocaleDateString("en-GB", { year: "numeric", month: "short", day: "numeric" });
  const fromUnix = (s) => day(s * 1000);
  const SOURCE = { youtube: "YouTube", soundcloud: "SoundCloud", local: "Local file" };

  function go(fn) {
    const t = songInfo.track;
    songInfo.close();
    fn(t);
  }

  async function copyLink() {
    await navigator.clipboard.writeText(songInfo.track.url).catch(() => {});
    toasts.show("Link copied");
  }
</script>

{#if songInfo.track}
  {@const t = songInfo.track}
  <div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && songInfo.close()}>
    <div class="dialog" role="dialog" aria-label="Song info" tabindex="-1" onkeydown={(e) => e.key === "Escape" && songInfo.close()}>
      <div class="top">
        <div class="art">{#if t.artwork}<img src={artSrc(t.artwork)} alt="" />{/if}</div>
        <div class="names">
          <h2>{t.title}</h2>
          {#if t.artist}
            {#if hasPages(t)}
              <button class="link" onclick={() => go(openArtist)}>{t.artist}</button>
            {:else}
              <p>{t.artist}</p>
            {/if}
          {/if}
          {#if t.album}
            {#if hasPages(t)}
              <button class="link muted" onclick={() => go(openAlbum)}>{t.album}</button>
            {:else}
              <p class="muted">{t.album}</p>
            {/if}
          {/if}
        </div>
      </div>

      <dl>
        <dt>Length</dt>
        <dd>{t.duration ? fmtTime(t.duration) : "Unknown"}</dd>
        <dt>Source</dt>
        <dd>{SOURCE[t.source] ?? t.source}{yt?.kind ? `, ${yt.kind.toLowerCase()}` : ""}</dd>
        {#if yt?.published}
          <dt>{yt.kind === "Song" ? "Released" : "Uploaded"}</dt>
          <dd>{day(yt.published)}</dd>
        {/if}
        {#if yt?.views != null}
          <dt>Views on YouTube</dt>
          <dd>{yt.views.toLocaleString("en-US")}</dd>
        {/if}
        {#if yt?.channel && yt.kind !== "Song"}
          <dt>Channel</dt>
          <dd>{yt.channel}</dd>
        {/if}
        {#if ytError}
          <dt>YouTube</dt>
          <dd class="muted">{ytError}</dd>
        {:else if t.source === "youtube" && !yt}
          <dt>YouTube</dt>
          <dd class="muted">Loading…</dd>
        {/if}
        {#if stats}
          <dt>Played in Kite</dt>
          <dd>
            {stats.plays ? plural(stats.plays, "time") : "Not yet"}{stats.last_played ? `, last on ${fromUnix(stats.last_played)}` : ""}
          </dd>
          {#if stats.first_played && stats.plays > 1}
            <dt>First played</dt>
            <dd>{fromUnix(stats.first_played)}</dd>
          {/if}
          {#if stats.liked_at}
            <dt>Liked</dt>
            <dd>{fromUnix(stats.liked_at)}</dd>
          {/if}
          {#if stats.playlists.length}
            <dt>In playlists</dt>
            <dd>{stats.playlists.join(", ")}</dd>
          {/if}
          {#if stats.path}
            <dt>File</dt>
            <dd class="path">{stats.path}</dd>
          {/if}
        {/if}
      </dl>

      <div class="row">
        {#if t.source !== "local"}
          <button class="btn text" onclick={copyLink}>Copy link</button>
          <button class="btn" onclick={() => invoke("open_url", { url: t.url }).catch((e) => toasts.show(String(e)))}>
            Open on {SOURCE[t.source] ?? "the web"}
          </button>
        {/if}
        <button class="btn primary" onclick={() => songInfo.close()}>Close</button>
      </div>
    </div>
  </div>
{/if}

<svelte:window onkeydown={(e) => songInfo.track && e.key === "Escape" && songInfo.close()} />

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.6);
  }
  .dialog {
    width: 480px;
    max-height: 86vh;
    overflow-y: auto;
    padding: 20px;
    background: #0e0e10;
    border: 1px solid var(--border);
    border-radius: 14px;
    outline: none;
  }
  .top {
    display: flex;
    gap: 16px;
    align-items: center;
    margin-bottom: 16px;
  }
  .art {
    flex: none;
    width: 96px;
    height: 96px;
    border-radius: 10px;
    overflow: hidden;
    background: var(--surface);
  }
  .art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .names {
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
  }
  h2 {
    margin: 0 0 2px;
    font-size: 18px;
    font-weight: 600;
    line-height: 1.25;
    user-select: text;
  }
  .names p {
    margin: 0;
  }
  .link {
    text-align: left;
  }
  .link:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .muted {
    color: var(--muted);
  }
  dl {
    display: grid;
    grid-template-columns: 130px 1fr;
    gap: 8px 12px;
    margin: 0;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: 13px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    min-width: 0;
    user-select: text;
  }
  .path {
    word-break: break-all;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
  }
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }
</style>
