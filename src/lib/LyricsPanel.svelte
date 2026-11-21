<script>
  // Lyrics beside the list: the current line is highlighted and kept in the middle, clicking a
  // line jumps there, and the timing can be nudged for songs whose lyrics are early or late.
  import { untrack } from "svelte";
  import { player } from "./player.svelte.js";
  import { lyrics } from "./lyrics.svelte.js";

  let box = $state();
  let userScrolledAt = 0;

  // Only a song change loads lyrics (load reads and writes lyrics state itself).
  $effect(() => {
    const track = player.current;
    untrack(() => lyrics.load(track));
  });

  // Keep the current line in the middle, unless the user scrolled in the last few seconds.
  $effect(() => {
    const i = lyrics.current;
    if (!box || i < 0 || Date.now() - userScrolledAt < 4000) return;
    const el = box.querySelector(`[data-i="${i}"]`);
    if (!el) return;
    const behavior = matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    box.scrollTo({ top: el.offsetTop - box.clientHeight / 2 + el.offsetHeight / 2, behavior });
  });

  // A new song starts at the top.
  $effect(() => {
    player.current?.url;
    if (box) box.scrollTop = 0;
  });

  const offsetLabel = $derived(
    lyrics.offset === 0 ? "In sync" : `${lyrics.offset > 0 ? "+" : "−"}${(Math.abs(lyrics.offset) / 1000).toFixed(1)} s`
  );
</script>

<aside>
  <div class="head">
    <h2>Lyrics</h2>
    {#if lyrics.state === "synced"}
      <div class="nudge" title="If the lyrics are early or late, move them. Kite remembers it for this song.">
        <button onclick={() => lyrics.nudge(-500)} aria-label="Lyrics later">−</button>
        <button class="value" onclick={() => lyrics.nudge(-lyrics.offset)} title="Reset">{offsetLabel}</button>
        <button onclick={() => lyrics.nudge(500)} aria-label="Lyrics earlier">+</button>
      </div>
    {/if}
  </div>

  <div class="scroll" bind:this={box} onwheel={() => (userScrolledAt = Date.now())}>
    {#if !player.current}
      <p class="empty">Play a song to see its lyrics.</p>
    {:else if lyrics.state === "loading"}
      <p class="empty">Looking for lyrics…</p>
    {:else if lyrics.state === "synced"}
      <div class="lines">
        {#each lyrics.lines as line, i}
          <button
            data-i={i}
            class="line"
            class:now={i === lyrics.current}
            class:past={i < lyrics.current}
            class:gap={!line.text}
            onclick={() => lyrics.seekTo(i)}
          >
            {line.text || "♪"}
          </button>
        {/each}
      </div>
    {:else if lyrics.state === "plain"}
      <p class="note">These lyrics aren't synced to the song.</p>
      <p class="plain">{lyrics.plain}</p>
    {:else if lyrics.state === "instrumental"}
      <p class="empty">Instrumental</p>
    {:else if lyrics.state === "error"}
      <p class="empty">{lyrics.error}</p>
      <button class="retry" onclick={() => lyrics.load(player.current)}>Try again</button>
    {:else}
      <p class="empty">No lyrics found for this song.</p>
    {/if}
  </div>

  {#if lyrics.source === "lrclib" && (lyrics.state === "synced" || lyrics.state === "plain")}
    <p class="credit">Lyrics from LRCLIB</p>
  {/if}
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--border);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 34px;
    padding: 4px 16px 8px;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .nudge {
    display: flex;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 8px;
    font-size: 12px;
  }
  .nudge button {
    height: 24px;
    min-width: 24px;
    padding: 0 6px;
    color: var(--muted);
  }
  .nudge button:hover {
    color: var(--text);
  }
  .nudge .value {
    min-width: 56px;
    font-variant-numeric: tabular-nums;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 12px 40vh;
    position: relative;
  }
  .lines {
    display: flex;
    flex-direction: column;
    padding-top: 8px;
  }
  .line {
    text-align: left;
    padding: 5px 8px;
    border-radius: 8px;
    font-size: 17px;
    font-weight: 600;
    line-height: 1.35;
    color: var(--faint);
    transition: color 0.25s;
  }
  .line:hover {
    background: var(--hover);
  }
  .line.past {
    color: var(--muted);
  }
  .line.now {
    color: var(--text);
  }
  .line.gap {
    font-size: 14px;
  }
  .plain {
    margin: 0 8px;
    white-space: pre-wrap;
    font-size: 15px;
    line-height: 1.6;
    color: var(--text);
    user-select: text;
  }
  .note,
  .credit {
    margin: 4px 8px 12px;
    color: var(--faint);
    font-size: 12px;
  }
  .credit {
    margin: 0;
    padding: 6px 20px 8px;
    border-top: 1px solid var(--border);
  }
  .empty {
    margin: 12px 8px;
    color: var(--faint);
    font-size: 13px;
  }
  .retry {
    margin: 0 8px;
    color: var(--text);
    text-decoration: underline;
    text-underline-offset: 2px;
    font-size: 13px;
  }
  @media (prefers-reduced-motion: reduce) {
    .line {
      transition: none;
    }
  }
</style>
