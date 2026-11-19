<script lang="ts">
  import type { Snippet } from "svelte";
  let { children }: { children: Snippet } = $props();
</script>

<div class="shelf">
  <div class="records">{@render children()}</div>
</div>

<style>
  .shelf { container-type: inline-size; }
  .records { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  @container (min-width: 540px) { .records { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @container (min-width: 840px) { .records { grid-template-columns: repeat(4, minmax(0, 1fr)); } }
</style>
