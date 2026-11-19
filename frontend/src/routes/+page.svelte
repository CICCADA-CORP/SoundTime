<script lang="ts">
  import { RefreshCw } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import { showcaseTracks } from "$lib/showcase";
  import CatalogSections from "$lib/components/home/CatalogSections.svelte";
  import FeaturedSession from "$lib/components/home/FeaturedSession.svelte";
  import PicksShelf from "$lib/components/home/PicksShelf.svelte";
  import { createCatalog } from "$lib/components/home/catalog.svelte";
  import { homeCopy } from "$lib/components/home/copy";

  const catalog = createCatalog();
  let demoSelected = $state(false);
  const copy = $derived(homeCopy());
  const featured = $derived(catalog.editorial.data?.find((playlist) => playlist.tracks.length));
  const tracks = $derived(catalog.offline ? showcaseTracks : (featured?.tracks ?? catalog.popular.data ?? []));
  const picks = $derived(catalog.offline ? showcaseTracks : (
    catalog.picks.data?.length ? catalog.picks.data : tracks.slice(0, 4)
  ));
  const session = $derived(tracks.length ? tracks : picks);
  const title = $derived(catalog.offline ? "After Hours" : (featured?.name ?? copy.rotation));
  const empty = $derived(!session.length && !catalog.incomplete && !catalog.loading &&
    !catalog.stats.data?.total_tracks && !catalog.albums.data?.length &&
    !catalog.artists.data?.length && !catalog.editorial.data?.length);
</script>

<svelte:head><title>{copy.title} — SoundTime</title></svelte:head>

{#if catalog.offline && !demoSelected}
  <section>
    <h1 class="text-2xl font-semibold">{copy.title}</h1>
    <p class="my-4 text-sm text-muted-foreground">{t("access.retryHint")}</p>
    <div class="flex flex-wrap gap-2">
      <Button onclick={catalog.retry}>{t("access.retry")}</Button>
      <Button variant="outline" onclick={() => demoSelected = true}>{t("access.openDemo")}</Button>
    </div>
  </section>
{:else}
<FeaturedSession {title} tracks={session} source={featured ? "playlist" : "collection"} {empty} editorial={!!featured} />

<div class="browser">
  <PicksShelf tracks={picks} {catalog}>
    <CatalogSections {catalog} />
  </PicksShelf>
</div>

{#if catalog.offline}
  <button type="button" class="catalog-status" onclick={catalog.retry} title={copy.offline} aria-label={copy.offline}>
    <RefreshCw size={13} />{copy.demo}
  </button>
{:else if catalog.incomplete && !catalog.loading}
  <div class="catalog-status" role="status">
    {copy.incomplete}
    <button type="button" class="font-medium underline underline-offset-2" onclick={catalog.retry}>{copy.retry}</button>
  </div>
{/if}
{/if}

<style>
  .browser { margin-top: 2rem; }
  .catalog-status { display: flex; align-items: center; gap: .45rem; margin-top: 1rem; color: hsl(var(--muted-foreground)); font-size: .75rem; }
  button.catalog-status:hover { color: hsl(var(--foreground)); }
  @media (max-width: 767px) { .browser { margin-top: 1.4rem; } }
</style>
