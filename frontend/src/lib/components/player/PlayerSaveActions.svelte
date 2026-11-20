<script lang="ts">
  import { ListPlus, RefreshCw } from "lucide-svelte";
  import { api } from "$lib/api";
  import { favoriteQuery } from "$lib/catalog/favorites.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Track } from "$lib/types";
  import { Button } from "$lib/components/ui/button";
  import FavoriteButton from "../FavoriteButton.svelte";
  import TrackActions from "../catalog/TrackActions.svelte";

  let { track }: { track: Track } = $props();
  let choosingPlaylist = $state(false);
  const favorites = favoriteQuery(() => api.get<string[]>("/favorites/check?track_ids=" + track.id));
</script>

<div class="save-controls">
  <Button variant="ghost" size="icon" aria-label={t("track.addToPlaylist")} onclick={() => choosingPlaylist = true}>
    <ListPlus size={19} />
  </Button>
  {#if favorites.failed}
    <Button variant="ghost" size="icon" aria-label={t("radio.tryAgain")} onclick={favorites.retry}><RefreshCw size={16} /></Button>
  {:else if !favorites.loading}
    <FavoriteButton trackId={track.id} liked={favorites.data?.includes(track.id) ?? false} size={19} />
  {/if}
</div>

{#if choosingPlaylist}
  <TrackActions action="playlist" {track} close={() => choosingPlaylist = false} />
{/if}

<style>
  .save-controls { display: flex; align-items: center; }
  .save-controls :global(button) { display: grid; place-items: center; width: 40px; height: 40px; border-radius: 50%; }
</style>
