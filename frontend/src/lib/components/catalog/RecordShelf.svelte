<script lang="ts">
  import type { Snippet } from "svelte";
  let { children, mobileRow = false }: { children: Snippet; mobileRow?: boolean } = $props();
</script>

<div class="shelf">
  <div class="records" class:mobile-row={mobileRow}>{@render children()}</div>
</div>

<style>
  .shelf { container-type: inline-size; }
  .records { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  @container (max-width: 539px) {
    .mobile-row { grid-template-columns: none; grid-auto-flow: column; grid-auto-columns: max(8.25rem, 44%); overflow-x: auto; scroll-snap-type: x proximity; padding: 2px 2px 5px; }
    .mobile-row > :global(*) { scroll-snap-align: start; }
  }
  @container (min-width: 540px) { .records { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @container (min-width: 840px) { .records { grid-template-columns: repeat(4, minmax(0, 1fr)); } }
</style>
