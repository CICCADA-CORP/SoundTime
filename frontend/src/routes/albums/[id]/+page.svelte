<script lang="ts">
  import { page } from "$app/stores";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Album } from "$lib/types";
  import TrackList from "$lib/components/TrackList.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { formatDuration } from "$lib/utils";
  import { t } from "$lib/i18n/index.svelte";

  const queue = getQueueStore();
  let album: Album | null = null;
  let loading = true;

  onMount(async () => {
    try {
      album = await api.get<Album>(`/albums/${$page.params.id}`);
    } catch { /* empty */ } finally { loading = false; }
  });

  function playAll() {
    if (album?.tracks) queue.playQueue(album.tracks);
  }

  $: totalDuration = album?.tracks?.reduce((sum, t) => sum + t.duration_secs, 0) ?? 0;
</script>

<svelte:head><title>{album?.title ?? "Album"} — SoundTime</title></svelte:head>

{#if loading}
  <div class="flex justify-center py-20">
    <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
  </div>
{:else if album}
  <div class="space-y-6">
    <div class="flex items-end gap-6">
      <div class="w-52 h-52 rounded-lg bg-[hsl(var(--secondary))] flex items-center justify-center text-6xl shadow-xl flex-shrink-0 overflow-hidden">
        {#if album.cover_url}
          <img src={album.cover_url} alt={album.title} class="w-full h-full object-cover" />
        {:else}
          💿
        {/if}
      </div>
      <div>
        <p class="text-xs uppercase tracking-wider text-[hsl(var(--muted-foreground))]">Album</p>
        <h1 class="text-4xl font-bold mt-1">{album.title}</h1>
        <p class="text-sm text-[hsl(var(--muted-foreground))] mt-2">
          <a href="/artists/{album.artist_id}" class="hover:underline">{album.artist_name ?? "Unknown Artist"}</a>
          {#if album.year} · {album.year}{/if}
          · {album.tracks?.length ?? 0} {t('album.tracks')}
          · {formatDuration(totalDuration)}
        </p>
      </div>
    </div>

    <div class="flex gap-3">
      <button aria-label="Play all" class="w-12 h-12 rounded-full bg-[hsl(var(--primary))] text-white flex items-center justify-center hover:scale-105 transition" on:click={playAll}>
        <svg class="w-5 h-5 ml-0.5" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
      </button>
    </div>

    {#if album.tracks && album.tracks.length > 0}
      <TrackList tracks={album.tracks} showAlbum={false} />
    {/if}
  </div>
{:else}
  <p class="text-center py-20 text-[hsl(var(--muted-foreground))]">Album not found.</p>
{/if}
