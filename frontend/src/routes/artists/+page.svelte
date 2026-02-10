<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Artist } from "$lib/types";
  import ArtistCard from "$lib/components/ArtistCard.svelte";
  import { t } from "$lib/i18n/index.svelte";

  let artists: Artist[] = $state([]);
  let loading = $state(true);
  let page = $state(1);
  let totalPages = $state(1);

  onMount(() => loadArtists());

  async function loadArtists() {
    loading = true;
    try {
      const res = await api.get<{ data: Artist[]; total_pages: number }>(`/artists?page=${page}&per_page=50`);
      artists = res.data ?? [];
      totalPages = res.total_pages ?? 1;
    } catch (e) { console.error('Failed to load artists:', e); } finally { loading = false; }
  }

  function prevPage() { if (page > 1) { page--; loadArtists(); } }
  function nextPage() { if (page < totalPages) { page++; loadArtists(); } }
</script>

<svelte:head><title>{t('explore.artists')} — SoundTime</title></svelte:head>

<div class="space-y-6">
  <h1 class="text-2xl font-bold">{t('explore.artists')}</h1>

  {#if loading}
    <div class="flex justify-center py-20">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else if artists.length > 0}
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
      {#each artists as artist}
        <ArtistCard {artist} />
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
    <p class="text-center text-[hsl(var(--muted-foreground))] py-16">{t('explore.noArtists')}</p>
  {/if}
</div>
