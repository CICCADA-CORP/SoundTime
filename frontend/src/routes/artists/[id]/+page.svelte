<script lang="ts">
  import { page } from "$app/state";
  import { Radio } from "lucide-svelte";
  import { apiFetch } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import type { Artist } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import SectionHeading from "$lib/components/catalog/SectionHeading.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import RecordShelf from "$lib/components/catalog/RecordShelf.svelte";
  import ReleaseHeader from "$lib/components/catalog/ReleaseHeader.svelte";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";

  const artist = catalogQuery(signal => apiFetch<Artist>(`/artists/${page.params.id}`, { signal }));
  const radio = getRadioStore();
  const artistRadio = $derived(radio.seedType === "artist" && radio.seedId === page.params.id);
</script>

<svelte:head><title>{artist.data?.name ?? t("track.artist")} — SoundTime</title></svelte:head>

{#if artist.data}
  {@const data = artist.data}
  <div class="space-y-8">
    <ReleaseHeader artworkId={data.id} kind="artist" title={data.name} image={data.image_url} label={t("track.artist")}>
      {#snippet info()}{data.bio ?? ""}{/snippet}
      {#snippet actions()}
        <Button variant="secondary" disabled={artistRadio && radio.loading} onclick={() => radio.startRadio("artist", { seedId: data.id, label: data.name })}>
          <Radio size={16} />{t("radio.startFromArtist")}
        </Button>
      {/snippet}
    </ReleaseHeader>
    {#if artistRadio && radio.error}<p class="text-sm text-destructive" role="alert">{t("radio.error")}</p>{/if}
    {#if data.albums?.length}
      <section class="space-y-4">
        <SectionHeading title={t("artist.albums")} />
        <RecordShelf>{#each data.albums as album (album.id)}<AlbumCard {album} />{/each}</RecordShelf>
      </section>
    {/if}
    {#if data.tracks?.length}
      <section class="space-y-4"><SectionHeading title={t("artist.tracks")} /><TrackList tracks={data.tracks} showArtist={false} /></section>
    {/if}
  </div>
{:else}
  <CatalogState layout="detail" loading={artist.loading} failed={artist.failed} status={artist.status} retry={artist.retry} />
{/if}
