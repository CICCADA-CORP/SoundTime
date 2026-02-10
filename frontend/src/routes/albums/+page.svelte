<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Album } from "$lib/types";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import { t } from "$lib/i18n/index.svelte";

  let albums: Album[] = $state([]);
  let loading = $state(true);
  let page = $state(1);
  let totalPages = $state(1);

  onMount(() => loadAlbums());

  async function loadAlbums() {
    loading = true;
    try {
      const res = await api.get<{ data: Album[]; total_pages: number }>(`/albums?page=${page}&per_page=50`);
      albums = res.data ?? [];
      totalPages = res.total_pages ?? 1;
    } catch (e) { console.error('Failed to load albums:', e); } finally { loading = false; }
  }

  function prevPage() { if (page > 1) { page--; loadAlbums(); } }
  function nextPage() { if (page < totalPages) { page++; loadAlbums(); } }
</script>

<svelte:head><title>{t('explore.albums')} — SoundTime</title></svelte:head>

<div class="space-y-6">
  <h1 class="text-2xl font-bold">{t('explore.albums')}</h1>

  {#if loading}
    <div class="flex justify-center py-20">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else if albums.length > 0}
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
      {#each albums as album}
        <AlbumCard {album} />
      {/each}
    </div>
    {#if totalPages > 1}
      <div class="flex items-center justify-center gap-4 pt-4">
        <button class="px-4 py-2 rounded-lg bg-[hsl(var(--secondary))] text-sm disabled:opacity-50" onclick={prevPage} disabled={page <= 1}>{t('admin.reports.previous')}</button>
        <span class="text-sm text-[hsl(var(--muted-foreground))]">{page} / {totalPages}</span>
        <button class="px-4 py-2 rounded-lg bg-[hsl(var(--secondary))] text-sm disabled:opacity-50" onclick={nextPage} disabled={page >= totalPages}>{t('admin.reports.next')}</button>
      </div>
    {/if}
  {:else}
    <p class="text-center text-[hsl(var(--muted-foreground))] py-16">{t('explore.noAlbums')}</p>
  {/if}
</div>
