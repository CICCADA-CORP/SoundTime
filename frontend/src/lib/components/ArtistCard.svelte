<script lang="ts">
  import type { Artist } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";

  let { artist }: { artist: Artist } = $props();
</script>

<a href="/artists/{artist.id}" class="group block">
  <div class="bg-[hsl(var(--card))] rounded-lg p-4 hover:bg-[hsl(var(--secondary))] transition-colors duration-200">
    <div class="aspect-square rounded-full bg-[hsl(var(--secondary))] mb-3 overflow-hidden mx-auto w-36 h-36 ring-2 ring-transparent group-hover:ring-[hsl(var(--primary)/0.5)] transition-all duration-300">
      {#if artist.image_url}
        <img
          src={artist.image_url}
          alt={artist.name}
          loading="lazy"
          class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
        />
      {:else}
        <div class="w-full h-full flex items-center justify-center text-4xl">🎤</div>
      {/if}
    </div>
    <h3 class="text-sm font-medium truncate text-center group-hover:text-[hsl(var(--primary))] transition-colors">{artist.name}</h3>
    <p class="text-xs text-[hsl(var(--muted-foreground))] text-center mt-1">{t('artist.tracks')}</p>
  </div>
</a>
