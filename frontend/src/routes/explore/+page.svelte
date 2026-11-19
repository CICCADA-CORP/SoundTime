<script lang="ts">
  import { ArrowUpRight, Shuffle } from "lucide-svelte";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import RecordShelf from "$lib/components/catalog/RecordShelf.svelte";
  import BrowseLinks from "$lib/components/catalog/BrowseLinks.svelte";
  import ExploreGenres from "$lib/components/catalog/ExploreGenres.svelte";
  import ExploreTrending from "$lib/components/catalog/ExploreTrending.svelte";
  import ExploreEditorial from "$lib/components/catalog/ExploreEditorial.svelte";

  const popular = catalogQuery(() => homeApi.popularTracks(10));
  const albums = catalogQuery(() => homeApi.recentAlbums(10));
  const artists = catalogQuery(() => homeApi.topArtists(10));
  const editorial = catalogQuery(() => homeApi.editorialPlaylists());
  const genres = catalogQuery(() => homeApi.genres());
  const stats = catalogQuery(() => homeApi.statsOverview());
  const feeds = [popular, albums, artists, editorial, genres, stats];
  const queue = getQueueStore();
  let busy = $state(false);
  let shuffleError = $state(false);
  let loading = $derived(feeds.some(feed => feed.loading));
  let failed = $derived(feeds.some(feed => feed.failed));
  let hasContent = $derived(!!(popular.data?.data.length || albums.data?.data.length ||
    artists.data?.data.length || editorial.data?.length || genres.data?.length || stats.data?.total_tracks));

  function retryFailed() {
    for (const feed of feeds) if (feed.failed) feed.retry();
  }

  async function shuffle() {
    busy = true;
    shuffleError = false;
    try {
      const selection = await homeApi.randomTracks(20);
      if (selection.length) queue.playQueue(selection, 0, "explore");
      else shuffleError = true;
    } catch {
      shuffleError = true;
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head><title>{t("explore.title")} — SoundTime</title></svelte:head>

<div class="space-y-8">
  <CatalogHeader title={t("explore.title")} description={t("explore.subtitle")}>
    {#snippet actions()}
      {#if hasContent}<Button onclick={shuffle} disabled={busy}><Shuffle size={16} />{t("explore.surpriseMe")}</Button>{/if}
    {/snippet}
  </CatalogHeader>
  {#if shuffleError}<p role="alert" class="text-sm text-destructive">{t("explore.noTracks")}</p>{/if}
  <BrowseLinks />
  {#if !hasContent}
    <CatalogState {loading} {failed} retry={retryFailed} message={t("explore.empty")} />
  {:else if failed && !loading}
    <div role="status" class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
      {t("catalog.partial")} <button class="font-medium underline underline-offset-2" onclick={retryFailed}>{t("radio.tryAgain")}</button>
    </div>
  {/if}
  {#if albums.data?.data.length}
    <section class="space-y-4">
      <div class="flex items-center justify-between">
        <h2 class="text-xl font-semibold tracking-tight">{t("explore.recentAlbums")}</h2>
        <a href="/albums" class="inline-flex items-center gap-1 text-sm text-primary">{t("explore.viewAll")} <ArrowUpRight size={15} /></a>
      </div>
      <RecordShelf>{#each albums.data.data as album (album.id)}<AlbumCard {album} />{/each}</RecordShelf>
    </section>
  {/if}
  {#if genres.data?.length}<ExploreGenres genres={genres.data} />{/if}
  {#if editorial.data?.length}<ExploreEditorial playlists={editorial.data} />{/if}
  {#if hasContent}<ExploreTrending popular={popular.data?.data ?? []} />{/if}
  {#if artists.data?.data.length}
    <section class="space-y-4">
      <div class="flex items-center justify-between">
        <h2 class="text-xl font-semibold tracking-tight">{t("explore.discoverArtists")}</h2>
        <a href="/artists" class="inline-flex items-center gap-1 text-sm text-primary">{t("explore.viewAll")} <ArrowUpRight size={15} /></a>
      </div>
      <RecordShelf>{#each artists.data.data as artist (artist.id)}<ArtistCard {artist} />{/each}</RecordShelf>
    </section>
  {/if}
  {#if stats.data}
    <section class="space-y-4 border-t border-border pt-6">
      <h2 class="text-xl font-semibold tracking-tight">{t("explore.fromTheNetwork")}</h2>
      <dl class="grid grid-cols-2 gap-4 sm:grid-cols-4">
        {#each [[stats.data.total_tracks, t("stats.totalTracks")], [stats.data.total_albums, t("stats.totalAlbums")], [stats.data.total_artists, t("stats.totalArtists")], [stats.data.peer_count, t("stats.peers")]] as [value, label] (label)}
          <div><dt class="text-sm text-muted-foreground">{label}</dt><dd class="mt-1 text-xl font-semibold tabular-nums">{value}</dd></div>
        {/each}
      </dl>
    </section>
  {/if}
</div>
