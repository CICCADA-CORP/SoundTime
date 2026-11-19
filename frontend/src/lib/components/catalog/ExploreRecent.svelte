<script lang="ts">
  import { ChevronRight, Shuffle } from "lucide-svelte";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { canPlay } from "$lib/catalog/availability";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "./CatalogState.svelte";
  import RecordShelf from "./RecordShelf.svelte";

  const albums = catalogQuery(() => homeApi.recentAlbums(4));
  const tracks = catalogQuery(() => homeApi.recentlyAdded(6));
  const queue = getQueueStore();
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

{#if !hasMusic}
  <CatalogState loading={albums.loading || tracks.loading} failed={albums.failed || tracks.failed}
    retry={retryFailed} message={t("explore.emptyCatalog")} />
{:else}
  <div class="space-y-8">
    {#if albums.data?.data.length}
      <section class="space-y-3">
        <div class="flex items-center justify-between gap-3">
          <h2 class="text-lg font-semibold tracking-tight">{t("explore.recentAlbums")}</h2>
          <a class="catalog-link" href="/albums">{t("explore.allAlbums")} <ChevronRight size={15} /></a>
        </div>
        <RecordShelf>{#each albums.data.data as album (album.id)}<AlbumCard {album} />{/each}</RecordShelf>
      </section>
    {/if}
    {#if tracks.data?.data.length}
      <section class="space-y-3">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h2 class="text-lg font-semibold tracking-tight">{t("explore.recentTracks")}</h2>
          <Button variant="ghost" size="sm" disabled={shuffling} onclick={shuffle}>
            <Shuffle size={15} />{t("explore.shuffleCatalog")}
          </Button>
        </div>
        {#if shuffleMessage}<p role="alert" class="text-sm text-destructive">{t(shuffleMessage)}</p>{/if}
        <TrackList source="explore" tracks={tracks.data.data} showAlbum={false} />
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
  .catalog-link { display: inline-flex; align-items: center; gap: 2px; font-size: 0.8125rem; color: hsl(var(--muted-foreground)); }
  .catalog-link:hover { color: hsl(var(--foreground)); text-decoration: underline; }
</style>
