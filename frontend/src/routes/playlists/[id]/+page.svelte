<script lang="ts">
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { Pencil, Play, Trash2 } from "lucide-svelte";
  import { canPlay } from "$lib/catalog/availability";
  import { api } from "$lib/api";
  import { invalidatePlaylists, playlistQuery } from "$lib/catalog/playlists.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Playlist } from "$lib/types";
  import { Button } from "$lib/components/ui/button";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import PlaylistForm from "$lib/components/catalog/PlaylistForm.svelte";
  import ReleaseHeader from "$lib/components/catalog/ReleaseHeader.svelte";

  const auth = getAuthStore();
  const queue = getQueueStore();
  const playlist = playlistQuery(() => api.get<Playlist>(`/playlists/${page.params.id}`));
  let editId = $state("");
  let removing = $state(false);
  let error = $state("");
  let tracks = $derived(playlist.data?.tracks ?? []);

  async function removeTrack(id: string) {
    if (removing || !playlist.data) return;
    removing = true;
    error = "";
    try {
      await api.delete(`/playlists/${playlist.data.id}/tracks/${id}`);
      invalidatePlaylists();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : t("common.error");
    } finally {
      removing = false;
    }
  }

  async function remove(id: string) {
    if (removing || !confirm(t("playlists.deleteConfirm"))) return;
    removing = true;
    error = "";
    try {
      await api.delete(`/playlists/${id}`);
      invalidatePlaylists();
      await goto("/playlists");
    } catch (cause) {
      error = cause instanceof Error ? cause.message : t("common.error");
    } finally {
      removing = false;
    }
  }
</script>

<svelte:head><title>{playlist.data?.name ?? t("playlists.title")} — SoundTime</title></svelte:head>

{#if playlist.data}
  {@const data = playlist.data}
  {@const mine = !!auth.user && (data.user_id ?? data.owner_id) === auth.user.id}
  <div class="space-y-7">
    <ReleaseHeader kind="playlist" title={data.name} image={data.cover_url} label={t(data.is_public ? "playlists.publicPlaylist" : "playlists.private")}>
      {#snippet info()}
        {#if data.description}<p class="mb-1">{data.description}</p>{/if}
        {tracks.length} {t("playlists.tracks")}
      {/snippet}
      {#snippet actions()}
        <Button disabled={!tracks.some(canPlay)} onclick={() => queue.playQueue(tracks, 0, "playlist")}><Play size={15} fill="currentColor" />{t("album.playAll")}</Button>
        {#if mine}
          <Button variant="outline" onclick={() => editId = editId === data.id ? "" : data.id}><Pencil size={14} />{t("playlists.edit")}</Button>
          <Button variant="ghost" class="text-destructive" disabled={removing} onclick={() => remove(data.id)}><Trash2 size={14} />{t("common.delete")}</Button>
        {/if}
      {/snippet}
    </ReleaseHeader>
    {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
    {#if mine && editId === data.id}
      <div class="max-w-lg rounded-xl border border-border p-5">
        <PlaylistForm playlist={data} oncancel={() => editId = ""} onsave={() => editId = ""} />
      </div>
    {/if}
    {#if tracks.length}<TrackList source="playlist" {tracks} onremove={mine ? removeTrack : undefined} />{:else}<CatalogState message={t("playlists.empty")} />{/if}
  </div>
{:else}
  <CatalogState loading={playlist.loading} failed={playlist.failed} status={playlist.status} retry={playlist.retry} />
{/if}
