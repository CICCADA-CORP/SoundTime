<script lang="ts">
  import { DropdownMenu } from "bits-ui";
  import { ArrowDown, ArrowUp, Ellipsis, Play, X } from "lucide-svelte";
  import { canPlay } from "$lib/catalog/availability";
  import CatalogArtwork from "../catalog/CatalogArtwork.svelte";
  import { playerArtwork } from "$lib/components/player/artwork";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Track } from "$lib/types";
  import { formatDuration } from "$lib/utils";

  let { tracks, compact = false, onselect }: { tracks: Track[]; compact?: boolean; onselect?: () => void } = $props();
  const queue = getQueueStore();
  const entries = $derived.by(() => {
    // A recording may occupy several positions in a playlist.
    const occurrences = new Map<string, number>();
    return tracks.map(track => {
      const occurrence = occurrences.get(track.id) ?? 0;
      occurrences.set(track.id, occurrence + 1);
      return { key: track.id + ":" + occurrence, track, cover: playerArtwork(track) };
    });
  });
  const menuItem = "flex min-h-9 cursor-default select-none items-center gap-2.5 rounded-md px-2.5 text-sm outline-none data-[highlighted]:bg-muted data-[disabled]:pointer-events-none data-[disabled]:opacity-40";
</script>

<ol class:compact>
  {#each entries as { key, track, cover }, index (key)}
    {@const position = queue.currentIndex + index + 1}
    <li>
      <button class="record" type="button" disabled={!canPlay(track)}
        onclick={() => { queue.playQueue(queue.queue, position, queue.sourceContext ?? "queue"); onselect?.(); }}>
        <span class="art" aria-hidden="true">
          <CatalogArtwork source={cover} title={track.album_title || track.title} kind="album" artworkId={track.album_id ?? track.id} />
          <span class="play"><Play size={15} fill="currentColor" /></span>
        </span>
        <span class="label">
          <span class="title">{track.title}</span>
          {#if track.artist_name}<span class="artist">{track.artist_name}</span>{/if}
        </span>
        <span class="duration">{formatDuration(track.duration_secs)}</span>
      </button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger aria-label={t("track.options") + " · " + track.title} class="queue-more">
          <Ellipsis size={16} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content align="end" sideOffset={4} loop class="z-[160] min-w-36 rounded-lg border border-border bg-popover p-1 text-popover-foreground shadow-lg">
            <DropdownMenu.Item class={menuItem} disabled={index === 0} onSelect={() => queue.moveInQueue(position, position - 1)}>
              <ArrowUp size={15} />{t("shell.moveUp")}
            </DropdownMenu.Item>
            <DropdownMenu.Item class={menuItem} disabled={index === entries.length - 1} onSelect={() => queue.moveInQueue(position, position + 1)}>
              <ArrowDown size={15} />{t("shell.moveDown")}
            </DropdownMenu.Item>
            <DropdownMenu.Separator class="my-1 h-px bg-border" />
            <DropdownMenu.Item class={menuItem} onSelect={() => queue.removeFromQueue(position)}>
              <X size={15} />{t("upload.remove")}
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </li>
  {/each}
</ol>

<style>
  li { position: relative; display: flex; min-width: 0; align-items: center; gap: 4px; padding-block: 4px; }
  li + li { border-top: 1px solid hsl(var(--border) / .6); }
  .record { display: flex; align-items: center; gap: 10px; flex: 1; min-width: 0; text-align: start; border-radius: 6px; }
  .art { display: grid; place-items: center; width: 46px; height: 46px; flex: none; overflow: hidden; position: relative; border-radius: 6px; background: hsl(var(--muted)); color: hsl(var(--muted-foreground)); }
  .play { position: absolute; display: grid; place-items: center; inset: 0; background: #0006; color: white; opacity: 0; }
  .record:hover .play, .record:focus-visible .play { opacity: 1; }
  .record:disabled { opacity: .5; }
  .label { flex: 1; min-width: 0; }
  .title, .artist { display: block; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .title { font-size: .82rem; line-height: 1.5; font-weight: 530; }
  .artist, .duration { color: hsl(var(--muted-foreground)); font-size: .74rem; line-height: 1.4; }
  .duration { font-variant-numeric: tabular-nums; }
  :global(.queue-more) { display: grid; place-items: center; width: 32px; height: 32px; flex: none; border-radius: 5px; color: hsl(var(--muted-foreground)); }
  :global(.queue-more:hover), :global(.queue-more[data-state=open]) { background: hsl(var(--foreground) / .08); }
  .compact li { padding: 0; border: 0; }
  .compact .art { width: 32px; height: 32px; }
  .compact .record { min-height: 47px; padding: 4px 3px; gap: 8px; }
  .compact .title { font-size: .78rem; font-weight: 550; }
  .compact .artist, .compact .duration { font-size: .7rem; }
  .compact :global(.queue-more) { width: 36px; height: 40px; }
  @media (hover: hover) and (pointer: fine) {
    .compact :global(.queue-more) { opacity: 0; position: absolute; inset-inline-end: 0; }
    .compact li:hover :global(.queue-more), .compact li:focus-within :global(.queue-more), .compact :global(.queue-more[data-state=open]) { opacity: 1; }
    .compact li:hover .duration, .compact li:focus-within .duration { visibility: hidden; }
  }
</style>
