<script>
  // Small dark right-click menu. `menu` is { x, y, items: [{ label, action }] } or null.
  let { menu, onclose } = $props();
  let el = $state();
  let pos = $state({ x: 0, y: 0 });

  $effect(() => {
    if (!menu || !el) return;
    const r = el.getBoundingClientRect();
    pos = {
      x: Math.min(menu.x, innerWidth - r.width - 8),
      y: Math.min(menu.y, innerHeight - r.height - 8),
    };
  });

  function pick(item) {
    onclose();
    item.action();
  }
</script>

<svelte:window
  onpointerdown={(e) => menu && !el?.contains(e.target) && onclose()}
  onkeydown={(e) => menu && e.key === "Escape" && onclose()}
  onblur={() => menu && onclose()}
  onresize={() => menu && onclose()}
/>

{#if menu}
  <div class="menu" bind:this={el} style="left:{pos.x || menu.x}px; top:{pos.y || menu.y}px" role="menu">
    {#each menu.items as item}
      <button role="menuitem" onclick={() => pick(item)}>{item.label}</button>
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 50;
    min-width: 170px;
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
  }
  button:hover {
    background: var(--hover);
  }
</style>
