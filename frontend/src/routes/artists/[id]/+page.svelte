<script lang="ts">
  import { page } from "$app/state";
  import { Radio } from "lucide-svelte";
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import type { Artist } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import RecordShelf from "$lib/components/catalog/RecordShelf.svelte";
  import ReleaseHeader from "$lib/components/catalog/ReleaseHeader.svelte";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";

  const artist = catalogQuery(() => api.get<Artist>(`/artists/${page.params.id}`));
  const radio = getRadioStore();
</script>

<svelte:head><title>{artist.data?.name ?? t("track.artist")} — SoundTime</title></svelte:head>

{#if artist.data}
  {@const data = artist.data}
  <div class="space-y-8">
    <ReleaseHeader kind="artist" title={data.name} image={data.image_url} label={t("track.artist")}>
      {#snippet info()}{data.bio ?? ""}{/snippet}
      {#snippet actions()}
        <Button variant="secondary" disabled={radio.loading} onclick={() => radio.startRadio("artist", { seedId: data.id, label: data.name })}>
          <Radio size={16} />{t("radio.startFromArtist")}
        </Button>
      {/snippet}
    </ReleaseHeader>
    {#if radio.error}<p class="text-sm text-destructive" role="alert">{t("radio.error")}</p>{/if}
    {#if data.albums?.length}
      <section class="space-y-4">
        <h2 class="text-xl font-semibold">{t("artist.albums")}</h2>
        <RecordShelf>{#each data.albums as album (album.id)}<AlbumCard {album} />{/each}</RecordShelf>
      </section>
    {/if}
    {#if data.tracks?.length}
      <section class="space-y-4"><h2 class="text-xl font-semibold">{t("artist.tracks")}</h2><TrackList tracks={data.tracks} showArtist={false} /></section>
    {/if}
  </div>
{:else}
  <CatalogState loading={artist.loading} failed={artist.failed} status={artist.status} retry={artist.retry} />
{/if}
