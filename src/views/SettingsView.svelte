<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { library } from "../lib/library.svelte.js";
  import { nav, menu, toasts } from "../lib/ui.svelte.js";
  import { updates } from "../lib/updates.svelte.js";

  let s = $state(null);
  let tools = $state({ ytdlp_version: "", js_runtime: "" });
  let cacheBytes = $state(null);
  let updatingTools = $state(false);

  onMount(async () => {
    s = await invoke("settings_get");
    tools = await invoke("tools_info");
    cacheBytes = await invoke("cache_size");
  });

  async function save(patch) {
    s = { ...s, ...patch };
    await invoke("settings_set", { settings: { close_to_tray: s.close_to_tray, check_updates: s.check_updates } }).catch((e) =>
      toasts.show(String(e))
    );
  }

  async function setAutostart(on) {
    try {
      s.autostart = await invoke("autostart_set", { enabled: on });
    } catch (e) {
      toasts.show(String(e));
    }
  }

  async function updateTools() {
    updatingTools = true;
    try {
      const v = await invoke("tools_update");
      toasts.show(v === tools.ytdlp_version ? "yt-dlp is up to date" : `yt-dlp updated to ${v}`);
      tools = await invoke("tools_info");
    } catch (e) {
      toasts.show(String(e));
    }
    updatingTools = false;
  }

  async function clearCache() {
    cacheBytes = await invoke("cache_clear");
    toasts.show("Cache cleared");
  }

  function clearHistory(e) {
    menu.open(e, [
      {
        label: "Clear all listening history",
        danger: true,
        action: async () => {
          await invoke("lib_clear_history");
          toasts.show("History cleared");
        },
      },
      { label: "Cancel", action: () => {} },
    ]);
  }

  let moving = $state(false);
  const FILTER = [{ name: "Kite library", extensions: ["json"] }];

  async function exportLibrary() {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const day = new Date().toISOString().slice(0, 10);
    const path = await save({ title: "Export your library", defaultPath: `Kite library ${day}.json`, filters: FILTER });
    if (!path) return;
    moving = true;
    try {
      const r = await invoke("lib_export", { path });
      toasts.show(`Exported ${r.songs} songs and ${r.playlists} playlists`);
    } catch (e) {
      toasts.show(String(e));
    }
    moving = false;
  }

  async function importLibrary() {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ title: "Import a library", filters: FILTER });
    if (!path) return;
    moving = true;
    try {
      const r = await invoke("lib_import", { path });
      const parts = [
        r.playlists && `${r.playlists} playlists`,
        r.playlist_songs && `${r.playlist_songs} playlist songs`,
        r.likes && `${r.likes} liked songs`,
        r.plays && `${r.plays} plays`,
        r.folders && `${r.folders} music folders`,
      ].filter(Boolean);
      toasts.show(parts.length ? `Imported ${parts.join(", ")}` : "Nothing new to import, your library already has all of it");
    } catch (e) {
      toasts.show(String(e));
    }
    moving = false;
  }

  async function copyDebugInfo() {
    const text = await invoke("debug_info").catch((e) => String(e));
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Older webviews: copy through a hidden text box.
      const box = Object.assign(document.createElement("textarea"), { value: text });
      document.body.append(box);
      box.select();
      document.execCommand("copy");
      box.remove();
    }
    toasts.show("Debug info copied. Paste it into your bug report.");
  }

  function size(b) {
    if (b === null) return "…";
    if (b < 1024 * 1024) return `${Math.round(b / 1024)} KB`;
    if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
    return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  const RUNTIME_NOTE = {
    Deno: "Using Deno from your PC.",
    "Node.js": "Using Node.js from your PC.",
    QuickJS: "Using QuickJS, downloaded with Kite. Installing Node.js or Deno makes songs start a bit faster.",
    none: "No JavaScript runtime found. Some YouTube songs may not play.",
  };
</script>

{#snippet toggle(on, onchange, label)}
  <button class="switch" class:on role="switch" aria-checked={on} aria-label={label} onclick={() => onchange(!on)}><span></span></button>
{/snippet}

<div class="view settings">
  <header class="view-head"><h1>Settings</h1></header>

  {#if s}
    <section>
      <h2>General</h2>
      <div class="row">
        <div><p class="label">Start with Windows</p><p class="hint">Kite starts in the tray when you sign in.</p></div>
        {@render toggle(s.autostart, setAutostart, "Start with Windows")}
      </div>
      <div class="row">
        <div><p class="label">Keep playing when the window is closed</p><p class="hint">Closing Kite hides it to the tray. Quit from the tray icon.</p></div>
        {@render toggle(s.close_to_tray, (v) => save({ close_to_tray: v }), "Keep playing when closed")}
      </div>
    </section>

    <section>
      <h2>Library</h2>
      <div class="row">
        <div>
          <p class="label">Music folders</p>
          <p class="hint">{library.folders.length ? library.folders.join(", ") : "No folders added yet."}</p>
        </div>
        <button class="btn" onclick={() => nav.go({ name: "local" })}>Manage</button>
      </div>
      <div class="row">
        <div><p class="label">Listening history</p><p class="hint">Used for Recently played and Most played.</p></div>
        <button class="btn" onclick={clearHistory}>Clear</button>
      </div>
      <div class="row">
        <div>
          <p class="label">Back up or move your library</p>
          <p class="hint">Liked songs, playlists, history and music folders in one file. Importing adds what's missing and keeps what you have.</p>
        </div>
        <div class="pair">
          <button class="btn" onclick={importLibrary} disabled={moving}>Import…</button>
          <button class="btn" onclick={exportLibrary} disabled={moving}>Export…</button>
        </div>
      </div>
    </section>

    <section>
      <h2>Storage</h2>
      <div class="row">
        <div><p class="label">Cache</p><p class="hint">Covers and downloaded audio. {size(cacheBytes)} used.</p></div>
        <button class="btn" onclick={clearCache} disabled={!cacheBytes}>Clear cache</button>
      </div>
    </section>

    <section>
      <h2>Streaming</h2>
      <div class="row">
        <div>
          <p class="label">yt-dlp <span class="value">{tools.ytdlp_version || "…"}</span></p>
          <p class="hint">Finds the audio for YouTube and SoundCloud. It updates itself once a day.</p>
        </div>
        <button class="btn" onclick={updateTools} disabled={updatingTools}>{updatingTools ? "Updating…" : "Update now"}</button>
      </div>
      <div class="row">
        <div>
          <p class="label">JavaScript runtime <span class="value">{tools.js_runtime || "…"}</span></p>
          <p class="hint">{RUNTIME_NOTE[tools.js_runtime] ?? "YouTube needs one to play some songs."}</p>
        </div>
      </div>
    </section>

    <section>
      <h2>About</h2>
      <div class="row">
        <div>
          <p class="label">Kite <span class="value">{s.version}</span></p>
          <p class="hint">
            {#if updates.available}
              Version {updates.available} is ready to install.
            {:else if updates.checked}
              You have the latest version.
            {:else}
              Free and open source, MIT license.
            {/if}
          </p>
        </div>
        {#if updates.available}
          <button class="btn primary" onclick={() => updates.install()} disabled={updates.installing}>
            {updates.installing ? "Installing…" : "Install and restart"}
          </button>
        {:else}
          <button class="btn" onclick={() => updates.check(true)} disabled={updates.checking}>
            {updates.checking ? "Checking…" : "Check for updates"}
          </button>
        {/if}
      </div>
      <div class="row">
        <div><p class="label">Check for updates automatically</p><p class="hint">Kite looks once, shortly after it starts.</p></div>
        {@render toggle(s.check_updates, (v) => save({ check_updates: v }), "Check for updates automatically")}
      </div>
      <div class="row">
        <div>
          <p class="label">Debug info</p>
          <p class="hint">Your Kite and yt-dlp versions, library size and recent problems, for a bug report. Nothing is sent anywhere.</p>
        </div>
        <button class="btn" onclick={copyDebugInfo}>Copy debug info</button>
      </div>
    </section>
  {/if}
</div>

<style>
  .settings {
    max-width: 720px;
  }
  section {
    margin: 0 8px 22px;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
  }
  .row:last-child {
    border-bottom: 0;
  }
  .label {
    margin: 0;
    font-weight: 500;
  }
  .value {
    margin-left: 6px;
    color: var(--muted);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
  .hint {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: 13px;
    word-break: break-word;
  }
  .pair {
    display: flex;
    gap: 8px;
    flex: none;
  }
  .btn {
    flex: none;
  }
  .switch {
    flex: none;
    position: relative;
    width: 40px;
    height: 24px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.16);
    transition: background 0.15s;
  }
  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.15s;
  }
  .switch.on {
    background: var(--grad);
  }
  .switch.on span {
    background: var(--on-accent);
    transform: translateX(16px);
  }
</style>
