<script lang="ts">
  import { page } from "$app/state";
  import { Play } from "lucide-svelte";
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Album } from "$lib/types";
  import { formatDuration } from "$lib/utils";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { Button } from "$lib/components/ui/button";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import ReleaseHeader from "$lib/components/catalog/ReleaseHeader.svelte";
  import TrackList from "$lib/components/TrackList.svelte";

  const queue = getQueueStore();
  const album = catalogQuery(() => api.get<Album>(`/albums/${page.params.id}`));
  let tracks = $derived(album.data?.tracks ?? []);
</script>

<svelte:head><title>{album.data?.title ?? t("track.album")} — SoundTime</title></svelte:head>

{#if album.data}
  {@const data = album.data}
  <div class="space-y-7">
    <ReleaseHeader kind="album" title={data.title} image={data.cover_url} label={t("track.album")}>
      {#snippet info()}
        <a class="hover:underline" href="/artists/{data.artist_id}">{data.artist_name ?? t("track.unknownArtist")}</a>
        {#if data.year} · {data.year}{/if} · {tracks.length} {t("album.tracks")} · {formatDuration(tracks.reduce((sum, track) => sum + track.duration_secs, 0))}
      {/snippet}
      {#snippet actions()}
        <Button disabled={!tracks.length} onclick={() => queue.playQueue(tracks, 0, "album")}><Play size={15} fill="currentColor" />{t("album.playAll")}</Button>
      {/snippet}
    </ReleaseHeader>
    {#if tracks.length}<TrackList source="album" {tracks} showAlbum={false} />{:else}<CatalogState message={t("explore.noTracks")} />{/if}
  </div>
{:else}
  <CatalogState loading={album.loading} failed={album.failed} status={album.status} retry={album.retry} />
{/if}
