<script lang="ts">
  import { page } from "$app/stores";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { SearchResults } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";

  let results: SearchResults = $state({ tracks: [], albums: [], artists: [], total: 0 });
  let loading = $state(true);

  let query = $derived($page.url.searchParams.get("q") ?? "");

  onMount(async () => {
    await doSearch();
  });

  $effect(() => {
    if (query) doSearch();
  });

  async function doSearch() {
    if (!query) { loading = false; return; }
    loading = true;
    try {
      results = await api.get<SearchResults>(`/search?q=${encodeURIComponent(query)}&include_p2p=true`);
    } catch (e) { console.error('Search failed:', e); } finally { loading = false; }
  }
</script>

<svelte:head><title>Search: {query} — SoundTime</title></svelte:head>

<div class="space-y-8">
  <h1 class="text-2xl font-bold">
    {#if query}Results for "{query}"{:else}Search{/if}
  </h1>

  {#if loading}
    <div class="flex justify-center py-20">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else if !query}
    <p class="text-[hsl(var(--muted-foreground))] text-center py-10">Type something to search...</p>
  {:else}
    {#if results.tracks.length > 0}
    <section>
      <h2 class="text-xl font-semibold mb-3">Tracks</h2>
      <TrackList tracks={results.tracks} />
    </section>
    {/if}

    {#if results.albums.length > 0}
    <section>
      <h2 class="text-xl font-semibold mb-3">Albums</h2>
      <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
        {#each results.albums as album}
          <AlbumCard {album} />
        {/each}
      </div>
    </section>
    {/if}

    {#if results.artists.length > 0}
    <section>
      <h2 class="text-xl font-semibold mb-3">Artists</h2>
      <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
        {#each results.artists as artist}
          <ArtistCard {artist} />
        {/each}
      </div>
    </section>
    {/if}

    {#if results.p2p_results && results.p2p_results.length > 0}
    <section>
      <div class="flex items-center gap-2 mb-3">
        <h2 class="text-xl font-semibold">{t('search.p2pResults')}</h2>
        <span class="text-xs bg-purple-500/20 text-purple-400 px-2 py-0.5 rounded-full">{t('search.p2pBadge')}</span>
      </div>
      <div class="overflow-x-auto">
        <table class="w-full text-sm">
          <thead>
            <tr class="border-b border-[hsl(var(--border))]">
              <th class="p-3 text-left font-medium text-[hsl(var(--muted-foreground))]">{t('track.title')}</th>
              <th class="p-3 text-left font-medium text-[hsl(var(--muted-foreground))]">{t('track.artist')}</th>
              <th class="p-3 text-left font-medium text-[hsl(var(--muted-foreground))]">{t('track.album')}</th>
              <th class="p-3 text-left font-medium text-[hsl(var(--muted-foreground))]">{t('track.format')}</th>
              <th class="p-3 text-left font-medium text-[hsl(var(--muted-foreground))]">{t('track.bitrate')}</th>
              <th class="p-3 text-left font-medium text-[hsl(var(--muted-foreground))]">{t('player.source')}</th>
            </tr>
          </thead>
          <tbody>
            {#each results.p2p_results as result}
              <tr class="border-b border-[hsl(var(--border))]">
                <td class="p-3 font-medium">{result.title}</td>
                <td class="p-3 text-[hsl(var(--muted-foreground))]">{result.artist_name}</td>
                <td class="p-3 text-[hsl(var(--muted-foreground))]">{result.album_title ?? '—'}</td>
                <td class="p-3 text-xs uppercase">{result.format}</td>
                <td class="p-3">{result.bitrate ? `${result.bitrate}k` : '—'}</td>
                <td class="p-3 font-mono text-xs">{result.source_node.slice(0, 8)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
    {/if}

    {#if results.tracks.length === 0 && results.albums.length === 0 && results.artists.length === 0 && (!results.p2p_results || results.p2p_results.length === 0)}
      <p class="text-[hsl(var(--muted-foreground))] text-center py-16">No results found for "{query}".</p>
    {/if}
  {/if}
</div>
