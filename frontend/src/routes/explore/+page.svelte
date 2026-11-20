<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { Tabs } from "bits-ui";
  import { ArrowRight, ChevronRight, Search } from "lucide-svelte";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import ExploreEditorial from "$lib/components/catalog/ExploreEditorial.svelte";
  import ExploreGenres from "$lib/components/catalog/ExploreGenres.svelte";
  import ExploreRecent from "$lib/components/catalog/ExploreRecent.svelte";
  import ExploreTrending from "$lib/components/catalog/ExploreTrending.svelte";
  import RecordShelf from "$lib/components/catalog/RecordShelf.svelte";
  import SectionHeading from "$lib/components/catalog/SectionHeading.svelte";

  const tabs = [
    ["new", "explore.newInCatalog"], ["trending", "explore.trending"], ["artists", "explore.artists"],
    ["genres", "explore.genres"], ["playlists", "nav.playlists"],
  ] as const;
  const active = $derived(tabs.find(([id]) => id === page.url.searchParams.get("tab"))?.[0] ?? "new");
  const artists = catalogQuery(signal => active === "artists" ? homeApi.topArtists(8, signal) : null);
  const genres = catalogQuery(signal => active === "genres" ? homeApi.genres(signal) : null);
  const editorial = catalogQuery(signal => active === "playlists" ? homeApi.editorialPlaylists(signal) : null);
  let query = $state("");

  function search(event: SubmitEvent) {
    event.preventDefault();
    if (query.trim()) void goto("/search?q=" + encodeURIComponent(query.trim()));
  }
</script>

<svelte:head><title>{t("explore.title")} — SoundTime</title></svelte:head>

<div class="explore">
  <div class="explore-heading">
    <CatalogHeader title={t("explore.title")} />
    <form class="catalog-search" role="search" action="/search" method="GET" onsubmit={search}>
      <Search size={19} strokeWidth={1.7} aria-hidden="true" />
      <input name="q" type="search" bind:value={query} aria-label={t("explore.searchCatalog")}
        placeholder={t("explore.searchHint")} autocomplete="off" enterkeyhint="search" required />
      <button type="submit" aria-label={t("common.search")} disabled={!query.trim()}><ArrowRight size={17} /></button>
    </form>
  </div>

  <Tabs.Root value={active} onValueChange={(id) => { if (id !== active) void goto("/explore?tab=" + encodeURIComponent(id), { reset: false }); }}>
    <Tabs.List class="explore-tabs" aria-label={t("explore.title")}>
      {#each tabs as [id, label] (id)}
        <Tabs.Trigger class="explore-tab" value={id}>{t(label)}</Tabs.Trigger>
      {/each}
    </Tabs.List>

    <Tabs.Content class="explore-panel" value="new" tabindex={-1}>{#if active === "new"}<ExploreRecent />{/if}</Tabs.Content>
    <Tabs.Content class="explore-panel" value="trending" tabindex={-1}>
      {#if active === "trending"}<ExploreTrending />{/if}
    </Tabs.Content>
    <Tabs.Content class="explore-panel" value="artists" tabindex={-1}>
      <SectionHeading title={t("explore.discoverArtists")} href="/artists" linkLabel={t("explore.allArtists")} />
      {#if artists.data?.data.length}
        <RecordShelf>{#each artists.data.data as artist (artist.id)}<ArtistCard {artist} />{/each}</RecordShelf>
      {:else}
        <CatalogState layout="shelf" loading={artists.loading} failed={artists.failed} status={artists.status} retry={artists.retry} message={t("explore.noArtists")} />
      {/if}
    </Tabs.Content>
    <Tabs.Content class="explore-panel" value="genres" tabindex={-1}>
      {#if genres.data?.length}<ExploreGenres genres={genres.data} />
      {:else}<CatalogState loading={genres.loading} failed={genres.failed} status={genres.status} retry={genres.retry} message={t("explore.emptyGenres")} />{/if}
    </Tabs.Content>
    <Tabs.Content class="explore-panel" value="playlists" tabindex={-1}>
      {#if editorial.data?.length}<ExploreEditorial playlists={editorial.data} />
      {:else}<CatalogState layout="shelf" loading={editorial.loading} failed={editorial.failed} status={editorial.status} retry={editorial.retry} message={t("explore.emptySelections")} />{/if}
      <a class="more mt-4" href="/playlists">{t("explore.allPlaylists")} <ChevronRight size={15} /></a>
    </Tabs.Content>
  </Tabs.Root>
</div>

<style>
  .explore { width: 100%; min-width: 0; container-type: inline-size; }
  .explore-heading { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 16px 32px; margin-bottom: 24px; }
  .catalog-search { width: min(100%, 22rem); display: flex; align-items: center; gap: 10px; background: hsl(var(--card) / .52); border: 1px solid hsl(var(--border)); border-radius: var(--radius-control, .65rem); padding: 0 5px 0 12px; color: hsl(var(--muted-foreground)); }
  .catalog-search:focus-within { outline: 2px solid hsl(var(--ring)); outline-offset: 2px; }
  .catalog-search input { flex: 1; min-width: 0; height: 42px; background: transparent; color: hsl(var(--foreground)); outline: none; font-size: .9375rem; }
  .catalog-search button { display: grid; place-items: center; width: 34px; height: 34px; border-radius: .45rem; background: hsl(var(--foreground)); color: hsl(var(--background)); }
  .catalog-search button:disabled { display: none; }
  .explore :global(.explore-tabs) { display: flex; width: fit-content; max-width: 100%; gap: 3px; padding: 3px; overflow-x: auto;
    border-radius: var(--radius-control, .65rem); background: hsl(var(--muted) / .65); }
  .explore :global(.explore-tab) { flex: none; padding: .45rem .75rem; border-radius: calc(var(--radius-control, .65rem) - 2px); color: hsl(var(--muted-foreground)); font-size: .875rem; font-weight: 540; }
  .explore :global(.explore-tab[data-state="active"]) { background: hsl(var(--card) / .86); color: hsl(var(--foreground)); box-shadow: 0 1px 4px hsl(var(--foreground) / .1); }
  .explore :global(.explore-tab:focus-visible) { outline-offset: -3px; }
  .explore :global(.explore-panel) { padding-top: 24px; }
  .more { display: inline-flex; align-items: center; gap: 2px; color: hsl(var(--primary)); font-size: .8125rem; }
  .more:hover { text-decoration: underline; color: hsl(var(--foreground)); }
  @container (max-width: 480px) {
    .explore :global(.explore-tabs) { gap: 0; }
    .explore :global(.explore-tab) { font-size: .78rem; padding: .5rem .55rem; }
  }
  @container (max-width: 600px) {
    .explore-heading { display: grid; grid-template-columns: minmax(0, 1fr); gap: 14px; margin-bottom: 16px; }
    .catalog-search { width: 100%; }
    .catalog-search input { font-size: 1rem; }
    .explore :global(.explore-panel) { padding-top: 18px; }
  }
</style>
