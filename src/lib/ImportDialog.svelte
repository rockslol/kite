<script>
  import { importer } from "./importer.svelte.js";

  let url = $state("");
  let input = $state();

  $effect(() => {
    if (importer.dialogOpen) {
      url = "";
      queueMicrotask(() => input?.focus());
    }
  });

  function submit(e) {
    e.preventDefault();
    if (!url.trim()) return;
    importer.dialogOpen = false;
    importer.importLink(url.trim());
  }
</script>

{#if importer.dialogOpen}
  <div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && (importer.dialogOpen = false)}>
    <form class="dialog" onsubmit={submit} onkeydown={(e) => e.key === "Escape" && (importer.dialogOpen = false)}>
      <h2>Import a playlist</h2>
      <p class="hint">Paste a link to a playlist or album from Spotify, Deezer, YouTube Music, YouTube or SoundCloud.</p>
      <input bind:this={input} bind:value={url} type="text" placeholder="https://open.spotify.com/playlist/…" spellcheck="false" />
      <p class="small">Spotify and Deezer songs are matched to YouTube Music. Spotify shares the first 100 songs of a playlist.</p>
      <div class="row">
        <button type="button" class="btn text" onclick={() => (importer.dialogOpen = false)}>Cancel</button>
        <button type="submit" class="btn primary" disabled={!url.trim()}>Import</button>
      </div>
    </form>
  </div>
{/if}

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
    width: 460px;
    padding: 20px;
    background: #0e0e10;
    border: 1px solid var(--border);
    border-radius: 14px;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 17px;
    font-weight: 600;
  }
  .hint {
    margin: 0 0 14px;
    color: var(--muted);
    font-size: 13px;
  }
  input {
    width: 100%;
    height: 38px;
    padding: 0 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    outline: none;
    user-select: text;
  }
  input:focus {
    border-color: rgba(255, 255, 255, 0.22);
  }
  .small {
    margin: 10px 0 0;
    color: var(--faint);
    font-size: 12px;
  }
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
