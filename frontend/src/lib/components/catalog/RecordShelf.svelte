<script lang="ts">
  import type { Snippet } from "svelte";
  let { children, mobileRow = false }: { children: Snippet; mobileRow?: boolean } = $props();
</script>

<div class="shelf">
  <div class="records" class:mobile-row={mobileRow}>{@render children()}</div>
</div>

<style>
  .shelf { container-type: inline-size; }
  .records { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 9.1rem), 1fr)); column-gap: clamp(1rem, 2.4cqi, 1.65rem); row-gap: 1.7rem; }
  @container (max-width: 24rem) { .records { grid-template-columns: repeat(2, minmax(0,1fr)); } }
  @container (max-width: 539px) {
    .mobile-row { grid-template-columns: none; grid-auto-flow: column; grid-auto-columns: max(8.25rem,44%); overflow-x: auto; scroll-snap-type: x proximity; padding: 2px 2px 6px; }
    .mobile-row > :global(*) { scroll-snap-align: start; }
  }
</style>
