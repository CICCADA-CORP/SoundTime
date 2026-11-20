<script lang="ts">
  import { Shuffle } from "lucide-svelte";
  import { homeApi } from "$lib/api";
  import { catalogSection } from "$lib/catalog/query.svelte";
  import { canPlay } from "$lib/catalog/availability";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "./CatalogState.svelte";
  import CatalogLoading from "./CatalogLoading.svelte";
  import SectionHeading from "./SectionHeading.svelte";
  import ArtworkRail from "./ArtworkRail.svelte";

  const albums = catalogSection(signal => homeApi.recentAlbums(8, signal));
  const tracks = catalogSection(signal => homeApi.recentlyAdded(6, signal));
  const queue = getQueueStore();
  const lead = $derived(albums.data?.data.find(album => album.cover_url) ?? albums.data?.data[0]);
  const remaining = $derived((albums.data?.data ?? []).filter(album => album.id !== lead?.id));
  const hasMusic = $derived(Boolean(albums.data?.data.length || tracks.data?.data.length));
  let shuffling = $state(false);
  let shuffleMessage = $state<"explore.noTracks" | "catalog.unavailable" | null>(null);

  function retryFailed() {
    if (albums.failed) albums.retry();
    if (tracks.failed) tracks.retry();
  }

  async function shuffle() {
    if (shuffling) return;
    shuffling = true;
    shuffleMessage = null;
    try {
      const selection = await homeApi.randomTracks(20);
      if (selection.some(canPlay)) queue.playQueue(selection, 0, "explore");
      else shuffleMessage = "explore.noTracks";
    } catch {
      shuffleMessage = "catalog.unavailable";
    } finally {
      shuffling = false;
    }
  }
</script>

{#if albums.initialLoading || tracks.initialLoading}
  <CatalogLoading layout="shelf" mobileRow />
{:else if !hasMusic}
  <CatalogState loading={albums.loading || tracks.loading} failed={albums.failed || tracks.failed}
    retry={retryFailed} message={t("explore.emptyCatalog")} />
{:else}
  <div class="features">
    <div class="spread" class:paired={lead && tracks.data?.data.length}>
      {#if lead}
        <section class="release">
          <SectionHeading title={t("explore.featuredAlbum")} href="/albums" linkLabel={t("explore.allAlbums")} />
          <div class="lead-cover"><AlbumCard album={lead} /></div>
        </section>
      {/if}
      {#if tracks.data?.data.length}
        <section class="tracks">
          <SectionHeading title={t("explore.recentTracks")}>
            {#snippet actions()}
              <Button variant="ghost" size="icon" disabled={shuffling} aria-label={t("explore.shuffleCatalog")} title={t("explore.shuffleCatalog")} onclick={shuffle}>
                <Shuffle size={16} />
              </Button>
            {/snippet}
          </SectionHeading>
          {#if shuffleMessage}<p role="alert" class="text-sm text-destructive">{t(shuffleMessage)}</p>{/if}
          <TrackList source="explore" tracks={tracks.data.data} showAlbum={false} compact />
        </section>
      {/if}
    </div>
    {#if remaining.length}
      <section>
        <SectionHeading title={t("explore.recentAlbums")} href="/albums" />
        <ArtworkRail label={t("explore.recentAlbums")}>{#each remaining as album (album.id)}<AlbumCard {album} />{/each}</ArtworkRail>
      </section>
    {/if}
    {#if albums.failed || tracks.failed}
      <p role="status" class="text-sm text-muted-foreground">
        {t("catalog.partial")} <button class="underline underline-offset-2" onclick={retryFailed}>{t("radio.tryAgain")}</button>
      </p>
    {/if}
  </div>
{/if}

<style>
  .features { display: grid; min-width: 0; gap: 2rem; container-type: inline-size; }
  .spread { display: grid; min-width: 0; align-items: start; gap: 1.5rem; }
  .features section { min-width: 0; }
  .lead-cover { width: min(100%, 16.5rem); }
  @container (min-width: 590px) {
    .paired { grid-template-columns: minmax(0, .72fr) minmax(0, 1.32fr); gap: clamp(1.1rem, 3.4cqi, 2.4rem); }
  }
  @container (max-width: 589px) {
    .lead-cover { width: min(100%, 14rem); }
  }
</style>
