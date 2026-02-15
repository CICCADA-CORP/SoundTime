<script lang="ts">
  import { Play } from "lucide-svelte";
  import type { Album } from "$lib/types";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";

  let { album }: { album: Album } = $props();
  const queue = getQueueStore();

  function handlePlay(e: Event) {
    e.preventDefault();
    e.stopPropagation();
    if (album.tracks && album.tracks.length > 0) {
      queue.playQueue(album.tracks);
    }
  }
</script>

<a href="/albums/{album.id}" class="group block">
  <div class="bg-[hsl(var(--card))] rounded-lg p-4 hover:bg-[hsl(var(--secondary))] transition-colors duration-200">
    <div class="aspect-square rounded-md bg-[hsl(var(--secondary))] mb-3 overflow-hidden relative">
      {#if album.cover_url}
        <img
          src={album.cover_url}
          alt={album.title}
          loading="lazy"
          class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
        />
      {:else}
        <div class="w-full h-full flex items-center justify-center text-4xl">💿</div>
      {/if}
      <button
        aria-label={t('a11y.playAlbum')}
        onclick={handlePlay}
        class="absolute bottom-2 right-2 w-10 h-10 rounded-full bg-[hsl(var(--primary))] text-white flex items-center justify-center opacity-0 group-hover:opacity-100 transition-all transform translate-y-2 group-hover:translate-y-0 shadow-lg"
      >
        <Play class="w-5 h-5 ml-0.5" fill="currentColor" />
      </button>
    </div>
    <h3 class="text-sm font-medium truncate">{album.title}</h3>
    <p class="text-xs text-[hsl(var(--muted-foreground))] truncate mt-1">
      {album.year ?? ""}{album.artist_name ? ` · ${album.artist_name}` : ""}
    </p>
  </div>
</a>
