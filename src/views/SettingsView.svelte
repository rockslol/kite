<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy, onMount } from "svelte";
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
    const { close_to_tray, check_updates, hotkeys_on, hotkeys, discord } = s;
    await invoke("settings_set", { settings: { close_to_tray, check_updates, hotkeys_on, hotkeys, discord } }).catch((e) =>
      toasts.show(String(e))
    );
    if ("hotkeys_on" in patch || "hotkeys" in patch) problems = await invoke("hotkeys_apply");
  }

  // ---- scrobbling
  let accounts = $state({ lastfm_available: false, lastfm_user: "", listenbrainz_user: "" });
  let lastfmBusy = $state(false);
  let lbToken = $state("");
  let lbBusy = $state(false);

  onMount(async () => {
    accounts = await invoke("scrobble_status").catch(() => accounts);
  });

  async function connectLastfm() {
    lastfmBusy = true;
    try {
      const user = await invoke("lastfm_connect");
      accounts = { ...accounts, lastfm_user: user };
      toasts.show(`Scrobbling to Last.fm as ${user}`);
    } catch (e) {
      toasts.show(String(e));
    }
    lastfmBusy = false;
  }

  async function disconnectLastfm() {
    await invoke("lastfm_disconnect").catch((e) => toasts.show(String(e)));
    accounts = { ...accounts, lastfm_user: "" };
  }

  async function connectListenbrainz(e) {
    e.preventDefault();
    if (!lbToken.trim()) return;
    lbBusy = true;
    try {
      const user = await invoke("listenbrainz_connect", { token: lbToken });
      accounts = { ...accounts, listenbrainz_user: user };
      lbToken = "";
      toasts.show(`Sending listens to ListenBrainz as ${user}`);
    } catch (err) {
      toasts.show(String(err));
    }
    lbBusy = false;
  }

  async function disconnectListenbrainz() {
    await invoke("listenbrainz_disconnect").catch((e) => toasts.show(String(e)));
    accounts = { ...accounts, listenbrainz_user: "" };
  }

  // ---- shortcuts that work everywhere
  const ACTIONS = [
    ["toggle", "Play or pause"],
    ["next", "Next song"],
    ["previous", "Previous song"],
    ["like", "Like the current song"],
    ["show", "Show Kite"],
  ];
  const MAC = navigator.userAgent.includes("Mac");
  const MODIFIERS = new Set(["Control", "Alt", "Shift", "Meta", "AltGraph", "OS"]);
  let recording = $state(null); // the action whose keys are being recorded
  let problems = $state({});

  onMount(async () => {
    problems = await invoke("hotkeys_apply").catch(() => ({}));
  });
  // Leaving Settings mid-recording mustn't leave the shortcuts switched off.
  onDestroy(() => {
    if (recording) invoke("hotkeys_apply").catch(() => {});
  });

  /** "Ctrl+Alt+ArrowRight" → "Ctrl + Alt + →" */
  function pretty(keys) {
    if (!keys) return "Not set";
    const names = { ArrowRight: "→", ArrowLeft: "←", ArrowUp: "↑", ArrowDown: "↓", Super: MAC ? "Cmd" : "Win" };
    return keys
      .split("+")
      .map((k) => names[k] ?? k.replace(/^Key/, "").replace(/^Digit/, ""))
      .join(" + ");
  }

  async function record(action) {
    recording = action;
    // Registered shortcuts would swallow the keys before the window sees them.
    await invoke("hotkeys_suspend").catch(() => {});
  }

  async function stopRecording() {
    recording = null;
    problems = await invoke("hotkeys_apply").catch(() => ({}));
  }

  function recordKey(e) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (MODIFIERS.has(e.key)) return; // wait for the actual key
    const action = recording;
    if (e.key === "Escape") return stopRecording();
    if ((e.key === "Backspace" || e.key === "Delete") && !e.ctrlKey && !e.altKey && !e.metaKey) {
      recording = null;
      return save({ hotkeys: { ...s.hotkeys, [action]: "" } });
    }
    const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
    // A plain key (or Shift + key) would fire while typing anywhere; F13-F24 and media keys are fine alone.
    const standalone = /^F(1[3-9]|2[0-4])$/.test(e.code) || e.code.startsWith("Media") || e.code.startsWith("Audio");
    if (!standalone && !mods.some((m) => m !== "Shift")) {
      problems = { ...problems, [action]: "Use Ctrl, Alt or " + (MAC ? "Cmd" : "Win") + " with the key" };
      return;
    }
    recording = null;
    save({ hotkeys: { ...s.hotkeys, [action]: [...mods, e.code].join("+") } });
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

<svelte:window onkeydowncapture={recordKey} />

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
      <h2>Sharing</h2>
      <div class="row">
        <div>
          <p class="label">Show what you're listening to on Discord</p>
          <p class="hint">Your Discord status shows the song, artist and cover, with a button to listen. Clears when you pause. Needs the Discord app running.</p>
        </div>
        {@render toggle(s.discord, (v) => save({ discord: v }), "Show on Discord")}
      </div>
      {#if accounts.lastfm_available}
        <div class="row">
          <div>
            <p class="label">Last.fm</p>
            <p class="hint">
              {#if accounts.lastfm_user}
                Scrobbling as {accounts.lastfm_user}.
              {:else if lastfmBusy}
                Approve Kite in the browser window that opened.
              {:else}
                Scrobble what you play to your Last.fm profile.
              {/if}
            </p>
          </div>
          {#if accounts.lastfm_user}
            <button class="btn" onclick={disconnectLastfm}>Disconnect</button>
          {:else}
            <button class="btn" onclick={connectLastfm} disabled={lastfmBusy}>{lastfmBusy ? "Waiting…" : "Connect"}</button>
          {/if}
        </div>
      {/if}
      <div class="row">
        <div>
          <p class="label">ListenBrainz</p>
          <p class="hint">
            {#if accounts.listenbrainz_user}
              Sending listens as {accounts.listenbrainz_user}.
            {:else}
              Send what you play to ListenBrainz. Paste your user token from
              <span class="mono">listenbrainz.org/settings</span>.
            {/if}
          </p>
        </div>
        {#if accounts.listenbrainz_user}
          <button class="btn" onclick={disconnectListenbrainz}>Disconnect</button>
        {:else}
          <form class="pair" onsubmit={connectListenbrainz}>
            <input class="token" type="text" placeholder="User token" bind:value={lbToken} spellcheck="false" autocomplete="off" />
            <button class="btn" disabled={lbBusy || !lbToken.trim()}>{lbBusy ? "Checking…" : "Connect"}</button>
          </form>
        {/if}
      </div>
    </section>

    <section>
      <h2>Keyboard shortcuts</h2>
      <div class="row">
        <div>
          <p class="label">Shortcuts that work everywhere</p>
          <p class="hint">Control Kite while you're in another app. They also work inside games, so they're off until you switch them on.</p>
        </div>
        {@render toggle(s.hotkeys_on, (v) => save({ hotkeys_on: v }), "Shortcuts that work everywhere")}
      </div>
      {#if s.hotkeys_on}
        {#each ACTIONS as [action, label]}
          <div class="row keyrow">
            <div>
              <p class="label">{label}</p>
              {#if recording === action}
                <p class="hint">Press the keys. Esc cancels, Backspace clears.</p>
              {:else if problems[action]}
                <p class="hint error">{problems[action]}</p>
              {/if}
            </div>
            <button
              class="btn keys"
              class:recording={recording === action}
              class:unset={!s.hotkeys[action]}
              onclick={() => (recording === action ? stopRecording() : record(action))}
            >
              {recording === action ? "Press keys…" : pretty(s.hotkeys[action])}
            </button>
          </div>
        {/each}
      {/if}
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
  .token {
    width: 190px;
    height: 32px;
    padding: 0 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 9px;
    outline: none;
    user-select: text;
  }
  .token:focus {
    border-color: rgba(255, 255, 255, 0.22);
  }
  .mono {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
  }
  .keys {
    min-width: 150px;
    font-variant-numeric: tabular-nums;
  }
  .keys.unset {
    color: var(--muted);
  }
  .keys.recording {
    border-color: var(--text);
  }
  .keyrow {
    padding: 8px 0;
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
