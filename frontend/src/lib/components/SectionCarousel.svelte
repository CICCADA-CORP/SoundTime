<script lang="ts">
  import { ChevronLeft, ChevronRight } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Snippet } from "svelte";

  let { title, href, children }: { title: string; href?: string; children: Snippet } = $props();

  let scrollContainer: HTMLDivElement | undefined = $state();
  let canScrollLeft = $state(false);
  let canScrollRight = $state(true);

  function updateScrollState() {
    if (!scrollContainer) return;
    canScrollLeft = scrollContainer.scrollLeft > 0;
    canScrollRight = scrollContainer.scrollLeft < scrollContainer.scrollWidth - scrollContainer.clientWidth - 1;
  }

  function scrollBy(direction: number) {
    scrollContainer?.scrollBy({ left: direction * 300, behavior: "smooth" });
  }
</script>

<section>
  <div class="flex items-center justify-between mb-4">
    <h2 class="text-xl font-bold">{title}</h2>
    <div class="flex items-center gap-2">
      <button
        class="p-1.5 rounded-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] hover:bg-[hsl(var(--border))] transition disabled:opacity-30"
        disabled={!canScrollLeft}
        onclick={() => scrollBy(-1)}
        aria-label={t('a11y.scrollLeft')}
      >
        <ChevronLeft class="w-4 h-4" />
      </button>
      <button
        class="p-1.5 rounded-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] hover:bg-[hsl(var(--border))] transition disabled:opacity-30"
        disabled={!canScrollRight}
        onclick={() => scrollBy(1)}
        aria-label={t('a11y.scrollRight')}
      >
        <ChevronRight class="w-4 h-4" />
      </button>
      {#if href}
        <a href={href} class="text-sm text-[hsl(var(--primary))] hover:underline font-medium ml-2">
          {t('explore.viewAll')}
        </a>
      {/if}
    </div>
  </div>
  <div
    bind:this={scrollContainer}
    onscroll={updateScrollState}
    class="flex gap-4 overflow-x-auto pb-3 scrollbar-thin scroll-smooth"
  >
    {@render children()}
  </div>
</section>
