<script lang="ts">
  import { page } from "$app/state";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import { api } from "$lib/api";
  import type { SearchResults } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import RecordShelf from "$lib/components/catalog/RecordShelf.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";

  import NetworkSearch from "$lib/components/catalog/NetworkSearch.svelte";

  const query = $derived(page.url.searchParams.get("q")?.trim() ?? "");
  const search = catalogQuery(() => query
    ? api.get<SearchResults>(`/search?q=${encodeURIComponent(query)}`)
    : null);
  const results = $derived(search.data);
</script>

<svelte:head><title>{t("common.search")} — SoundTime</title></svelte:head>

<div class="space-y-8">
  <CatalogHeader title={query ? t("shell.searchResults", { query }) : t("common.search")} />

  {#if search.loading}
    <CatalogState loading />
  {:else if search.failed}
    <CatalogState failed status={search.status} retry={search.retry} />
  {:else if !query}
    <CatalogState message={t("search.placeholder")} />
  {:else if results}
    {#if results.tracks.length > 0}
    <section>
      <h2 class="mb-3 text-lg font-semibold tracking-tight">{t("artist.tracks")}</h2>
      <TrackList source="search" tracks={results.tracks} />
    </section>
    {/if}

    {#if results.albums.length > 0}
    <section>
      <h2 class="mb-3 text-lg font-semibold tracking-tight">{t("library.albums")}</h2>
      <RecordShelf>
        {#each results.albums as album (album.id)}
          <AlbumCard {album} />
        {/each}
      </RecordShelf>
    </section>
    {/if}

    {#if results.artists.length > 0}
    <section>
      <h2 class="mb-3 text-lg font-semibold tracking-tight">{t("shell.artists")}</h2>
      <RecordShelf>
        {#each results.artists as artist (artist.id)}
          <ArtistCard {artist} />
        {/each}
      </RecordShelf>
    </section>
    {/if}

    {#if results.total === 0}
      <CatalogState message={t("common.noResults")} />
    {/if}
  {/if}
  {#if query}{#key query}<NetworkSearch {query} />{/key}{/if}
</div>
