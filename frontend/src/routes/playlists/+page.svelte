<script lang="ts">
  import { currentCatalogPage, selectCatalogPage } from "$lib/catalog/pagination";
  import { Trash2 } from "lucide-svelte";
  import { api } from "$lib/api";
  import { invalidatePlaylists, playlistQuery } from "$lib/catalog/playlists.svelte";
  import type { Playlist, PaginatedResponse } from "$lib/types";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import RecordShelf from "$lib/components/catalog/RecordShelf.svelte";
  import PlaylistCard from "$lib/components/catalog/PlaylistCard.svelte";
  import CreatePlaylistDialog from "$lib/components/catalog/CreatePlaylistDialog.svelte";
  import PageControls from "$lib/components/catalog/PageControls.svelte";

  const auth = getAuthStore();
  const page = $derived(currentCatalogPage());
  const playlists = playlistQuery(() => api.get<PaginatedResponse<Playlist>>(`/playlists?page=${page}&per_page=50`));
  let removing = $state<string | null>(null);
  let error = $state("");

  async function remove(id: string) {
    if (removing || !confirm(t("playlists.deleteConfirm"))) return;
    removing = id;
    error = "";
    try {
      await api.delete(`/playlists/${id}`);
      invalidatePlaylists();
      if (playlists.data?.data.length === 1 && page > 1) void selectCatalogPage(page - 1);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : t("common.error");
    } finally {
      removing = null;
    }
  }
</script>

<svelte:head><title>{t("playlists.title")} — SoundTime</title></svelte:head>

<div class="space-y-7">
  <CatalogHeader title={t("playlists.title")}>
    {#snippet actions()}
      {#if auth.isAuthenticated}
        <CreatePlaylistDialog />
      {/if}
    {/snippet}
  </CatalogHeader>
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
  {#if playlists.data?.data.length}
    <RecordShelf>
      {#each playlists.data.data as playlist (playlist.id)}
        <div class="group relative">
          <PlaylistCard {playlist} />
          {#if auth.user && (playlist.user_id ?? playlist.owner_id) === auth.user.id}
            <button class="absolute left-2 top-2 grid size-8 place-items-center rounded-full bg-background text-destructive shadow-sm sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100"
              aria-label={t("common.delete") + " " + playlist.name} disabled={!!removing} onclick={() => remove(playlist.id)}><Trash2 size={14} /></button>
          {/if}
        </div>
      {/each}
    </RecordShelf>
    <PageControls {page} total={playlists.data.total_pages} select={selectCatalogPage} />
  {:else}
    <CatalogState loading={playlists.loading} failed={playlists.failed} status={playlists.status} retry={playlists.retry} message={t("playlists.noPlaylists")} />
  {/if}
</div>
