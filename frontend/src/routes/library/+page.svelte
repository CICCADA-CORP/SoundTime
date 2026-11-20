<script lang="ts">
  import { signInLink } from "$lib/access/navigation";
  import { currentCatalogPage, selectCatalogPage } from "$lib/catalog/pagination";
  import { page } from "$app/state";
  import { Tabs } from "bits-ui";
  import { Clock3, Heart, Plus } from "lucide-svelte";
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { ownedPlaylists } from "$lib/catalog/playlists.svelte";
  import type { Album, PaginatedResponse, Track } from "$lib/types";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import PageControls from "$lib/components/catalog/PageControls.svelte";
  import PlaylistCard from "$lib/components/catalog/PlaylistCard.svelte";
  import CreatePlaylistDialog from "$lib/components/catalog/CreatePlaylistDialog.svelte";
  import RecordShelf from "$lib/components/catalog/RecordShelf.svelte";

  const auth = getAuthStore();
  const tabs = [
    ["uploads", "library.uploads"], ["playlists", "library.ownedPlaylists"], ["albums", "library.catalogAlbums"],
  ] as const;
  const active = $derived(tabs.find(([id]) => id === page.url.searchParams.get("tab"))?.[0] ?? "uploads");
  const current = $derived(currentCatalogPage());

  const uploads = catalogQuery(signal => auth.user?.id && active === "uploads"
    ? api.get<PaginatedResponse<Track>>(`/tracks/my-uploads?page=${current}&per_page=50`, signal) : null);
  const playlists = ownedPlaylists(() => auth.user?.id && active === "playlists" ? current : null);
  const albums = catalogQuery(signal => auth.user?.id && active === "albums"
    ? api.get<PaginatedResponse<Album>>(`/albums?page=${current}&per_page=24`, signal) : null);
  const selected = $derived(({ uploads, playlists, albums })[active]);

</script>

<svelte:head><title>{t("library.title")} — SoundTime</title></svelte:head>

<div class="space-y-7">
  <CatalogHeader title={t("library.title")}>
    {#snippet actions()}
      {#if auth.isAuthenticated}
        {#if active === "playlists"}
          <CreatePlaylistDialog />
        {:else}<Button href="/upload"><Plus size={16} />{t("nav.upload")}</Button>{/if}
      {/if}
    {/snippet}
  </CatalogHeader>
  {#if !auth.isAuthenticated}
    <CatalogState message={t("library.signInPrompt")}>
      <Button href={signInLink(page.url)}>{t("nav.signIn")}</Button>
    </CatalogState>
  {:else}
    <nav aria-label={t("workspaceNav.yourMusic")} class="flex flex-wrap gap-2">
      <Button href="/favorites" variant="secondary" size="sm"><Heart size={16} />{t("nav.favorites")}</Button>
      <Button href="/history" variant="secondary" size="sm"><Clock3 size={16} />{t("nav.history")}</Button>
    </nav>
    <Tabs.Root value={active} onValueChange={(value) => selectCatalogPage(1, value)}>
      <Tabs.List aria-label={t("library.title")} class="flex max-w-full gap-4 overflow-x-auto border-b border-border">
        {#each tabs as [id, key] (id)}
          <Tabs.Trigger value={id} class="shrink-0 border-b-2 border-transparent py-2 text-sm font-medium text-muted-foreground data-[state=active]:border-primary data-[state=active]:text-foreground">{t(key)}</Tabs.Trigger>
        {/each}
      </Tabs.List>
      {#each tabs as [id] (id)}
        <Tabs.Content value={id} tabindex={-1} class="space-y-5 pt-5">
          {#if active === id}
            {#if selected.loading || selected.failed}
              <CatalogState layout={active === "uploads" ? "rows" : "shelf"} loading={selected.loading} failed={selected.failed} status={selected.status} retry={selected.retry} />
            {:else if active === "uploads" && uploads.data?.data.length}
              <TrackList tracks={uploads.data.data} />
            {:else if active === "albums" && albums.data?.data.length}
              <RecordShelf>{#each albums.data.data as album (album.id)}<AlbumCard {album} />{/each}</RecordShelf>
            {:else if active === "playlists" && playlists.data?.data.length}
              <RecordShelf>{#each playlists.data.data as playlist (playlist.id)}<PlaylistCard {playlist} />{/each}</RecordShelf>
            {:else}
              <CatalogState message={current > 1 ? t("common.noResults") : t(active === "uploads" ? "library.noUploads" : active === "albums" ? "library.noAlbums" : "library.noPlaylists")}>
                {#if current > 1}<Button variant="outline" onclick={() => selectCatalogPage(1, active)}>{t("library.firstPage")}</Button>
                {:else if active === "uploads"}<Button href="/upload">{t("nav.upload")}</Button>{/if}
              </CatalogState>
            {/if}
            {#if selected.data?.data.length}
              <PageControls page={current} total={selected.data.total_pages} select={(value) => selectCatalogPage(value, active)} />
            {/if}
          {/if}
        </Tabs.Content>
      {/each}
    </Tabs.Root>
  {/if}
</div>
