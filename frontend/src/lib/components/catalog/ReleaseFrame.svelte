<script lang="ts">
  import type { Snippet } from "svelte";

  let { cover, portrait = false, children }: {
    cover?: Snippet; portrait?: boolean; children: Snippet;
  } = $props();
</script>

<header class="release">
  <div class="layout">
    <div class="details">{@render children()}</div>
    <div class="cover" class:portrait>{@render cover?.()}</div>
  </div>
</header>

<style>
  .release { container-type: inline-size; }
  .layout { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 32%); grid-template-areas: "details art"; align-items: center;
    column-gap: clamp(1rem, 4.5cqi, 3.3rem); row-gap: 1.75rem; }
  .details { grid-area: details; min-width: 0; }
  .cover { grid-area: art; width: 100%; max-width: 23rem; justify-self: end; aspect-ratio: 1; overflow: hidden; border-radius: var(--art-radius);
    background: hsl(var(--muted)); box-shadow: var(--glass-shadow); }
  .portrait { border-radius: 50%; }
  @container (max-width: 24rem) {
    .layout { grid-template-columns: minmax(0, 1fr) minmax(4rem, 24%); align-items: start; gap: 1.2rem .75rem; }
  }
</style>
