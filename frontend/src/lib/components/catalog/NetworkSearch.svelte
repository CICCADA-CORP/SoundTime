<script lang="ts">
  import { apiFetch } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { NetworkSearchResponse, Track } from "$lib/types";
  import TrackList from "../TrackList.svelte";
  import { Button } from "../ui/button";
  import CatalogState from "./CatalogState.svelte";

  let { query }: { query: string } = $props();
  let requested = $state(false);
  const network = catalogQuery(signal => requested
    ? apiFetch<NetworkSearchResponse>(`/p2p/search?q=${encodeURIComponent(query)}`, { signal })
    : null);
  let available = $derived.by(() => {
    const tracks = new Map<string, Track>();
    for (const { track } of network.data?.results ?? []) {
      if (track) tracks.set(track.id, track);
    }
    return [...tracks.values()];
  });
  let hints = $derived(network.data?.results.filter(hit => !hit.track) ?? []);
</script>

<section aria-label={t("search.p2pResults")} class="space-y-3">
  {#if !requested}
    <Button variant="outline" onclick={() => requested = true}>{t("search.extend")}</Button>
  {:else}
    <h2 class="text-lg font-semibold">{t("search.p2pResults")}</h2>
    {#if network.loading}
      <Button variant="ghost" onclick={() => requested = false}>{t("common.cancel")}</Button>
      <CatalogState loading />
    {:else if network.failed}
      <CatalogState failed status={network.status} retry={network.retry} />
    {:else if network.data}
      {#if available.length}<TrackList source="search" tracks={available} />{/if}
      {#if hints.length}
        <p class="text-sm text-muted-foreground">{t("search.unverified")}</p>
        <ul class="divide-y divide-border">
          {#each hints as hit (JSON.stringify([hit.source_node, hit.authority_node, hit.publication_id, hit.revision, hit.hash]))}
            <li class="py-2 text-sm"><span class="font-medium">{hit.title}</span> · {hit.artist_name}
              {#if hit.album_title}<span class="text-muted-foreground"> · {hit.album_title}</span>{/if}
            </li>
          {/each}
        </ul>
      {:else if !available.length}<CatalogState message={t("common.noResults")} />{/if}
    {/if}
  {/if}
</section>
