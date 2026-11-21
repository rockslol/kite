<script>
  import { library } from "./library.svelte.js";
  import { nav, menu } from "./ui.svelte.js";
  import { importer } from "./importer.svelte.js";
  import { updates } from "./updates.svelte.js";

  const MAIN = [
    { name: "home", label: "Home" },
    { name: "search", label: "Search" },
    { name: "liked", label: "Liked songs" },
    { name: "history", label: "Recently played" },
    { name: "local", label: "Local files" },
  ];

  let dropTarget = $state(null);

  async function newPlaylist() {
    const id = await library.createPlaylist();
    nav.go({ name: "playlist", id, rename: true });
  }

  function playlistMenu(e, p) {
    menu.open(e, [
      { label: "Rename", action: () => nav.go({ name: "playlist", id: p.id, rename: true }) },
      { label: "Delete playlist", danger: true, action: () => library.deletePlaylist(p.id) },
    ]);
  }

  function dragOver(e, target) {
    if (!e.dataTransfer.types.includes("application/x-kite-tracks")) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    dropTarget = target;
  }
  async function drop(e, target) {
    e.preventDefault();
    dropTarget = null;
    const tracks = JSON.parse(e.dataTransfer.getData("application/x-kite-tracks") || "[]");
    if (!tracks.length) return;
    if (target === "liked") {
      for (const t of tracks) if (!library.isLiked(t)) await library.toggleLike(t);
    } else if (target === "new") {
      const id = await library.createPlaylist(tracks[0].title, tracks);
      nav.go({ name: "playlist", id });
    } else {
      library.addToPlaylist(target, tracks);
    }
  }
</script>

<nav>
  <div class="group">
    {#each MAIN as item}
      <button
        class="item"
        class:on={nav.is(item.name)}
        class:drop={dropTarget === item.name}
        onclick={() => nav.go({ name: item.name })}
        ondragover={item.name === "liked" ? (e) => dragOver(e, "liked") : undefined}
        ondragleave={() => (dropTarget = null)}
        ondrop={item.name === "liked" ? (e) => drop(e, "liked") : undefined}
      >
        {item.label}
        {#if item.name === "local" && (library.scan.state === "started" || library.scan.state === "progress")}
          <span class="spinner"></span>
        {/if}
      </button>
    {/each}
  </div>

  <div class="head">
    <span>Playlists</span>
    <button
      class="add"
      onclick={(e) =>
        menu.open(e, [
          { label: "New playlist", action: newPlaylist },
          { label: "Import from a link…", action: () => (importer.dialogOpen = true) },
        ])}
      aria-label="New playlist"
      title="New or import"
    >
      <svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14" /></svg>
    </button>
  </div>
  <div class="group scroll">
    {#each library.playlists as p (p.id)}
      <button
        class="item"
        class:on={nav.is("playlist", p.id)}
        class:drop={dropTarget === p.id}
        onclick={() => nav.go({ name: "playlist", id: p.id })}
        oncontextmenu={(e) => playlistMenu(e, p)}
        ondragover={(e) => dragOver(e, p.id)}
        ondragleave={() => (dropTarget = null)}
        ondrop={(e) => drop(e, p.id)}
      >
        <span class="name">{p.name}</span>
      </button>
    {:else}
      <button
        class="item hint"
        class:drop={dropTarget === "new"}
        onclick={newPlaylist}
        ondragover={(e) => dragOver(e, "new")}
        ondragleave={() => (dropTarget = null)}
        ondrop={(e) => drop(e, "new")}
      >
        Create a playlist, drop a song here, or import one from Spotify, Deezer or YouTube
      </button>
    {/each}
  </div>

  <div class="bottom">
    <button class="item" class:on={nav.is("settings")} onclick={() => nav.go({ name: "settings" })}>
      Settings
      {#if updates.available}<span class="dot" title="Update available"></span>{/if}
    </button>
  </div>
</nav>

<style>
  nav {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 4px 8px 12px 12px;
    border-right: 1px solid var(--border);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .scroll {
    overflow-y: auto;
    min-height: 0;
  }
  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: 34px;
    padding: 0 10px;
    border-radius: 8px;
    color: var(--muted);
    text-align: left;
    font-weight: 500;
  }
  .item:hover {
    color: var(--text);
    background: var(--hover);
  }
  .item.on {
    color: var(--text);
    background: var(--accent-soft);
  }
  .item.drop {
    color: var(--text);
    box-shadow: inset 0 0 0 1px var(--text);
  }
  .item.hint {
    height: auto;
    padding: 8px 10px;
    font-weight: 400;
    font-size: 13px;
    color: var(--faint);
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 18px 0 6px;
    padding: 0 4px 0 10px;
    color: var(--faint);
    font-size: 12px;
    font-weight: 500;
  }
  .add {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    color: var(--muted);
  }
  .add:hover {
    color: var(--text);
    background: var(--hover);
  }
  .add svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }
  .bottom {
    margin-top: auto;
    padding-top: 8px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text);
  }
  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-top-color: var(--text);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
