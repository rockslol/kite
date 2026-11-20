<script>
  // The one right-click menu, driven by `menu` in ui.svelte.js.
  import { menu } from "./ui.svelte.js";

  let el = $state();
  let pos = $state(null);

  $effect(() => {
    const m = menu.current;
    if (!m || !el) {
      pos = null;
      return;
    }
    // Measure after the items render, then keep the menu on screen.
    el.style.left = `${m.x}px`;
    el.style.top = `${m.y}px`;
    const r = el.getBoundingClientRect();
    pos = {
      x: Math.max(8, Math.min(m.x, innerWidth - r.width - 8)),
      y: Math.max(8, Math.min(m.y, innerHeight - r.height - 8)),
    };
  });

  function pick(item) {
    if (!item.keepOpen) menu.close();
    item.action();
  }
</script>

<svelte:window
  onpointerdown={(e) => menu.current && !el?.contains(e.target) && menu.close()}
  onkeydown={(e) => menu.current && e.key === "Escape" && menu.close()}
  onblur={() => menu.close()}
  onresize={() => menu.close()}
/>

{#if menu.current}
  <div
    class="menu"
    bind:this={el}
    style={pos ? `left:${pos.x}px; top:${pos.y}px` : "visibility:hidden"}
    role="menu"
  >
    {#each menu.current.items as item}
      <button role="menuitem" class:danger={item.danger} onclick={() => pick(item)}>{item.label}</button>
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 50;
    min-width: 190px;
    max-width: 300px;
    max-height: 70vh;
    overflow-y: auto;
    padding: 4px;
    background: #141416;
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }
  button {
    display: block;
    width: 100%;
    padding: 7px 10px;
    border-radius: 6px;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button:hover {
    background: var(--hover);
  }
  button.danger {
    color: var(--error);
  }
</style>
