<script lang="ts">
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { api, API_BASE } from "$lib/api";
  import { formatDuration } from "$lib/utils";
  import FavoriteButton from "./FavoriteButton.svelte";
  import { X, ChevronDown, Music, ListMusic, Mic2 } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";

  interface Props {
    open: boolean;
    onclose: () => void;
  }

  let { open = $bindable(), onclose }: Props = $props();

  const player = getPlayerStore();
  const queue = getQueueStore();
  const auth = getAuthStore();

  // Lyrics state  
  let lyrics = $state<string | null>(null);
  let lyricsSource = $state<string | null>(null);
  let lyricsLoading = $state(false);
  let lyricsError = $state<string | null>(null);
  let lastLyricsTrackId = $state<string | null>(null);

  // Active panel on mobile  
  let activePanel = $state<"info" | "lyrics" | "queue">("info");

  // Favorite state  
  let liked = $state(false);
  let lastCheckedTrackId = $state<string | null>(null);

  function resolveMediaUrl(url: string): string {
    if (url.startsWith("http")) return url;
    const base = API_BASE.replace(/\/api$/, "");
    return `${base}${url}`;
  }

  // Fetch lyrics when track changes and panel is open
  $effect(() => {
    const trackId = player.currentTrack?.id;
    if (open && trackId && trackId !== lastLyricsTrackId) {
      lastLyricsTrackId = trackId;
      fetchLyrics(trackId);
    }
  });

  // Check favorite status
  $effect(() => {
    const trackId = player.currentTrack?.id;
    if (open && trackId && auth.isAuthenticated && trackId !== lastCheckedTrackId) {
      lastCheckedTrackId = trackId;
      api.get<string[]>(`/favorites/check?track_ids=${trackId}`)
        .then((ids) => { liked = ids.includes(trackId); })
        .catch(() => { liked = false; });
    }
  });

  async function fetchLyrics(trackId: string) {
    lyricsLoading = true;
    lyricsError = null;
    lyrics = null;
    lyricsSource = null;
    try {
      const res = await api.get<{ lyrics: string | null; source: string | null }>(`/tracks/${trackId}/lyrics`);
      lyrics = res.lyrics;
      lyricsSource = res.source;
    } catch (e: any) {
      if (e?.status === 503 || e?.status === 404) {
        lyrics = null;
      } else {
        lyricsError = "Impossible de récupérer les paroles";
      }
    } finally {
      lyricsLoading = false;
    }
  }

  function handleSeek(e: MouseEvent) {
    const bar = e.currentTarget as HTMLElement;
    const rect = bar.getBoundingClientRect();
    const ratio = (e.clientX - rect.left) / rect.width;
    player.seek(ratio * player.duration);
  }

  function playFromQueue(index: number) {
    if (queue.queue[index]) {
      const player = getPlayerStore();
      // Update queue index and play
      queue.playQueue(queue.queue, index);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onclose();
    }
  }

  // Upcoming tracks (after current)
  let upcomingTracks = $derived(
    queue.currentIndex >= 0
      ? queue.queue.slice(queue.currentIndex + 1)
      : []
  );
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open && player.currentTrack}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-[100] bg-[hsl(0,0%,4%)] flex flex-col transition-all duration-300"
    class:animate-slide-up={open}
  >
    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4">
      <button
        onclick={onclose}
        class="text-[hsl(var(--muted-foreground))] hover:text-white transition p-2 rounded-full hover:bg-white/10"
        aria-label="Fermer"
      >
        <ChevronDown class="w-6 h-6" />
      </button>

      <p class="text-xs font-medium text-[hsl(var(--muted-foreground))] uppercase tracking-widest">
        {t('player.nowPlaying')}
      </p>

      <div class="w-10"></div>
    </div>

    <!-- Mobile panel switcher -->
    <div class="flex justify-center gap-1 px-6 pb-3 md:hidden">
      <button
        class="px-3 py-1.5 text-xs rounded-full transition font-medium {activePanel === 'info' ? 'bg-white/15 text-white' : 'text-[hsl(var(--muted-foreground))] hover:text-white'}"
        onclick={() => activePanel = "info"}
      >
        <Music class="w-3.5 h-3.5 inline-block mr-1" />
        {t('player.track')}
      </button>
      <button
        class="px-3 py-1.5 text-xs rounded-full transition font-medium {activePanel === 'lyrics' ? 'bg-white/15 text-white' : 'text-[hsl(var(--muted-foreground))] hover:text-white'}"
        onclick={() => activePanel = "lyrics"}
      >
        <Mic2 class="w-3.5 h-3.5 inline-block mr-1" />
        {t('player.lyrics')}
      </button>
      <button
        class="px-3 py-1.5 text-xs rounded-full transition font-medium {activePanel === 'queue' ? 'bg-white/15 text-white' : 'text-[hsl(var(--muted-foreground))] hover:text-white'}"
        onclick={() => activePanel = "queue"}
      >
        <ListMusic class="w-3.5 h-3.5 inline-block mr-1" />
        {t('player.queue')}
      </button>
    </div>

    <!-- Main content — 3 columns on desktop, swipeable on mobile -->
    <div class="flex-1 overflow-hidden flex">
      <!-- Left: Track info + Cover -->
      <div class="flex-1 flex flex-col items-center justify-center px-6 md:px-12 {activePanel !== 'info' ? 'hidden md:flex' : ''}">
        <!-- Cover art -->
        <div class="w-full max-w-[380px] aspect-square rounded-lg overflow-hidden bg-[hsl(var(--secondary))] shadow-2xl mb-8">
          {#if player.currentTrack.cover_url}
            <img
              src={resolveMediaUrl(player.currentTrack.cover_url)}
              alt={player.currentTrack.title}
              class="w-full h-full object-cover"
            />
          {:else}
            <div class="w-full h-full flex items-center justify-center">
              <Music class="w-24 h-24 text-[hsl(var(--muted-foreground))]" />
            </div>
          {/if}
        </div>

        <!-- Track info -->
        <div class="w-full max-w-[380px] flex items-start justify-between gap-3">
          <div class="min-w-0 flex-1">
            <h2 class="text-xl font-bold truncate">{player.currentTrack.title}</h2>
            <p class="text-sm text-[hsl(var(--muted-foreground))] truncate">{player.currentTrack.artist_name ?? "Unknown Artist"}</p>
            {#if player.currentTrack.album_title}
              <p class="text-xs text-[hsl(var(--muted-foreground))]/60 truncate mt-0.5">{player.currentTrack.album_title}</p>
            {/if}
          </div>
          {#if auth.isAuthenticated}
            <FavoriteButton trackId={player.currentTrack.id} bind:liked size={22} />
          {/if}
        </div>

        <!-- Progress bar -->
        <div class="w-full max-w-[380px] mt-6">
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="w-full h-1.5 bg-[hsl(var(--secondary))] rounded-full cursor-pointer group" onclick={handleSeek}>
            <div
              class="h-full bg-white rounded-full group-hover:bg-[hsl(var(--primary))] transition relative"
              style="width: {player.duration > 0 ? (player.progress / player.duration) * 100 : 0}%"
            >
              <div class="absolute right-0 top-1/2 -translate-y-1/2 w-3 h-3 bg-white rounded-full opacity-0 group-hover:opacity-100 transition"></div>
            </div>
          </div>
          <div class="flex justify-between mt-1.5">
            <span class="text-xs text-[hsl(var(--muted-foreground))]">{formatDuration(player.progress)}</span>
            <span class="text-xs text-[hsl(var(--muted-foreground))]">{formatDuration(player.duration)}</span>
          </div>
        </div>

        <!-- Controls -->
        <div class="flex items-center gap-6 mt-4">
          <button class="text-[hsl(var(--muted-foreground))] hover:text-white transition" class:text-[hsl(var(--primary))]={player.shuffle} onclick={player.toggleShuffle} title="Shuffle">
            <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 24 24"><path d="M10.59 9.17L5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z"/></svg>
          </button>
          <button class="text-[hsl(var(--muted-foreground))] hover:text-white transition" onclick={queue.previous} title="Précédent">
            <svg class="w-7 h-7" fill="currentColor" viewBox="0 0 24 24"><path d="M6 6h2v12H6zm3.5 6l8.5 6V6z"/></svg>
          </button>
          <button class="w-14 h-14 rounded-full bg-white text-black flex items-center justify-center hover:scale-105 transition" onclick={player.togglePlay}>
            {#if player.isPlaying}
              <svg class="w-6 h-6" fill="currentColor" viewBox="0 0 24 24"><path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
            {:else}
              <svg class="w-6 h-6 ml-0.5" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
            {/if}
          </button>
          <button class="text-[hsl(var(--muted-foreground))] hover:text-white transition" onclick={queue.next} title="Suivant">
            <svg class="w-7 h-7" fill="currentColor" viewBox="0 0 24 24"><path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z"/></svg>
          </button>
          <button class="text-[hsl(var(--muted-foreground))] hover:text-white transition" class:text-[hsl(var(--primary))]={player.repeat !== 'none'} onclick={player.cycleRepeat} title="Repeat: {player.repeat}">
            <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 24 24"><path d="M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z"/></svg>
            {#if player.repeat === 'one'}
              <span class="absolute text-[8px] font-bold">1</span>
            {/if}
          </button>
        </div>

        <!-- Technical details -->
        <div class="flex items-center gap-3 mt-6 text-[10px] text-[hsl(var(--muted-foreground))]/60 uppercase tracking-wider">
          {#if player.currentTrack.format}
            <span class="px-2 py-0.5 rounded bg-white/5">{player.currentTrack.format}</span>
          {/if}
          {#if player.currentTrack.bitrate}
            <span>{player.currentTrack.bitrate} kbps</span>
          {/if}
          {#if player.currentTrack.sample_rate}
            <span>{(player.currentTrack.sample_rate / 1000).toFixed(1)} kHz</span>
          {/if}
        </div>
      </div>

      <!-- Center: Lyrics -->
      <div class="w-[380px] flex-shrink-0 border-x border-[hsl(var(--border))]/20 flex flex-col {activePanel !== 'lyrics' ? 'hidden md:flex' : 'flex-1'}">
        <div class="px-6 py-3 border-b border-[hsl(var(--border))]/20">
          <h3 class="text-sm font-semibold flex items-center gap-2">
            <Mic2 class="w-4 h-4 text-[hsl(var(--primary))]" />
            {t('player.lyrics')}
          </h3>
        </div>
        <div class="flex-1 overflow-y-auto px-6 py-4">
          {#if lyricsLoading}
            <div class="flex items-center justify-center h-full">
              <div class="w-6 h-6 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
            </div>
          {:else if lyricsError}
            <p class="text-sm text-red-400 text-center mt-8">{lyricsError}</p>
          {:else if lyrics}
            <div class="text-sm text-[hsl(var(--muted-foreground))] leading-relaxed whitespace-pre-wrap">
              {lyrics}
            </div>
            {#if lyricsSource}
              <p class="text-[10px] text-[hsl(var(--muted-foreground))]/40 mt-4 uppercase tracking-wider">
                Source : {lyricsSource}
              </p>
            {/if}
          {:else}
            <div class="flex flex-col items-center justify-center h-full text-center">
              <Mic2 class="w-10 h-10 text-[hsl(var(--muted-foreground))]/30 mb-3" />
              <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('player.noLyrics')}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))]/50 mt-1">{t('player.noLyricsHint')}</p>
            </div>
          {/if}
        </div>
      </div>

      <!-- Right: Queue -->
      <div class="w-[340px] flex-shrink-0 flex flex-col {activePanel !== 'queue' ? 'hidden md:flex' : 'flex-1'}">
        <div class="px-6 py-3 border-b border-[hsl(var(--border))]/20">
          <h3 class="text-sm font-semibold flex items-center gap-2">
            <ListMusic class="w-4 h-4 text-[hsl(var(--primary))]" />
            {t('player.queue')}
            {#if upcomingTracks.length > 0}
              <span class="text-xs text-[hsl(var(--muted-foreground))] font-normal">({upcomingTracks.length})</span>
            {/if}
          </h3>
        </div>
        <div class="flex-1 overflow-y-auto">
          {#if upcomingTracks.length === 0}
            <div class="flex flex-col items-center justify-center h-full text-center px-6">
              <ListMusic class="w-10 h-10 text-[hsl(var(--muted-foreground))]/30 mb-3" />
              <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('player.queueEmpty')}</p>
            </div>
          {:else}
            <div class="divide-y divide-[hsl(var(--border))]/10">
              {#each upcomingTracks as track, i}
                <button
                  class="w-full flex items-center gap-3 px-6 py-3 hover:bg-white/5 transition text-left"
                  onclick={() => playFromQueue(queue.currentIndex + 1 + i)}
                >
                  <span class="text-xs text-[hsl(var(--muted-foreground))] w-5 text-right flex-shrink-0">{i + 1}</span>
                  <div class="w-10 h-10 rounded bg-[hsl(var(--secondary))] flex-shrink-0 overflow-hidden">
                    {#if track.cover_url}
                      <img src={resolveMediaUrl(track.cover_url)} alt="" class="w-full h-full object-cover" />
                    {:else}
                      <div class="w-full h-full flex items-center justify-center text-xs">🎵</div>
                    {/if}
                  </div>
                  <div class="min-w-0 flex-1">
                    <p class="text-sm font-medium truncate">{track.title}</p>
                    <p class="text-xs text-[hsl(var(--muted-foreground))] truncate">{track.artist_name ?? "Unknown"}</p>
                  </div>
                  <span class="text-xs text-[hsl(var(--muted-foreground))] flex-shrink-0">{formatDuration(track.duration_secs)}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  @keyframes slide-up {
    from {
      transform: translateY(100%);
      opacity: 0;
    }
    to {
      transform: translateY(0);
      opacity: 1;
    }
  }

  .animate-slide-up {
    animation: slide-up 0.3s ease-out;
  }
</style>
