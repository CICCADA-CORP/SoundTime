<script lang="ts">
  import { ChevronDown, RefreshCw } from "lucide-svelte";
  import type { Track } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import { showcaseTracks } from "$lib/showcase";
  import { canPlay } from "$lib/catalog/availability";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import SectionHeading from "$lib/components/catalog/SectionHeading.svelte";
  import CatalogSections from "$lib/components/home/CatalogSections.svelte";
  import FeaturedSession from "$lib/components/home/FeaturedSession.svelte";
  import ArtistStrip from "$lib/components/home/ArtistStrip.svelte";
  import SessionTracks from "$lib/components/home/SessionTracks.svelte";
  import PicksShelf from "$lib/components/home/PicksShelf.svelte";
  import GenreShelf from "$lib/components/home/GenreShelf.svelte";
  import ChartShelf from "$lib/components/home/ChartShelf.svelte";
  import { createCatalog } from "$lib/components/home/catalog.svelte";
  import { availableTracks } from "$lib/components/home/spotlights";
  import { homeCopy } from "$lib/components/home/copy";

  const catalog = createCatalog();
  let demoSelected = $state(false);
  let browsing = $state<boolean>();
  const catalogOpen = $derived(browsing ?? catalog.offline);
  const copy = $derived(homeCopy());
  const featured = $derived(catalog.editorial.data?.find(playlist => playlist.tracks.some(canPlay)));
  const albumCovers = $derived(new Map(catalog.albums.data?.map(album => [album.id, album.cover_url])));
  function withAlbumCover(track: Track): Track {
    const cover = track.album_id && albumCovers.get(track.album_id);
    return !track.cover_url && cover ? { ...track, cover_url: cover } : track;
  }
  const session = $derived(catalog.offline ? showcaseTracks : availableTracks([
    featured?.tracks, catalog.popular.data, catalog.recent.data, catalog.picks.data,
  ]).map(withAlbumCover));
  const isPlaylist = $derived(!!featured && session.every(track => featured?.tracks.some(item => item.id === track.id)));
  const picks = $derived(catalog.offline ? showcaseTracks : (catalog.picks.data?.length ? catalog.picks.data.map(withAlbumCover) : session.slice(0, 4)));
  const history = $derived(catalog.history.data?.map(entry => entry.track) ?? []);
  const recent = $derived((history.length ? history : (catalog.recent.data ?? []).slice(0, 5)).map(withAlbumCover));
  const empty = $derived(!session.length && !catalog.incomplete && !catalog.loading &&
    !catalog.stats.data?.total_tracks && !catalog.recent.data?.length && !catalog.albums.data?.length &&
    !catalog.artists.data?.length && !catalog.editorial.data?.length);
  const title = $derived(catalog.offline ? "After Hours" : isPlaylist && featured ? featured.name : empty ? t("home.noMusic") : copy.rotation);
</script>

<svelte:head><title>{copy.title} — SoundTime</title></svelte:head>

<div class="browse">
  <CatalogHeader title={copy.title} />
  {#if catalog.initialLoading}
    <FeaturedSession title={copy.rotation} loading />
  {:else if catalog.offline && !demoSelected}
    <section class="unavailable">
      <h2>{t("catalog.unavailable")}</h2>
      <p>{t("access.retryHint")}</p>
      <div class="flex flex-wrap gap-2">
        <Button onclick={catalog.retry}>{t("access.retry")}</Button>
        <Button variant="outline" onclick={() => demoSelected = true}>{t("access.openDemo")}</Button>
      </div>
    </section>
  {:else}
    <FeaturedSession {title} tracks={session} source={isPlaylist ? "playlist" : "collection"} {empty} playAll={isPlaylist || catalog.offline} />
    {#if catalog.offline}
      <button type="button" class="catalog-status" onclick={catalog.retry} title={copy.offline} aria-label={copy.offline}>
        <RefreshCw size={13} />{copy.demo}
      </button>
    {:else if catalog.incomplete && !catalog.loading}
      <div class="catalog-status" role="status">
        {copy.incomplete}
        <button class="underline underline-offset-2" onclick={catalog.retry}>{copy.retry}</button>
      </div>
    {/if}

    {#if catalog.artists.data?.length}<ArtistStrip artists={catalog.artists.data} albums={catalog.albums.data ?? []} tracks={session} />{/if}
    {#if recent.length}
      <section class="recent">
        <SectionHeading title={t(history.length ? "home.recentlyPlayed" : "explore.recentTracks")} href={history.length ? "/history" : "/explore?tab=new"} />
        <SessionTracks tracks={recent} source={history.length ? "collection" : "explore"} />
      </section>
    {/if}

    {#if !empty}
      <section class="collection">
        <h2><button class="disclose" aria-expanded={catalogOpen} aria-controls="home-catalog"
          onclick={() => browsing = !catalogOpen}>
          {t("workspaceNav.catalog")}<ChevronDown size={17} />
        </button></h2>
        <div id="home-catalog" hidden={!catalogOpen}>
          {#if catalogOpen}
            <PicksShelf tracks={picks} {catalog}>
              <CatalogSections {catalog} />
            </PicksShelf>
            {#if catalog.genres.data?.length}<GenreShelf genres={catalog.genres.data} />{/if}
            {#if catalog.popular.data?.length}<ChartShelf tracks={catalog.popular.data} />{/if}
          {/if}
        </div>
      </section>
    {/if}
  {/if}
</div>

<style>
  .browse { display: grid; align-content: start; min-width: 0; gap: clamp(1.35rem, 3.9cqi, 2.15rem); container-type: inline-size; }
  .browse > :global(*) { min-width: 0; }
  .browse > :global(header:first-child) { margin-bottom: -.35rem; }
  .unavailable { padding-block: .8rem; min-height: 10rem; }
  .unavailable h2 { font-weight: 620; font-size: 1.2rem; letter-spacing: -.025em; }
  .unavailable p { margin: .6rem 0 1.2rem; color: hsl(var(--muted-foreground)); }
  .catalog-status { display: flex; align-items: center; gap: .5rem; margin-top: -.7rem; color: hsl(var(--muted-foreground)); font-size: .8rem; }
  button.catalog-status:hover { color: hsl(var(--foreground)); }
  .collection { border-top: 1px solid hsl(var(--border) / .65); padding-top: .8rem; }
  .disclose { display: inline-flex; align-items: center; gap: .5rem; font-weight: 580; font-size: .9rem; min-height: 2rem; }
  .disclose[aria-expanded="true"] :global(svg) { transform: rotate(180deg); }
  #home-catalog:not([hidden]) { display: grid; margin-top: 1rem; gap: 1.5rem; }
</style>
