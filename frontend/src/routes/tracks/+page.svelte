<script lang="ts">
  import { currentCatalogPage, selectCatalogPage } from "$lib/catalog/pagination";
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import type { PaginatedResponse, Track } from "$lib/types";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import PageControls from "$lib/components/catalog/PageControls.svelte";
  import { t } from "$lib/i18n/index.svelte";

  const page = $derived(currentCatalogPage());
  const result = catalogQuery(signal => api.get<PaginatedResponse<Track>>(`/tracks?page=${page}&per_page=50`, signal));
</script>

<svelte:head><title>{t("explore.tracks")} — SoundTime</title></svelte:head>

<div class="space-y-7">
  <CatalogHeader title={t("explore.tracks")} />
  {#if result.data?.data.length}
    <TrackList tracks={result.data.data} />
    <PageControls {page} total={result.data.total_pages} select={selectCatalogPage} />
  {:else}
    <CatalogState loading={result.loading} failed={result.failed} status={result.status} message={t("explore.noTracks")} retry={result.retry} />
  {/if}
</div>
