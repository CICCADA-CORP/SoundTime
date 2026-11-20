<script lang="ts">
  import { X } from "lucide-svelte";
  import { tick } from "svelte";
  import type { EditorialPlaylist } from "$lib/types";
  import { formatTrackCount, t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogArtwork from "./CatalogArtwork.svelte";
  import ArtworkRail from "./ArtworkRail.svelte";
  import SectionHeading from "./SectionHeading.svelte";

  let { playlists }: { playlists: EditorialPlaylist[] } = $props();
  const id = $props.id();
  let selectedId = $state<string | null>(null);
  let selected = $derived(playlists.find(item => item.id === selectedId));

  async function open(item: EditorialPlaylist) {
    selectedId = item.id;
    await tick();
    document.getElementById(`${id}-heading`)?.focus();
  }

  async function close() {
    const previous = selectedId;
    selectedId = null;
    await tick();
    document.getElementById(`${id}-${previous}`)?.focus();
  }
</script>

<section class="space-y-4" aria-label={t("explore.editorialPlaylists")}>
  {#if selected}
    <div class="flex items-start gap-4">
      <div class="size-20 shrink-0" aria-hidden="true">
        <CatalogArtwork source={selected.cover_url} artworkId={selected.id} title={selected.name} kind="playlist" />
      </div>
      <div class="min-w-0 flex-1">
        <h2 id={`${id}-heading`} tabindex="-1" class="selection-title">{selected.name}</h2>
        <p class="mt-1 text-sm text-muted-foreground">{formatTrackCount(selected.track_count)}</p>
        {#if selected.description}<p class="mt-2 break-words text-sm text-muted-foreground">{selected.description}</p>{/if}
      </div>
      <Button variant="ghost" size="icon" onclick={close} aria-label={t("common.close")}><X size={17} /></Button>
    </div>
    {#if selected.tracks.length}
      <TrackList source="playlist" compact tracks={selected.tracks} />
    {:else}
      <p class="text-sm text-muted-foreground">{t("explore.noTracks")}</p>
    {/if}
  {:else}
    <SectionHeading title={t("explore.editorialPlaylists")} />
    <ArtworkRail label={t("explore.editorialPlaylists")}>
      {#each playlists as item (item.id)}
          <button id={`${id}-${item.id}`} type="button" onclick={() => open(item)} class="mix">
            <span aria-hidden="true" class="mix-art">
              <CatalogArtwork source={item.cover_url} artworkId={item.id} title={item.name} kind="playlist" />
            </span>
            <strong>{item.name}</strong>
            <small>{formatTrackCount(item.track_count)}</small>
          </button>
      {/each}
    </ArtworkRail>
  {/if}
</section>

<style>
  .mix { display: grid; min-width: 0; align-content: start; justify-items: stretch; gap: .15rem; border-radius: .7rem; text-align: left; }
  .mix-art { display: block; width: 100%; aspect-ratio: 1; margin-bottom: .45rem; overflow: hidden; border-radius: .7rem; }
  .selection-title { overflow-wrap: anywhere; font-size: 1.3rem; font-weight: 630; line-height: 1.2; letter-spacing: -.035em; }
  strong, small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  strong { font-size: .88rem; font-weight: 560; }
  small { color: hsl(var(--muted-foreground)); font-size: .78rem; }
  .mix:hover strong { text-decoration: underline; }
</style>
