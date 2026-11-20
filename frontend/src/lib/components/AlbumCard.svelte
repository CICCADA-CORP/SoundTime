<script lang="ts">
  import { Loader2, Play } from "lucide-svelte";
  import { api } from "$lib/api";
  import type { Album } from "$lib/types";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import CatalogCard from "./catalog/CatalogCard.svelte";

  let { album }: { album: Album } = $props();
  const queue = getQueueStore();
  let loading = $state(false);
  let failed = $state(false);

  async function play() {
    if (loading) return;
    loading = true;
    failed = false;
    try {
      const tracks = album.tracks ?? (await api.get<Album>(`/albums/${album.id}`)).tracks;
      if (tracks?.length) queue.playQueue(tracks, 0, "album");
    } catch (error) {
      failed = true;
      console.error("Failed to fetch album tracks for playback:", error);
    } finally {
      loading = false;
    }
  }
</script>

<CatalogCard artworkId={album.id} kind="album" href="/albums/{album.id}" title={album.title} image={album.cover_url}
  description={[album.year, album.artist_name].filter(Boolean).join(" · ")}>
  {#snippet actions()}
    <button type="button" aria-label={t("a11y.playAlbum")} disabled={loading} onclick={play}
      class="absolute right-2 top-2 grid size-8 place-items-center rounded-full bg-background text-foreground shadow-sm transition-opacity disabled:opacity-50 sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100">
      {#if loading}<Loader2 size={17} class="motion-safe:animate-spin" />{:else}<Play size={15} fill="currentColor" />{/if}
    </button>
    {#if failed}<p class="px-1 text-xs text-destructive" role="alert">{t("catalog.unavailable")}</p>{/if}
  {/snippet}
</CatalogCard>
