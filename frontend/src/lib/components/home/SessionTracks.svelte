<script lang="ts">
  import { AudioLines, Pause, Play } from "lucide-svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import type { PlaybackSource, Track } from "$lib/types";
  import { formatDuration } from "$lib/utils";
  import { homeCopy } from "./copy";

  let { tracks, source = "collection" }: { tracks: Track[]; source?: PlaybackSource } = $props();
  const player = getPlayerStore();
  const queue = getQueueStore();
</script>

<ol class="divide-y divide-border overflow-hidden rounded-xl border border-border bg-card">
  {#each tracks as track, index (track.id)}
    {@const current = player.currentTrack?.id === track.id}
    {@const playing = current && player.isPlaying}
    <li>
      <button
        type="button"
        class="flex min-h-14 w-full items-center gap-3 px-3 text-left hover:bg-muted/60 focus-visible:outline-offset-[-3px] sm:px-4"
        aria-label={`${playing ? homeCopy().pause : homeCopy().play} · ${track.title}`}
        onclick={() => current ? player.togglePlay() : queue.playQueue(tracks, index, source)}
      >
        <span class="w-4 text-center text-xs tabular-nums text-muted-foreground">{index + 1}</span>
        <span class="grid size-9 shrink-0 place-items-center overflow-hidden rounded-md bg-muted text-muted-foreground">
          {#if track.cover_url}<img src={track.cover_url} alt="" loading="lazy" class="size-full object-cover" />
          {:else}<AudioLines size={17} />{/if}
        </span>
        <span class="min-w-0 flex-1">
          <span class="block truncate text-sm font-semibold" class:text-primary={current}>{track.title}</span>
          {#if track.artist_name}<span class="block truncate text-xs text-muted-foreground">{track.artist_name}</span>{/if}
        </span>
        <span class="mr-2 text-xs tabular-nums text-muted-foreground">{formatDuration(track.duration_secs)}</span>
        {#if playing}<Pause size={14} class="text-primary" fill="currentColor" />
        {:else}<Play size={14} class="text-muted-foreground" />{/if}
      </button>
    </li>
  {/each}
</ol>
