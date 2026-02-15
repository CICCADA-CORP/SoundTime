<script lang="ts">
  import { Play } from "lucide-svelte";
  import type { Track } from "$lib/types";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";

  let { track, tracks, index }: { track: Track; tracks?: Track[]; index?: number } = $props();

  const player = getPlayerStore();
  const queue = getQueueStore();

  function handlePlay() {
    if (tracks && index !== undefined) {
      queue.playQueue(tracks, index);
    } else {
      player.play(track);
    }
  }
</script>

<button
  class="flex-shrink-0 w-40 group cursor-pointer text-left"
  onclick={handlePlay}
>
  <div class="relative aspect-square rounded-lg overflow-hidden mb-2 bg-[hsl(var(--secondary))]">
    {#if track.cover_url}
      <img
        src={track.cover_url}
        alt={track.title}
        loading="lazy"
        class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
      />
    {:else}
      <div class="w-full h-full flex items-center justify-center text-3xl">🎵</div>
    {/if}
    <div class="absolute inset-0 bg-black/0 group-hover:bg-black/40 transition-colors flex items-center justify-center">
      <div class="w-10 h-10 rounded-full bg-[hsl(var(--primary))] text-white flex items-center justify-center opacity-0 group-hover:opacity-100 transition-all transform scale-75 group-hover:scale-100 shadow-lg">
        <Play class="w-5 h-5 ml-0.5" fill="currentColor" />
      </div>
    </div>
  </div>
  <h3 class="text-sm font-medium truncate">{track.title}</h3>
  <p class="text-xs text-[hsl(var(--muted-foreground))] truncate mt-0.5">
    {track.artist_name ?? t('track.unknownArtist')}
  </p>
</button>
