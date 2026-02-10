<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Album, Artist, Track, EditorialPlaylist } from "$lib/types";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";

  const queue = getQueueStore();

  let tracks: Track[] = $state([]);
  let albums: Album[] = $state([]);
  let artists: Artist[] = $state([]);
  let editorialPlaylists: EditorialPlaylist[] = $state([]);
  let loading = $state(true);
  let selectedEditorial: EditorialPlaylist | null = $state(null);

  onMount(async () => {
    try {
      const [tr, al, ar, ed] = await Promise.all([
        api.get<{ data: Track[] }>("/tracks/popular?per_page=10"),
        api.get<{ data: Album[] }>("/albums?per_page=10"),
        api.get<{ data: Artist[] }>("/artists?per_page=10"),
        api.get<EditorialPlaylist[]>("/editorial-playlists").catch(() => []),
      ]);
      tracks = tr.data ?? [];
      albums = al.data ?? [];
      artists = ar.data ?? [];
      editorialPlaylists = ed ?? [];
    } catch (e) { console.error('Failed to load explore data:', e); } finally { loading = false; }
  });

  function openEditorial(pl: EditorialPlaylist) {
    selectedEditorial = pl;
  }

  function closeEditorial() {
    selectedEditorial = null;
  }
</script>

<svelte:head><title>{t('explore.title')} — SoundTime</title></svelte:head>

{#if loading}
  <div class="flex justify-center py-20">
    <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
  </div>
{:else}
<div class="space-y-10">
  <!-- Hero / Header -->
  <div class="relative overflow-hidden rounded-2xl bg-gradient-to-br from-[hsl(var(--primary)/0.15)] via-[hsl(var(--primary)/0.05)] to-transparent border border-[hsl(var(--border))] p-8 md:p-12">
    <div class="absolute -top-20 -right-20 w-64 h-64 bg-[hsl(var(--primary)/0.08)] rounded-full blur-3xl"></div>
    <div class="absolute -bottom-16 -left-16 w-48 h-48 bg-purple-500/5 rounded-full blur-3xl"></div>
    <div class="relative">
      <h1 class="text-3xl md:text-4xl font-bold mb-2">{t('explore.title')}</h1>
      <p class="text-[hsl(var(--muted-foreground))] text-lg max-w-xl">{t('explore.subtitle')}</p>
    </div>
  </div>

  <!-- Editorial Playlists -->
  {#if editorialPlaylists.length > 0}
    <section>
      <div class="flex items-center justify-between mb-4">
        <div>
          <h2 class="text-xl font-bold">{t('explore.editorialPlaylists')}</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('explore.curatedByAi')}</p>
        </div>
      </div>
      <div class="flex gap-4 overflow-x-auto pb-3 -mx-2 px-2 snap-x snap-mandatory scrollbar-thin">
        {#each editorialPlaylists as pl}
          <button
            class="flex-shrink-0 w-48 group cursor-pointer text-left snap-start"
            onclick={() => openEditorial(pl)}
          >
            <div class="relative aspect-square rounded-xl overflow-hidden mb-2 shadow-lg ring-1 ring-black/5">
              {#if pl.cover_url}
                <img
                  src={pl.cover_url}
                  alt={pl.name}
                  class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                />
              {:else}
                <div class="w-full h-full bg-gradient-to-br from-[hsl(var(--primary))] to-purple-700 flex items-center justify-center">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-12 h-12 text-white/60" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
                </div>
              {/if}
              <div class="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent opacity-0 group-hover:opacity-100 transition-opacity flex items-end p-3">
                <div class="flex items-center gap-2 text-white">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"/></svg>
                  <span class="text-xs font-medium">{pl.track_count} {t('explore.tracks')}</span>
                </div>
              </div>
            </div>
            <h3 class="font-semibold text-sm truncate group-hover:text-[hsl(var(--primary))] transition-colors">
              {pl.name}
            </h3>
            {#if pl.description}
              <p class="text-xs text-[hsl(var(--muted-foreground))] line-clamp-1 mt-0.5">{pl.description}</p>
            {/if}
          </button>
        {/each}
      </div>
    </section>
  {/if}

  <!-- Selected editorial detail overlay -->
  {#if selectedEditorial}
    <section class="bg-[hsl(var(--card))] rounded-xl border border-[hsl(var(--border))] p-6 shadow-sm">
      <div class="flex items-start justify-between mb-5">
        <div class="flex gap-4">
          {#if selectedEditorial.cover_url}
            <img src={selectedEditorial.cover_url} alt={selectedEditorial.name} class="w-20 h-20 rounded-lg object-cover shadow" />
          {:else}
            <div class="w-20 h-20 rounded-lg bg-gradient-to-br from-[hsl(var(--primary))] to-purple-700 flex items-center justify-center shadow">
              <svg xmlns="http://www.w3.org/2000/svg" class="w-8 h-8 text-white/60" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
            </div>
          {/if}
          <div>
            <h2 class="text-xl font-bold">{selectedEditorial.name}</h2>
            {#if selectedEditorial.description}
              <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1">{selectedEditorial.description}</p>
            {/if}
            <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{selectedEditorial.track_count} {t('explore.tracks')}</p>
          </div>
        </div>
        <button
          class="text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--foreground))] transition p-1.5 rounded-md hover:bg-[hsl(var(--secondary))]"
          onclick={closeEditorial}
          aria-label="Close"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
        </button>
      </div>
      <TrackList tracks={selectedEditorial.tracks} />
    </section>
  {/if}

  <!-- Popular Tracks -->
  {#if tracks.length > 0}
    <section>
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-xl font-bold">{t('explore.popularTracks')}</h2>
        <a href="/tracks" class="text-sm text-[hsl(var(--primary))] hover:underline font-medium">{t('explore.viewAll')}</a>
      </div>
      <TrackList {tracks} />
    </section>
  {/if}

  <!-- Recent Albums -->
  {#if albums.length > 0}
    <section>
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-xl font-bold">{t('explore.recentAlbums')}</h2>
        <a href="/albums" class="text-sm text-[hsl(var(--primary))] hover:underline font-medium">{t('explore.viewAll')}</a>
      </div>
      <div class="flex gap-4 overflow-x-auto pb-3 -mx-2 px-2 snap-x snap-mandatory scrollbar-thin">
        {#each albums as album}
          <div class="flex-shrink-0 w-44 snap-start">
            <AlbumCard {album} />
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <!-- Top Artists -->
  {#if artists.length > 0}
    <section>
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-xl font-bold">{t('explore.topArtists')}</h2>
        <a href="/artists" class="text-sm text-[hsl(var(--primary))] hover:underline font-medium">{t('explore.viewAll')}</a>
      </div>
      <div class="flex gap-4 overflow-x-auto pb-3 -mx-2 px-2 snap-x snap-mandatory scrollbar-thin">
        {#each artists as artist}
          <div class="flex-shrink-0 w-36 snap-start">
            <ArtistCard {artist} />
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <!-- Empty state -->
  {#if tracks.length === 0 && albums.length === 0 && artists.length === 0 && editorialPlaylists.length === 0}
    <div class="text-center py-16">
      <div class="w-16 h-16 mx-auto mb-4 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-8 h-8 text-[hsl(var(--muted-foreground))]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
      </div>
      <p class="text-[hsl(var(--muted-foreground))]">{t('explore.empty')}</p>
    </div>
  {/if}
</div>
{/if}
