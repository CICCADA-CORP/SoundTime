<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Track, Playlist, Album } from "$lib/types";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();
  let uploads: Track[] = $state([]);
  let playlists: Playlist[] = $state([]);
  let albums: Album[] = $state([]);
  let loading = $state(true);
  let activeTab = $state<'uploads' | 'playlists' | 'albums'>('uploads');

  onMount(async () => {
    if (!auth.isAuthenticated) { loading = false; return; }
    try {
      const [u, p, a] = await Promise.all([
        api.get<{ data: Track[] }>("/tracks/my-uploads?per_page=100"),
        api.get<{ data: Playlist[] }>("/playlists"),
        api.get<{ data: Album[] }>("/albums?per_page=12"),
      ]);
      uploads = u.data ?? [];
      playlists = p.data ?? [];
      albums = a.data ?? [];
    } catch (e) { console.error('Failed to load library:', e); } finally { loading = false; }
  });
</script>

<svelte:head><title>{t('library.title')} — SoundTime</title></svelte:head>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold">{t('library.title')}</h1>
    {#if auth.isAuthenticated}
      <a
        href="/upload"
        class="inline-flex items-center gap-2 px-4 py-2 bg-[hsl(var(--primary))] text-white rounded-full text-sm font-medium hover:opacity-90 transition"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
        {t('nav.upload')}
      </a>
    {/if}
  </div>

  {#if !auth.isAuthenticated}
    <div class="text-center py-16">
      <div class="w-16 h-16 mx-auto mb-4 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-8 h-8 text-[hsl(var(--muted-foreground))]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
      </div>
      <p class="text-[hsl(var(--muted-foreground))] mb-4">{t('library.signInPrompt')}</p>
      <a href="/login" class="inline-block px-6 py-2 bg-[hsl(var(--primary))] text-white rounded-full text-sm font-medium">{t('auth.signIn')}</a>
    </div>
  {:else if loading}
    <div class="flex justify-center py-20">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else}
    <!-- Tabs -->
    <div class="flex gap-2 border-b border-[hsl(var(--border))] pb-1">
      {#each [
        { id: 'uploads' as const, label: t('library.uploads'), count: uploads.length },
        { id: 'playlists' as const, label: t('library.playlists'), count: playlists.length },
        { id: 'albums' as const, label: t('library.albums'), count: albums.length },
      ] as tab}
        <button
          class="px-4 py-2 text-sm font-medium transition-colors relative {activeTab === tab.id ? 'text-[hsl(var(--primary))]' : 'text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--foreground))]'}"
          onclick={() => activeTab = tab.id}
        >
          {tab.label}
          <span class="ml-1 text-xs opacity-60">({tab.count})</span>
          {#if activeTab === tab.id}
            <span class="absolute bottom-0 left-0 right-0 h-0.5 bg-[hsl(var(--primary))] rounded-full"></span>
          {/if}
        </button>
      {/each}
    </div>

    <!-- Tab content -->
    {#if activeTab === 'uploads'}
      {#if uploads.length === 0}
        <div class="text-center py-16">
          <div class="w-16 h-16 mx-auto mb-4 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center">
            <svg xmlns="http://www.w3.org/2000/svg" class="w-8 h-8 text-[hsl(var(--muted-foreground))]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
          </div>
          <p class="text-[hsl(var(--muted-foreground))] mb-2">{t('library.noUploads')}</p>
          <a href="/upload" class="text-sm text-[hsl(var(--primary))] hover:underline">{t('nav.upload')}</a>
        </div>
      {:else}
        <TrackList tracks={uploads} />
      {/if}

    {:else if activeTab === 'playlists'}
      {#if playlists.length === 0}
        <div class="text-center py-12">
          <p class="text-[hsl(var(--muted-foreground))]">{t('library.noPlaylists')}</p>
        </div>
      {:else}
        <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {#each playlists as playlist}
            <a href="/playlists/{playlist.id}" class="block bg-[hsl(var(--card))] rounded-lg p-4 hover:bg-[hsl(var(--secondary))] transition group">
              <div class="aspect-square rounded-md bg-[hsl(var(--secondary))] mb-3 flex items-center justify-center text-4xl group-hover:scale-[1.02] transition-transform">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-10 h-10 text-[hsl(var(--muted-foreground))]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15V6"/><path d="M18.5 18a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z"/><path d="M12 12H3"/><path d="M16 6H3"/><path d="M12 18H3"/></svg>
              </div>
              <h3 class="text-sm font-medium truncate">{playlist.name}</h3>
              <p class="text-xs text-[hsl(var(--muted-foreground))]">{playlist.track_count ?? 0} {t('explore.tracks')}</p>
            </a>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'albums'}
      {#if albums.length === 0}
        <div class="text-center py-12">
          <p class="text-[hsl(var(--muted-foreground))]">{t('library.noAlbums')}</p>
        </div>
      {:else}
        <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4">
          {#each albums as album}
            <AlbumCard {album} />
          {/each}
        </div>
      {/if}
    {/if}
  {/if}
</div>
