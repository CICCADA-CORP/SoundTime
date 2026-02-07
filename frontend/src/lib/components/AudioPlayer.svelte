<script lang="ts">
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { api, API_BASE } from "$lib/api";
  import { formatDuration } from "$lib/utils";
  import FavoriteButton from "./FavoriteButton.svelte";
  import ExpandedPlayer from "./ExpandedPlayer.svelte";

  const player = getPlayerStore();
  const queue = getQueueStore();
  const auth = getAuthStore();

  let liked = $state(false);
  let lastCheckedTrackId = $state<string | null>(null);
  let expanded = $state(false);

  // Check favorite status when track changes
  $effect(() => {
    const trackId = player.currentTrack?.id;
    if (trackId && auth.isAuthenticated && trackId !== lastCheckedTrackId) {
      lastCheckedTrackId = trackId;
      api.get<string[]>(`/favorites/check?track_ids=${trackId}`)
        .then((ids) => { liked = ids.includes(trackId); })
        .catch(() => { liked = false; });
    } else if (!trackId) {
      liked = false;
      lastCheckedTrackId = null;
    }
  });

  function resolveMediaUrl(url: string): string {
    // If already absolute, return as-is
    if (url.startsWith("http")) return url;
    // Strip /api prefix from API_BASE and prepend
    const base = API_BASE.replace(/\/api$/, "");
    return `${base}${url}`;
  }

  function handleSeek(e: MouseEvent) {
    const bar = e.currentTarget as HTMLElement;
    const rect = bar.getBoundingClientRect();
    const ratio = (e.clientX - rect.left) / rect.width;
    player.seek(ratio * player.duration);
  }

  function handleVolume(e: MouseEvent) {
    const bar = e.currentTarget as HTMLElement;
    const rect = bar.getBoundingClientRect();
    const ratio = (e.clientX - rect.left) / rect.width;
    player.setVolume(ratio);
  }
</script>

{#if player.currentTrack}
  <div class="fixed bottom-0 left-0 right-0 h-20 bg-[hsl(0,0%,10%)] border-t border-[hsl(var(--border))] flex items-center px-4 z-50">
    <!-- Track Info -->
    <div class="flex items-center gap-3 w-64 min-w-0">
      <button class="w-12 h-12 rounded bg-[hsl(var(--secondary))] flex items-center justify-center text-lg flex-shrink-0 cursor-pointer hover:ring-2 hover:ring-[hsl(var(--primary))] transition" onclick={() => expanded = true} title="Vue étendue">
        {#if player.currentTrack.cover_url}
          <img src={resolveMediaUrl(player.currentTrack.cover_url)} alt="" class="w-full h-full rounded object-cover" />
        {:else}
          🎵
        {/if}
      </button>
      <div class="min-w-0 flex-1">
        <p class="text-sm font-medium truncate">{player.currentTrack.title}</p>
        <p class="text-xs text-[hsl(var(--muted-foreground))] truncate">{player.currentTrack.artist_name ?? "Unknown"}</p>
      </div>
      <FavoriteButton trackId={player.currentTrack.id} bind:liked size={16} />
    </div>

    <!-- Controls -->
    <div class="flex-1 flex flex-col items-center max-w-xl mx-auto">
      <div class="flex items-center gap-4 mb-1">
        <button class="text-[hsl(var(--muted-foreground))] hover:text-white transition" class:text-[hsl(var(--primary))]={player.shuffle} onclick={player.toggleShuffle} title="Shuffle">
          <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 24 24"><path d="M10.59 9.17L5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z"/></svg>
        </button>
        <button class="text-[hsl(var(--muted-foreground))] hover:text-white transition" onclick={queue.previous} title="Previous">
          <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 24 24"><path d="M6 6h2v12H6zm3.5 6l8.5 6V6z"/></svg>
        </button>
        <button class="w-8 h-8 rounded-full bg-white text-black flex items-center justify-center hover:scale-105 transition" onclick={player.togglePlay}>
          {#if player.isPlaying}
            <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 24 24"><path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
          {:else}
            <svg class="w-4 h-4 ml-0.5" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
          {/if}
        </button>
        <button class="text-[hsl(var(--muted-foreground))] hover:text-white transition" onclick={queue.next} title="Next">
          <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 24 24"><path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z"/></svg>
        </button>
        <button class="text-[hsl(var(--muted-foreground))] hover:text-white transition" class:text-[hsl(var(--primary))]={player.repeat !== 'none'} onclick={player.cycleRepeat} title="Repeat: {player.repeat}">
          <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 24 24"><path d="M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z"/></svg>
          {#if player.repeat === 'one'}
            <span class="absolute text-[8px] font-bold">1</span>
          {/if}
        </button>
      </div>

      <!-- Progress Bar -->
      <div class="flex items-center gap-2 w-full">
        <span class="text-xs text-[hsl(var(--muted-foreground))] w-10 text-right">{formatDuration(player.progress)}</span>
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="flex-1 h-1 bg-[hsl(var(--secondary))] rounded-full cursor-pointer group" onclick={handleSeek}>
          <div class="h-full bg-white rounded-full group-hover:bg-[hsl(var(--primary))] transition relative"
               style="width: {player.duration > 0 ? (player.progress / player.duration) * 100 : 0}%">
            <div class="absolute right-0 top-1/2 -translate-y-1/2 w-3 h-3 bg-white rounded-full opacity-0 group-hover:opacity-100 transition"></div>
          </div>
        </div>
        <span class="text-xs text-[hsl(var(--muted-foreground))] w-10">{formatDuration(player.duration)}</span>
      </div>
    </div>

    <!-- Volume -->
    <div class="flex items-center gap-2 w-40 justify-end">
      <svg class="w-4 h-4 text-[hsl(var(--muted-foreground))]" fill="currentColor" viewBox="0 0 24 24"><path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z"/></svg>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="flex-1 h-1 bg-[hsl(var(--secondary))] rounded-full cursor-pointer" onclick={handleVolume}>
        <div class="h-full bg-white rounded-full" style="width: {player.volume * 100}%"></div>
      </div>
    </div>
  </div>
{/if}

<ExpandedPlayer bind:open={expanded} onclose={() => expanded = false} />
