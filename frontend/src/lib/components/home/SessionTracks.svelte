<script lang="ts">
  import { Pause, Play } from "lucide-svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import type { PlaybackSource, Track } from "$lib/types";
  import { canPlay } from "$lib/catalog/availability";
  import { formatDuration } from "$lib/utils";
  import CatalogArtwork from "$lib/components/catalog/CatalogArtwork.svelte";
  import { homeCopy } from "./copy";

  let { tracks, source = "collection" }: {
    tracks: Track[]; source?: PlaybackSource;
  } = $props();
  const player = getPlayerStore();
  const queue = getQueueStore();
</script>

<ol class="session">
  {#each tracks as track, index (track.id)}
    {@const current = player.currentTrack?.id === track.id}
    {@const playing = current && player.isPlaying}
    <li>
      <button type="button" class:current disabled={!canPlay(track)}
        aria-label={(playing ? homeCopy().pause : homeCopy().play) + " · " + track.title}
        onclick={() => current ? player.togglePlay() : queue.playQueue(tracks, index, source)}>
        <span class="thumb" aria-hidden="true"><CatalogArtwork source={track.cover_url}
          title={track.title} kind="album" artworkId={track.album_id ?? track.id} /></span>
        <span class="credit"><strong>{track.title}</strong>{#if track.artist_name}<small>{track.artist_name}</small>{/if}</span>
        <span class="time">{formatDuration(track.duration_secs)}</span>
        <span class="transport" class:playing aria-hidden="true">
          {#if playing}<Pause size={11} fill="currentColor" />{:else}<Play size={11} fill="currentColor" />{/if}
        </span>
      </button>
    </li>
  {/each}
</ol>

<style>
  .session { min-width: 0; }
  li + li { border-top: 1px solid hsl(var(--foreground) / .08); }
  button { width: 100%; display: flex; align-items: center; gap: .8rem; min-height: 3.15rem; padding: .4rem .2rem;
    border-radius: .35rem; text-align: start; }
  button:hover, button:focus-visible { background: hsl(var(--card) / .48); }
  button:disabled { opacity: .55; }
  .thumb { flex: none; width: 2.25rem; height: 2.25rem; overflow: hidden; border-radius: .32rem; }
  .credit { display: grid; flex: 1; min-width: 0; gap: 1px; }
  .credit strong, .credit small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .credit strong { font-size: .85rem; font-weight: 570; }
  .credit small, .time { font-size: .73rem; color: hsl(var(--muted-foreground)); }
  .current .credit strong { color: hsl(var(--link)); }
  .time { font-variant-numeric: tabular-nums; margin-inline: .3rem; }
  .transport { display: grid; place-items: center; width: 1.55rem; height: 1.55rem; flex: none; border-radius: 50%; background: hsl(var(--card) / .75); color: hsl(var(--foreground)); }
  button:hover .transport, .transport.playing { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); }
</style>
