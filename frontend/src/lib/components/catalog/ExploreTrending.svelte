<script lang="ts">
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "./CatalogState.svelte";

  const periods = [
    ["1h", "explore.trendingHour"],
    ["24h", "explore.trendingDay"],
    ["7d", "explore.trendingWeek"],
  ] as const;
  let period = $state<"1h" | "24h" | "7d">("24h");
  const trending = catalogQuery(signal => homeApi.trendingTracks(10, period, signal));
  const popular = catalogQuery(signal => !trending.loading && !trending.data?.length ? homeApi.popularTracks(8, signal) : null);
  const showPopular = $derived(!trending.loading && !trending.data?.length && Boolean(popular.data?.data.length));

  function retryFailed() {
    if (trending.failed) trending.retry();
    else if (popular.failed) popular.retry();
  }
</script>

<section class="space-y-4">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <h2 class="text-xl font-semibold tracking-tight">{t(showPopular ? "explore.popularTracks" : "explore.trending")}</h2>
    <div role="group" aria-label={t("explore.trending")} class="inline-flex gap-0.5 rounded-lg bg-secondary p-1">
      {#each periods as [value, label] (value)}
        <button type="button" class="rounded-md px-3 py-1.5 text-xs font-medium aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-sm text-muted-foreground"
          aria-pressed={value === period} onclick={() => period = value}>{t(label)}</button>
      {/each}
    </div>
  </div>

  {#if trending.data?.length}
    <TrackList source="explore" tracks={trending.data} />
  {:else if showPopular}
    {#if trending.failed}
      <div role="status" class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
        {t("catalog.partial")}
        <button type="button" class="font-medium underline underline-offset-2" onclick={trending.retry}>{t("radio.tryAgain")}</button>
      </div>
    {/if}
    <TrackList source="explore" tracks={popular.data?.data ?? []} />
  {:else}
    <CatalogState loading={trending.loading || popular.loading} failed={trending.failed || popular.failed}
      status={trending.status ?? popular.status} retry={retryFailed} message={t("explore.noTracks")} />
  {/if}
</section>
