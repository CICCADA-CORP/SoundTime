<script lang="ts">
  import { ChevronLeft, ChevronRight } from "lucide-svelte";
  import type { Snippet } from "svelte";
  import { t } from "$lib/i18n/index.svelte";

  let { label, banners = false, children }: {
    label: string; banners?: boolean; children: Snippet;
  } = $props();
  let viewport: HTMLDivElement;
  let width = $state(0);
  let fullWidth = $state(0);
  let left = $state(0);
  const hasNext = $derived(fullWidth > width && left + width < fullWidth - 2);

  function scroll(direction: number) {
    viewport.scrollBy({ left: direction * viewport.clientWidth * .85 });
  }
</script>

<div class="rail" class:banners>
  <div class="viewport" role="region" aria-label={label} bind:this={viewport} bind:clientWidth={width}
    onscroll={() => left = viewport.scrollLeft}>
    <div class="items" bind:offsetWidth={fullWidth}>{@render children()}</div>
  </div>
  {#if left > 2}
    <button class="arrow prev" type="button" aria-label={`${t("player.previous")} · ${label}`} onclick={() => scroll(-1)}><ChevronLeft size={17} /></button>
  {/if}
  {#if hasNext}
    <button class="arrow next" type="button" aria-label={`${t("player.next")} · ${label}`} onclick={() => scroll(1)}><ChevronRight size={17} /></button>
  {/if}
</div>

<style>
  .rail { --item: clamp(8.5rem, 21cqi, 12rem); --gap: clamp(.85rem, 2.65cqi, 1.8rem); position: relative; container-type: inline-size; }
  .banners { --item: clamp(12.75rem, 31cqi, 18rem); }
  .viewport { overflow-x: auto; scrollbar-width: none; scroll-snap-type: x proximity; overscroll-behavior-x: contain; border-radius: .25rem; }
  .viewport::-webkit-scrollbar { display: none; }
  .viewport:focus-visible { outline-offset: -2px; }
  .items { display: grid; width: max-content; grid-auto-flow: column; grid-auto-columns: var(--item); gap: var(--gap); padding: 2px 1px .65rem; }
  .items > :global(*) { min-width: 0; scroll-snap-align: start; }
  .arrow { position: absolute; z-index: 2; top: calc(var(--item) / 2); transform: translateY(-50%); display: grid; width: 1.75rem; height: 1.75rem; place-items: center; border: 1px solid hsl(var(--border)); border-radius: 50%; background: hsl(var(--card) / .96); color: hsl(var(--foreground)); box-shadow: 0 2px 8px #0002; }
  .prev { left: -.7rem; }
  .next { right: -.7rem; }
  .banners .arrow { top: calc(var(--item) / 3); }
  @media (hover: hover) and (prefers-reduced-motion: no-preference) { .viewport { scroll-behavior: smooth; } }
  @media (hover: none) { .arrow { display: none; } }
</style>
