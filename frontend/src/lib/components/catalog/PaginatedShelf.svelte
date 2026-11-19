<script lang="ts" generics="Item extends { id: string }">
  import { currentCatalogPage, selectCatalogPage } from "$lib/catalog/pagination";
  import type { Snippet } from "svelte";
  import { api } from "$lib/api";
  import type { PaginatedResponse } from "$lib/types";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import CatalogState from "./CatalogState.svelte";
  import CatalogHeader from "./CatalogHeader.svelte";
  import RecordShelf from "./RecordShelf.svelte";
  import PageControls from "./PageControls.svelte";

  let { title, empty, path, card }: {
    title: string; empty: string; path: string; card: Snippet<[Item]>;
  } = $props();

  const page = $derived(currentCatalogPage());
  const result = catalogQuery(() => api.get<PaginatedResponse<Item>>(`${path}?page=${page}&per_page=50`));
</script>

<div class="space-y-7">
  <CatalogHeader {title} />
  {#if result.data?.data.length}
    <RecordShelf>
      {#each result.data.data as item (item.id)}{@render card(item)}{/each}
    </RecordShelf>
    <PageControls {page} total={result.data.total_pages} select={selectCatalogPage} />
  {:else}
    <CatalogState loading={result.loading} failed={result.failed} status={result.status} message={empty} retry={result.retry} />
  {/if}
</div>
