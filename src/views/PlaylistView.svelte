<script>
  import { invoke } from "@tauri-apps/api/core";
  import TrackList from "../lib/TrackList.svelte";
  import { library } from "../lib/library.svelte.js";
  import { player } from "../lib/player.svelte.js";
  import { nav, menu } from "../lib/ui.svelte.js";
  import { importer } from "../lib/importer.svelte.js";
  import { artSrc, fmtTime, fmtTotal, plural } from "../lib/urls.js";

  let { id } = $props();

  let items = $state([]);
  let loaded = $state(false);
  let editing = $state(false);
  let draft = $state("");
  let input = $state();

  let info = $derived(library.playlists.find((p) => p.id === id));
  let result = $derived(importer.results[id]);
  let showMissing = $state(false);
  let syncing = $state(false);

  async function sync() {
    syncing = true;
    await importer.sync(id, info?.name ?? "Playlist");
    syncing = false;
  }

  const SERVICE = [
    ["spotify", "Spotify"],
    ["deezer", "Deezer"],
    ["soundcloud", "SoundCloud"],
    ["youtube", "YouTube"],
    ["youtu.be", "YouTube"],
  ];
  let sourceName = $derived(info?.source_url ? (SERVICE.find(([k]) => info.source_url.includes(k))?.[1] ?? "a link") : null);

  /** "Wrong song?": swap the menu for other versions once they're found. */
  async function pickAnother(t) {
    menu.swap([{ label: "Finding other versions…", keepOpen: true, action: () => {} }]);
    let found;
    try {
      found = await invoke("match_alternatives", { playlistId: id, itemId: t.item_id });
    } catch (e) {
      menu.swap([{ label: String(e), action: () => {} }]);
      return;
    }
    if (!menu.current) return;
    menu.swap(
      found.length
        ? found.map((c) => ({
            label: `${c.title} · ${c.artist} · ${fmtTime(c.duration)}`,
            action: () => invoke("match_choose", { playlistId: id, itemId: t.item_id, track: c }),
          }))
        : [{ label: "No other versions found", action: () => {} }]
    );
  }

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
      {#if info?.source_url}
        <button class="btn" onclick={sync} disabled={syncing} title="Add songs that are new on {sourceName}">
          {syncing ? "Syncing…" : "Sync"}
        </button>
      {/if}
      <button class="btn text" onclick={deleteMenu}>Delete</button>
    </div>
  </header>

  {#if result?.missing.length || result?.truncated}
    <div class="note">
      {#if result.truncated}
        <p>Spotify only shares the first 100 songs of a playlist, so the rest aren't here.</p>
      {/if}
      {#if result.missing.length}
        <p>
          {plural(result.missing.length, "song")} couldn't be found on YouTube Music.
          <button class="link" onclick={() => (showMissing = !showMissing)}>{showMissing ? "Hide" : "Show"}</button>
        </p>
        {#if showMissing}
          <ul>
            {#each result.missing as m}<li>{m}</li>{/each}
          </ul>
        {/if}
      {/if}
    </div>
  {/if}

  {#if items.length}
    <TrackList
      tracks={items}
      onmove={move}
      extraMenu={(t) => [
        { label: "Wrong song? Pick another version", keepOpen: true, action: () => pickAnother(t) },
        { label: "Remove from this playlist", action: () => remove(t) },
      ]}
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
  .note {
    margin: -4px 8px 14px;
    padding: 10px 14px;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--muted);
    font-size: 13px;
  }
  .note p {
    margin: 0;
  }
  .note p + p {
    margin-top: 4px;
  }
  .note ul {
    margin: 8px 0 0;
    padding-left: 18px;
    max-height: 180px;
    overflow-y: auto;
    user-select: text;
  }
  .link {
    color: var(--text);
    text-decoration: underline;
    text-underline-offset: 2px;
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
