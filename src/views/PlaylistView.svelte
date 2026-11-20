<script>
  import { invoke } from "@tauri-apps/api/core";
  import TrackList from "../lib/TrackList.svelte";
  import { library } from "../lib/library.svelte.js";
  import { player } from "../lib/player.svelte.js";
  import { nav, menu } from "../lib/ui.svelte.js";
  import { artSrc, fmtTotal, plural } from "../lib/urls.js";

  let { id } = $props();

  let items = $state([]);
  let loaded = $state(false);
  let editing = $state(false);
  let draft = $state("");
  let input = $state();

  let info = $derived(library.playlists.find((p) => p.id === id));

  $effect(() => {
    library.version;
    const pid = id;
    invoke("lib_playlist_items", { id: pid }).then((t) => {
      if (pid !== id) return;
      items = t;
      loaded = true;
    });
  });

  // Opened from "New playlist" or "Rename": start with the name selected.
  $effect(() => {
    if (nav.view.rename && info) {
      startRename();
      nav.view = { name: "playlist", id };
    }
  });

  function startRename() {
    draft = info?.name ?? "";
    editing = true;
    queueMicrotask(() => input?.select());
  }
  async function commitRename() {
    if (!editing) return;
    editing = false;
    const name = draft.trim();
    if (name && name !== info?.name) await library.renamePlaylist(id, name);
  }

  function move(from, to) {
    const item = items[from];
    const next = items.slice();
    next.splice(from, 1);
    next.splice(to, 0, item);
    items = next;
    invoke("lib_playlist_move", { id, itemId: item.item_id, to });
  }

  function remove(t) {
    items = items.filter((x) => x.item_id !== t.item_id);
    invoke("lib_playlist_remove", { id, itemId: t.item_id });
  }

  function deleteMenu(e) {
    menu.open(e, [
      {
        label: `Delete "${info?.name}"`,
        danger: true,
        action: async () => {
          await library.deletePlaylist(id);
          nav.go({ name: "liked" });
        },
      },
      { label: "Cancel", action: () => {} },
    ]);
  }
</script>

<div class="view">
  <header class="view-head">
    <div class="titleblock">
      <div class="cover" class:grid={(info?.covers.length ?? 0) >= 4}>
        {#if info?.covers.length >= 4}
          {#each info.covers.slice(0, 4) as c}<img src={artSrc(c)} alt="" />{/each}
        {:else if info?.covers.length}
          <img src={artSrc(info.covers[0])} alt="" />
        {/if}
      </div>
      <div class="names">
        {#if editing}
          <input
            bind:this={input}
            bind:value={draft}
            class="rename"
            spellcheck="false"
            onblur={commitRename}
            onkeydown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") {
                editing = false;
              }
            }}
          />
        {:else}
          <h1><button class="name" title="Rename" onclick={startRename}>{info?.name ?? "Playlist"}</button></h1>
        {/if}
        <p class="sub">{plural(items.length, "song")}{items.length ? ` · ${fmtTotal(items)}` : ""}</p>
      </div>
    </div>
    <div class="actions">
      {#if items.length}
        <button class="btn primary" onclick={() => player.playList(items, 0)}>Play</button>
        <button class="btn" onclick={() => player.playShuffled(items)}>Shuffle</button>
      {/if}
      <button class="btn text" onclick={deleteMenu}>Delete</button>
    </div>
  </header>

  {#if items.length}
    <TrackList
      tracks={items}
      onmove={move}
      extraMenu={(t) => [{ label: "Remove from this playlist", action: () => remove(t) }]}
    />
  {:else if loaded}
    <p class="empty">This playlist is empty. Right-click any song and pick "Add to playlist", or drag songs onto it in the sidebar.</p>
  {/if}
</div>

<style>
  .titleblock {
    flex: 1 1 340px;
    display: flex;
    gap: 16px;
    align-items: center;
    min-width: 0;
  }
  .cover {
    flex: none;
    width: 76px;
    height: 76px;
    border-radius: 10px;
    overflow: hidden;
    background: var(--surface);
    border: 1px solid var(--border);
  }
  .cover.grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
  }
  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .names {
    min-width: 0;
  }
  .name {
    font: inherit;
    text-align: left;
    max-width: 100%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rename {
    width: 360px;
    max-width: 100%;
    margin: 0;
    padding: 2px 8px;
    font-size: 24px;
    font-weight: 600;
    background: var(--surface);
    border: 1px solid rgba(255, 255, 255, 0.22);
    border-radius: 8px;
    outline: none;
    user-select: text;
  }
</style>
