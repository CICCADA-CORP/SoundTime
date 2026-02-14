<script lang="ts">
  import { Play } from "lucide-svelte";
  import type { Track } from "$lib/types";
  import { getPlayerStore } from "$lib/stores/player.svelte";

  let { track }: { track: Track } = $props();
  const player = getPlayerStore();
</script>

<button
  class="flex items-center gap-3 bg-[hsl(var(--secondary)/0.6)] hover:bg-[hsl(var(--secondary))] rounded-md overflow-hidden group transition-colors h-14 w-full text-left"
  onclick={() => player.play(track)}
>
  <div class="w-14 h-14 flex-shrink-0 bg-[hsl(var(--secondary))]">
    {#if track.cover_url}
      <img src={track.cover_url} alt={track.title} loading="lazy" class="w-full h-full object-cover" />
    {:else}
      <div class="w-full h-full flex items-center justify-center text-lg">🎵</div>
    {/if}
  </div>
  <span class="text-sm font-medium truncate flex-1">{track.title}</span>
  <div class="w-8 h-8 rounded-full bg-[hsl(var(--primary))] text-white flex items-center justify-center opacity-0 group-hover:opacity-100 transition mr-3 shadow-md">
    <Play class="w-4 h-4 ml-0.5" fill="currentColor" />
  </div>
</button>
