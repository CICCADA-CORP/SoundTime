<script lang="ts">
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Track } from "$lib/types";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "./CatalogState.svelte";

  let { popular }: { popular: Track[] } = $props();
  const periods = [
    ["1h", "explore.trendingHour"], ["24h", "explore.trendingDay"], ["7d", "explore.trendingWeek"],
  ] as const;
  let period = $state<"1h" | "24h" | "7d">("24h");
  const trending = catalogQuery(() => homeApi.trendingTracks(10, period));
</script>

<section class="space-y-4">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <h2 class="text-xl font-semibold tracking-tight">{t("explore.trending")}</h2>
    <div class="flex gap-0.5 rounded-lg bg-secondary p-1">
      {#each periods as [value, label] (value)}
        <button class="rounded-md px-3 py-1 text-xs font-medium {value === period ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground'}"
          aria-pressed={value === period} onclick={() => period = value}>{t(label)}</button>
      {/each}
    </div>
  </div>
  {#if trending.data?.length}
    <TrackList source="explore" tracks={trending.data} />
  {:else}
    <CatalogState loading={trending.loading} failed={trending.failed} status={trending.status} retry={trending.retry} message={t("explore.noTracks")} />
    {#if popular.length && !trending.loading}
      <h3 class="pt-4 font-semibold">{t("explore.popularTracks")}</h3>
      <TrackList source="explore" tracks={popular} />
    {/if}
  {/if}
</section>
