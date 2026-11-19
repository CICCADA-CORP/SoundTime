<script lang="ts">
  import { ChevronRight, X } from "lucide-svelte";
  import { tick } from "svelte";
  import type { EditorialPlaylist } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogArtwork from "./CatalogArtwork.svelte";

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

  function trackCount(count: number) {
    return count === 1 ? t("workspaceHome.oneTrack") : t("playlists.trackCount", { count });
  }
</script>

<section class="space-y-4" aria-label={t("explore.editorialPlaylists")}>
  {#if selected}
    <div class="flex items-start gap-4">
      <div class="size-20 shrink-0" aria-hidden="true">
        <CatalogArtwork source={selected.cover_url} title="" kind="playlist" />
      </div>
      <div class="min-w-0 flex-1">
        <h2 id={`${id}-heading`} tabindex="-1" class="break-words text-xl font-semibold tracking-tight">{selected.name}</h2>
        <p class="mt-1 text-sm text-muted-foreground">{trackCount(selected.track_count)}</p>
        {#if selected.description}<p class="mt-2 break-words text-sm text-muted-foreground">{selected.description}</p>{/if}
      </div>
      <Button variant="ghost" size="icon" onclick={close} aria-label={t("common.close")}><X size={17} /></Button>
    </div>
    {#if selected.tracks.length}
      <TrackList source="playlist" tracks={selected.tracks} />
    {:else}
      <p class="text-sm text-muted-foreground">{t("explore.noTracks")}</p>
    {/if}
  {:else}
    <h2 class="text-xl font-semibold tracking-tight">{t("explore.editorialPlaylists")}</h2>
    <ul class="grid gap-2 md:grid-cols-2">
      {#each playlists as item (item.id)}
        <li class="min-w-0">
          <button id={`${id}-${item.id}`} type="button" onclick={() => open(item)}
            class="flex w-full items-center gap-3 rounded-lg border border-border p-2.5 text-left hover:bg-secondary/60 focus-visible:outline-offset-2">
            <span aria-hidden="true" class="block size-14 shrink-0">
              <CatalogArtwork source={item.cover_url} title="" kind="playlist" />
            </span>
            <span class="min-w-0 flex-1">
              <span class="block truncate font-semibold">{item.name}</span>
              <span class="text-sm text-muted-foreground">{trackCount(item.track_count)}</span>
            </span>
            <ChevronRight size={16} aria-hidden="true" class="shrink-0 text-muted-foreground" />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
