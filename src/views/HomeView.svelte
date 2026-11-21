<script module>
  // Charts change once a day: keep them between visits for an hour. Country code → { at, charts, songs }.
  const chartCache = new Map();
  const CHART_TTL = 3600e3;
  // Related songs per liked song (its url), for "Because you liked".
  const relatedCache = new Map();
</script>

<script>
  // Home: what you played lately, mixes from the artists you play most, new releases from the
  // artists you follow and play, and the charts for a country.
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";
  import TrackList from "../lib/TrackList.svelte";
  import Cards from "../lib/Cards.svelte";
  import { player } from "../lib/player.svelte.js";
  import { library } from "../lib/library.svelte.js";
  import { songMenu } from "../lib/menus.js";
  import { menu, nav } from "../lib/ui.svelte.js";
  import { artSrc } from "../lib/urls.js";

  /** Releases first seen this recently get a "New" badge. */
  const NEW_SECS = 14 * 86400;
  const MIXES = 6;

  let recent = $state([]);
  let top = $state([]);
  let releases = $state([]);
  let liked = $state([]);
  let related = $state([]);
  let loaded = $state(false);

  let country = $state(localStorage.getItem("kite.chartsCountry") || "");
  let charts = $state(null);
  let chartSongs = $state([]);
  let chartsError = $state("");

  let warmed = null;
  onDestroy(() => player.stopWarming(warmed));

  $effect(() => {
    library.version;
    const since = Math.floor(Date.now() / 1000) - 90 * 86400;
    Promise.all([
      invoke("lib_history", { order: "recent", limit: 8 }),
      invoke("lib_history", { order: "top", since, limit: 150 }),
      invoke("lib_liked"),
      invoke("releases", { limit: 18 }),
    ])
      .then(([r, t, l, rel]) => {
        liked = l;
        recent = r;
        top = t;
        releases = rel;
        if (!loaded) player.warmList((warmed = recent));
        loaded = true;
      })
      .catch(() => (loaded = true));
  });

  $effect(() => {
    const want = country;
    localStorage.setItem("kite.chartsCountry", want);
    const hit = chartCache.get(want);
    if (hit && Date.now() - hit.at < CHART_TTL) {
      charts = hit.charts;
      chartSongs = hit.songs;
      chartsError = "";
      return;
    }
    chartsError = "";
    invoke("ytm_charts", { country: want || null })
      .then(async (ch) => {
        const list = topChart(ch);
        const songs = list ? ((await invoke("ytm_album", { id: list.id }).catch(() => null))?.tracks ?? []) : [];
        if (want !== country) return;
        chartCache.set(want, { at: Date.now(), charts: ch, songs });
        charts = ch;
        chartSongs = songs;
      })
      .catch((e) => want === country && (chartsError = String(e)));
  });

  /** The chart whose songs are listed: Top 100 when there is one. */
  const topChart = (ch) => ch?.playlists.find((p) => /top 100/i.test(p.title)) ?? ch?.playlists.at(-1);

  const firstArtist = (name) => name.split(/,| & | feat| ft\.| x /i)[0].trim();

  // One mix per artist you play most, started from your most played song of theirs. Radio
  // needs a YouTube video, so local files and SoundCloud songs don't start one.
  let mixes = $derived.by(() => {
    const seen = new Set();
    const out = [];
    for (const t of top) {
      if (t.source !== "youtube" || !t.artist) continue;
      const artist = firstArtist(t.artist);
      const key = artist.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      const title = `${artist} mix`;
      out.push({
        id: `mix:${t.url}`,
        title,
        subtitle: `Starts with ${t.title}`,
        artwork: t.artwork,
        play: () => player.startRadio({ track: t, title }),
      });
      if (out.length === MIXES) break;
    }
    return out;
  });

  let releaseCards = $derived.by(() => {
    const now = Date.now() / 1000;
    return releases.map((r) => ({
      id: r.id,
      title: r.title,
      subtitle: `${r.artist} · ${r.subtitle}`,
      artwork: r.artwork,
      badge: r.first_seen && now - r.first_seen < NEW_SECS ? "New" : null,
    }));
  });

  let followCards = $derived(
    [...library.follows.values()].map((a) => ({ id: a.id, title: a.name, subtitle: "Artist", artwork: a.artwork }))
  );

  let countryName = $derived(charts?.countries.find((c) => c.code === charts.country)?.name ?? "");

  function pickCountry(e) {
    if (!charts) return;
    const shown = charts.countries.find((c) => c.code === charts.country);
    const rest = charts.countries.filter((c) => c !== shown);
    menu.open(e, [
      ...(shown ? [{ label: `${shown.name} ✓`, action: () => {} }] : []),
      ...rest.map((c) => ({ label: c.name, action: () => (country = c.code) })),
    ]);
  }

  const isPlaying = (t) => player.current?.url === t.url;

  // Two full rows of tiles (at least 220 px wide, 8 px apart); the rest wait in Recently played.
  // Tile grids show two full rows: tiles are at least 220 px wide with 8 px between, inside
  // the page's 8 px side margins.
  let viewWidth = $state(0);
  let perRow = $derived(Math.max(1, Math.floor((viewWidth - 16 + 8) / 228)));
  const twoRows = (list) => (viewWidth ? list.slice(0, perRow * 2) : list);

  // "Because you liked": one of your 10 latest liked YouTube songs, a different one each day.
  // With nothing liked yet, one of your 10 most played instead ("More like").
  let fromLikes = $derived(liked.some((t) => t.source === "youtube"));
  let seed = $derived.by(() => {
    const yt = (fromLikes ? liked : top).filter((t) => t.source === "youtube");
    if (!yt.length) return null;
    const day = Math.floor(Date.now() / 86400e3);
    return yt[day % Math.min(10, yt.length)];
  });

  $effect(() => {
    const t = seed;
    related = [];
    if (!t) return;
    const hit = relatedCache.get(t.url);
    if (hit) return void (related = hit);
    invoke("ytm_related", { videoId: t.id })
      .then((r) => {
        relatedCache.set(t.url, r.songs);
        if (seed === t) related = r.songs;
      })
      .catch(() => {});
  });
</script>

{#snippet tiles(list)}
  <div class="tiles">
    {#each twoRows(list) as t, i (t.url)}
      <button
        class="tile"
        class:playing={isPlaying(t)}
        onclick={() => player.playList(list, i)}
        oncontextmenu={(e) => songMenu(e, list, i)}
        onpointerenter={() => player.warm(t)}
        title={`${t.title} · ${t.artist}`}
      >
        <span class="art">{#if t.artwork}<img src={artSrc(t.artwork)} alt="" loading="lazy" draggable="false" />{/if}</span>
        <span class="txt">
          <span class="name">{t.title}</span>
          <span class="by">{t.artist}</span>
        </span>
      </button>
    {/each}
  </div>
{/snippet}

<div class="view" bind:clientWidth={viewWidth}>
  <div class="view-head">
    <h1>Home</h1>
  </div>

  {#if loaded && !recent.length}
    <p class="hint">Play a few songs and this page fills up with what you played, mixes made from your favourite artists, and their new releases.</p>
  {/if}

  {#if recent.length}
    <h2 class="section">Jump back in</h2>
    {@render tiles(recent)}
  {/if}

  {#if mixes.length}
    <h2 class="section">Your mixes</h2>
    <Cards cards={mixes} rows={1} />
  {/if}

  {#if seed && related.length}
    <div class="head">
      <h2 class="section">{fromLikes ? "Because you liked" : "More like"} {seed.title}</h2>
      <button class="btn text" onclick={() => player.playList(related, 0)}>Play all</button>
    </div>
    {@render tiles(related)}
  {/if}

  {#if releaseCards.length}
    <h2 class="section">New releases</h2>
    <Cards cards={releaseCards} rows={2} />
  {/if}

  {#if followCards.length}
    <h2 class="section">Artists you follow</h2>
    <Cards cards={followCards} round rows={1} />
  {/if}

  <div class="head">
    <h2 class="section">Charts{countryName ? `: ${countryName}` : ""}</h2>
    {#if charts}
      <button class="btn text" onclick={pickCountry}>Change country</button>
    {/if}
  </div>
  {#if chartsError}
    <p class="hint error">{chartsError}</p>
  {:else if !charts}
    <p class="hint">Loading charts…</p>
  {:else}
    {#if chartSongs.length}
      <TrackList tracks={chartSongs.slice(0, 10)} />
      <button class="btn text more" onclick={() => nav.go({ name: "album", id: topChart(charts).id })}>
        See all {chartSongs.length}
      </button>
    {/if}
    {#if charts.playlists.length}
      <Cards cards={charts.playlists} rows={1} />
    {/if}
    {#if charts.artists.length}
      <h2 class="section">Top artists{countryName ? ` in ${countryName}` : ""}</h2>
      <Cards cards={charts.artists.slice(0, 12)} round rows={2} />
    {/if}
  {/if}
</div>

<style>
  .hint {
    margin: 0 8px 18px;
    max-width: 560px;
    color: var(--muted);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 8px;
    margin: 0 8px 18px;
  }
  .tile {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 6px;
    border-radius: 10px;
    background: var(--hover);
    text-align: left;
  }
  .tile:hover {
    background: var(--active);
  }
  .tile .art {
    flex: none;
    width: 44px;
    height: 44px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--surface);
  }
  .tile img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .txt {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name,
  .by {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font-weight: 500;
  }
  .tile.playing .name {
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .by {
    color: var(--muted);
    font-size: 12px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-right: 8px;
  }
  .more {
    margin: 2px 0 14px;
  }
</style>
