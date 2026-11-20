<script lang="ts">
  import { homeApi } from "$lib/api";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { homeCopy } from "$lib/components/home/copy";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "./CatalogState.svelte";

  const periods = [
    ["1h", "explore.trendingHour"],
    ["24h", "explore.trendingDay"],
    ["7d", "explore.trendingWeek"],
    ["all", "explore.popularTracks"],
  ] as const;
  const period = $derived(periods.find(([id]) => id === page.url.searchParams.get("period"))?.[0] ?? "24h");
  const trending = catalogQuery(signal => period === "all" ? null : homeApi.trendingTracks(10, period, signal));
  const popular = catalogQuery(signal => period === "all" ? homeApi.popularTracks(50, signal)
    : !trending.loading && !trending.data?.length ? homeApi.popularTracks(8, signal) : null);
  const showPopular = $derived(!trending.loading && !trending.data?.length && Boolean(popular.data?.data.length));

  function retryFailed() {
    if (trending.failed) trending.retry();
    else if (popular.failed) popular.retry();
  }
</script>

<section class="space-y-4">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <h2 class="trend-title">{period === "all" ? homeCopy().topTracks : t(showPopular ? "explore.popularTracks" : "explore.trending")}</h2>
    <div role="group" aria-label={t("explore.trending")} class="inline-flex gap-0.5 rounded-lg bg-secondary p-1">
      {#each periods as [value, label] (value)}
        <button type="button" class="rounded-lg px-3 py-1.5 text-xs font-medium aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-sm text-muted-foreground"
          aria-pressed={value === period} onclick={() => goto("/explore?tab=trending&period=" + value, { reset: false })}>{value === "all" ? homeCopy().allTime : t(label)}</button>
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

<style>
  .trend-title { font-size: 1.3rem; font-weight: 630; line-height: 1.2; letter-spacing: -.035em; }
</style>
