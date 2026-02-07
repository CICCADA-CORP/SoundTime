<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Album, Track } from "$lib/types";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import { t } from "$lib/i18n/index.svelte";

  let recentTracks: Track[] = [];
  let albums: Album[] = [];
  let loading = true;

  onMount(async () => {
    try {
      const [tracksRes, albumsRes] = await Promise.all([
        api.get<{ data: Track[] }>("/tracks?per_page=10"),
        api.get<{ data: Album[] }>("/albums?per_page=8"),
      ]);
      recentTracks = tracksRes.data ?? [];
      albums = albumsRes.data ?? [];
    } catch {
      // Server not available
    } finally {
      loading = false;
    }
  });
</script>

<svelte:head>
  <title>SoundTime — Home</title>
</svelte:head>

<div class="space-y-8">
  <section>
    <h1 class="text-2xl font-bold mb-1">{t('home.welcome')}</h1>
    <p class="text-[hsl(var(--muted-foreground))]">{t('home.subtitle')}</p>
  </section>

  {#if loading}
    <div class="flex items-center justify-center py-20">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else}
    {#if albums.length > 0}
    <section>
      <h2 class="text-xl font-semibold mb-4">{t('home.recentAlbums')}</h2>
      <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4">
        {#each albums as album}
          <AlbumCard {album} />
        {/each}
      </div>
    </section>
    {/if}

    {#if recentTracks.length > 0}
    <section>
      <h2 class="text-xl font-semibold mb-4">{t('home.recentTracks')}</h2>
      <TrackList tracks={recentTracks} />
    </section>
    {/if}

    {#if albums.length === 0 && recentTracks.length === 0}
    <section class="text-center py-16">
      <div class="text-6xl mb-4">🎵</div>
      <h2 class="text-xl font-semibold mb-2">{t('home.noMusic')}</h2>
      <p class="text-[hsl(var(--muted-foreground))] mb-4">Upload your first tracks to get started.</p>
      <a href="/upload" class="inline-block px-6 py-2 bg-[hsl(var(--primary))] text-white rounded-full text-sm font-medium hover:opacity-90 transition">
        {t('home.uploadFirst')}
      </a>
    </section>
    {/if}
  {/if}
</div>
