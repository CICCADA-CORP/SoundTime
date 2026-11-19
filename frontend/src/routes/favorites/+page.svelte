<script lang="ts">
  import { signInLink } from "$lib/access/navigation";
  import { page } from "$app/state";
  import { currentCatalogPage, selectCatalogPage } from "$lib/catalog/pagination";
  import { api } from "$lib/api";
  import { favoriteQuery } from "$lib/catalog/favorites.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { PaginatedResponse, Track } from "$lib/types";
  import { Button } from "$lib/components/ui/button";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import PageControls from "$lib/components/catalog/PageControls.svelte";

  const auth = getAuthStore();
  const current = $derived(currentCatalogPage());
  const favorites = favoriteQuery(() => auth.isAuthenticated ? api.get<PaginatedResponse<Track>>(`/favorites?page=${current}&per_page=100`) : null);
</script>

<svelte:head><title>{t("favorites.title")} — SoundTime</title></svelte:head>

<div class="space-y-6">
  <CatalogHeader title={t("favorites.title")} description={favorites.data ? t("favorites.count", { count: favorites.data.total }) : undefined} />
  {#if !auth.isAuthenticated}
    <CatalogState message={t("library.signInPrompt")}><Button href={signInLink(page.url)}>{t("auth.signIn")}</Button></CatalogState>
  {:else if favorites.data?.data.length}
    <TrackList tracks={favorites.data.data} />
    <PageControls page={current} total={favorites.data.total_pages} select={selectCatalogPage} />
  {:else}
    <CatalogState loading={favorites.loading} failed={favorites.failed} status={favorites.status} retry={favorites.retry} message={t("favorites.empty")} />
  {/if}
</div>
