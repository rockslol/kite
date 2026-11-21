<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { player } from "./lib/player.svelte.js";
  import { search } from "./lib/search.svelte.js";
  import { library } from "./lib/library.svelte.js";
  import { nav, toasts } from "./lib/ui.svelte.js";
  import Sidebar from "./lib/Sidebar.svelte";
  import PlayerBar from "./lib/PlayerBar.svelte";
  import QueuePanel from "./lib/QueuePanel.svelte";
  import LyricsPanel from "./lib/LyricsPanel.svelte";
  import SoundPanel from "./lib/SoundPanel.svelte";
  import "./lib/sound.svelte.js"; // applies saved sound settings at startup
  import ContextMenu from "./lib/ContextMenu.svelte";
  import ImportDialog from "./lib/ImportDialog.svelte";
  import { importer } from "./lib/importer.svelte.js";
  import SearchView from "./views/SearchView.svelte";
  import LikedView from "./views/LikedView.svelte";
  import HistoryView from "./views/HistoryView.svelte";
  import LocalView from "./views/LocalView.svelte";
  import PlaylistView from "./views/PlaylistView.svelte";
  import SettingsView from "./views/SettingsView.svelte";
  import "./lib/updates.svelte.js";

  const SOURCES = [
    { id: "ytmusic", label: "YouTube Music" },
    { id: "youtube", label: "YouTube" },
    { id: "soundcloud", label: "SoundCloud" },
  ];

  let tool = $state({ state: "checking" });
  // The right-hand panel: "queue", "lyrics" or "" (closed).
  let panel = $state(localStorage.getItem("kite.panel") ?? (localStorage.getItem("showQueue") === "1" ? "queue" : ""));
  const togglePanel = (name) => (panel = panel === name ? "" : name);
  let searchInput;

  $effect(() => {
    localStorage.setItem("kite.panel", panel);
  });

  onMount(() => {
    invoke("tool_status").then((s) => (tool = s));
    const unlisten = listen("tool-status", (e) => (tool = e.payload));
    // The "like" shortcut that works everywhere.
    const unHotkey = listen("hotkey", async ({ payload }) => {
      const t = player.current;
      if (payload.action !== "like" || !t) return;
      const liked = !library.isLiked(t);
      await library.toggleLike(t);
      toasts.show(liked ? `Liked ${t.title}` : `Removed ${t.title} from liked songs`);
    });
    return () => {
      unlisten.then((f) => f());
      unHotkey.then((f) => f());
    };
  });

  function submit(e) {
    e.preventDefault();
    nav.go({ name: "search" });
    search.run();
  }

  function playPause() {
    if (!player.current && search.results.length) player.playList(search.results, 0);
    else player.toggle();
  }

  function onKey(e) {
    const typing = e.target instanceof HTMLInputElement && e.target.type === "text";
    if ((e.ctrlKey && e.key === "f") || (!typing && e.key === "/")) {
      e.preventDefault();
      searchInput?.focus();
      searchInput?.select();
    } else if (typing) {
      if (e.key === "Escape") e.target.blur();
    } else if (e.key === " ") {
      e.preventDefault();
      playPause();
    } else if (e.ctrlKey && e.key === "ArrowRight") {
      player.next();
    } else if (e.ctrlKey && e.key === "ArrowLeft") {
      player.prev();
    } else if (e.key === "ArrowRight") {
      player.seekBy(5);
    } else if (e.key === "ArrowLeft") {
      player.seekBy(-5);
    } else if (e.key === "q" && !e.ctrlKey) {
      togglePanel("queue");
    } else if (e.key === "y" && !e.ctrlKey) {
      togglePanel("lyrics");
    } else if (e.key === "e" && !e.ctrlKey) {
      togglePanel("sound");
    } else if (e.key === "l" && !e.ctrlKey && player.current) {
      library.toggleLike(player.current);
    }
  }
</script>

<svelte:window onkeydown={onKey} oncontextmenu={(e) => e.target instanceof HTMLInputElement || e.preventDefault()} />

<div class="app">
  <header>
    <div class="brand">Kite</div>
    <form class="search" onsubmit={submit}>
      <svg viewBox="0 0 24 24" class="glass" aria-hidden="true">
        <circle cx="11" cy="11" r="6.5" /><path d="M16 16l4 4" />
      </svg>
      <input
        bind:this={searchInput}
        bind:value={search.query}
        type="text"
        placeholder="Search songs, or paste a link"
        spellcheck="false"
        autocomplete="off"
        onfocus={() => nav.go({ name: "search" })}
      />
      {#if search.searching}<span class="spinner"></span>{/if}
    </form>
    <div class="segmented" role="tablist">
      {#each SOURCES as s}
        <button role="tab" aria-selected={search.source === s.id} class:on={search.source === s.id} onclick={() => (nav.go({ name: "search" }), search.pickSource(s.id))}>
          {s.label}
        </button>
      {/each}
    </div>
  </header>

  {#if tool.state === "downloading"}
    <div class="notice">Getting the stream tools ready. This only happens once.</div>
  {:else if tool.state === "error"}
    <div class="notice error">{tool.message}</div>
  {/if}

  <div class="body" class:with-queue={panel}>
    <Sidebar />
    <main>
      {#if nav.view.name === "liked"}
        <LikedView />
      {:else if nav.view.name === "history"}
        <HistoryView />
      {:else if nav.view.name === "local"}
        <LocalView />
      {:else if nav.view.name === "settings"}
        <SettingsView />
      {:else if nav.view.name === "playlist"}
        {#key nav.view.id}<PlaylistView id={nav.view.id} />{/key}
      {:else}
        <SearchView />
      {/if}
    </main>
    {#if panel === "queue"}
      <QueuePanel />
    {:else if panel === "lyrics"}
      <LyricsPanel />
    {:else if panel === "sound"}
      <SoundPanel />
    {/if}
  </div>

  <PlayerBar bind:panel onplay={playPause} />
</div>

<ContextMenu />
<ImportDialog />

{#if importer.jobs.length}
  <div class="jobs">
    {#each importer.jobs as j (j.job)}
      <div class="job">
        <span class="spinner"></span>
        <span>
          {#if j.state === "matching" && j.total}
            Finding songs on YouTube Music, {j.done} of {j.total}
          {:else}
            {j.label}
          {/if}
        </span>
      </div>
    {/each}
  </div>
{/if}

<div class="toasts" aria-live="polite">
  {#each toasts.list as t (t.id)}
    <div class="toast">
      {t.text}
      {#if t.action}
        <button onclick={() => (toasts.dismiss(t.id), t.action.run())}>{t.action.label}</button>
      {/if}
    </div>
  {/each}
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  header {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 14px 20px 12px;
  }
  .brand {
    width: 188px;
    flex: none;
    padding-left: 10px;
    font-weight: 600;
    font-size: 16px;
  }
  .search {
    position: relative;
    flex: 1;
    max-width: 520px;
  }
  .search input {
    width: 100%;
    height: 38px;
    padding: 0 36px 0 38px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    outline: none;
    user-select: text;
  }
  .search input:focus {
    border-color: rgba(255, 255, 255, 0.22);
  }
  .search input::placeholder {
    color: var(--faint);
  }
  .glass {
    position: absolute;
    left: 12px;
    top: 10px;
    width: 18px;
    height: 18px;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.8;
    stroke-linecap: round;
  }
  .search .spinner {
    position: absolute;
    right: 12px;
    top: 11px;
  }
  .notice {
    margin: 0 20px 8px;
    padding: 10px 14px;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--muted);
  }
  .body {
    flex: 1;
    display: grid;
    grid-template-columns: 210px 1fr;
    min-height: 0;
  }
  .body.with-queue {
    grid-template-columns: 210px 1fr 300px;
  }
  main {
    overflow-y: auto;
    min-height: 0;
    padding: 0 12px 12px;
  }
  .spinner {
    display: inline-block;
    width: 16px;
    height: 16px;
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
  .jobs {
    position: fixed;
    right: 16px;
    bottom: 96px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    z-index: 55;
  }
  .job {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    background: #141416;
    border: 1px solid var(--border);
    border-radius: 10px;
    font-size: 13px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.4);
  }
  .toasts {
    position: fixed;
    left: 50%;
    bottom: 96px;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: center;
    pointer-events: none;
    z-index: 60;
  }
  .toast {
    padding: 8px 14px;
    background: #f2f2f2;
    color: #000;
    border-radius: 9px;
    font-weight: 500;
    font-size: 13px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.4);
    display: flex;
    gap: 14px;
    align-items: center;
  }
  .toast button {
    pointer-events: auto;
    margin: -4px -6px -4px 0;
    padding: 4px 8px;
    border-radius: 6px;
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .toast button:hover {
    background: rgba(0, 0, 0, 0.08);
  }
</style>
