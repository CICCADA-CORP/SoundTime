<script lang="ts">
  import { page } from "$app/stores";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { SearchResults } from "$lib/types";
  import TrackList from "$lib/components/TrackList.svelte";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";

  let results: SearchResults = { tracks: [], albums: [], artists: [], total: 0 };
  let loading = true;
  let query = "";

  $: query = $page.url.searchParams.get("q") ?? "";

  onMount(async () => {
    await doSearch();
  });

  $: if (query) doSearch();

  async function doSearch() {
    if (!query) { loading = false; return; }
    loading = true;
    try {
      results = await api.get<SearchResults>(`/search?q=${encodeURIComponent(query)}`);
    } catch { /* empty */ } finally { loading = false; }
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

    {#if results.tracks.length === 0 && results.albums.length === 0 && results.artists.length === 0}
      <p class="text-[hsl(var(--muted-foreground))] text-center py-16">No results found for "{query}".</p>
    {/if}
  {/if}
</div>
