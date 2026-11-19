<script lang="ts">
  import { ListMusic, X } from "lucide-svelte";
  import type { EditorialPlaylist } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import TrackList from "$lib/components/TrackList.svelte";
  import RecordShelf from "./RecordShelf.svelte";

  let { playlists }: { playlists: EditorialPlaylist[] } = $props();
  let selected = $state<EditorialPlaylist | null>(null);
</script>

<section class="space-y-4">
  <h2 class="text-xl font-semibold tracking-tight">{t("explore.editorialPlaylists")}</h2>
  <RecordShelf>
    {#each playlists as item (item.id)}
      <button class="group min-w-0 text-left" aria-expanded={selected?.id === item.id}
        onclick={() => selected = selected?.id === item.id ? null : item}>
        <span class="mb-2 grid aspect-square place-items-center overflow-hidden rounded-lg bg-secondary text-primary">
          {#if item.cover_url}<img src={item.cover_url} alt="" loading="lazy" class="size-full object-cover" />
          {:else}<ListMusic size={40} strokeWidth={1.3} />{/if}
        </span>
        <span class="block truncate font-semibold group-hover:text-primary">{item.name}</span>
        <span class="text-sm text-muted-foreground">{item.track_count} {t("album.tracks")}</span>
      </button>
    {/each}
  </RecordShelf>
  {#if selected}
    <div class="space-y-3 rounded-xl border border-border p-4">
      <div class="flex items-start justify-between gap-4">
        <div><h3 class="font-semibold">{selected.name}</h3>
          {#if selected.description}<p class="mt-1 text-sm text-muted-foreground">{selected.description}</p>{/if}
        </div>
        <Button variant="ghost" onclick={() => selected = null} aria-label={t("common.close")}><X size={16} /></Button>
      </div>
      <TrackList source="playlist" tracks={selected.tracks} />
    </div>
  {/if}
</section>
