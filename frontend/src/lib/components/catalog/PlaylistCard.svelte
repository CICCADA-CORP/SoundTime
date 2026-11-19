<script lang="ts">
  import { LockKeyhole } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Playlist } from "$lib/types";
  import CatalogCard from "./CatalogCard.svelte";
  let { playlist }: { playlist: Playlist } = $props();
</script>

<CatalogCard kind="playlist" href="/playlists/{playlist.id}" title={playlist.name} image={playlist.cover_url}
  description={playlist.description} detail={`${playlist.track_count ?? 0} ${t("playlists.tracks")}`}>
  {#snippet actions()}
    {#if !playlist.is_public}
      <span class="pointer-events-none absolute right-3 top-3 rounded-md bg-background/95 p-1.5" role="img" aria-label={t("playlists.private")}><LockKeyhole size={13} /></span>
    {/if}
  {/snippet}
</CatalogCard>
