<script lang="ts">
  import { Check, Pause, Play, Plus } from "lucide-svelte";
  import { onDestroy } from "svelte";
  import type { Track } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import { canPlay } from "$lib/catalog/availability";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import CatalogArtwork from "$lib/components/catalog/CatalogArtwork.svelte";
  import SectionHeading from "$lib/components/catalog/SectionHeading.svelte";
  import { homeCopy } from "./copy";

  let { tracks }: { tracks: Track[] } = $props();
  const copy = $derived(homeCopy());
  const player = getPlayerStore();
  const queue = getQueueStore();
  const chart = $derived(tracks.slice(0, 6));
  let added = $state("");
  let feedback: ReturnType<typeof setTimeout>;
  onDestroy(() => clearTimeout(feedback));

  function add(track: Track) {
    queue.addToQueue(track);
    added = track.id;
    clearTimeout(feedback);
    feedback = setTimeout(() => added = "", 1800);
  }
</script>

<section class="chart">
  <SectionHeading title={copy.topTracks} href="/explore?tab=trending&period=all" />
  <ol style:--rows={Math.ceil(chart.length / 2)}>
    {#each chart as track, index (track.id)}
      {@const playing = track.id === player.currentTrack?.id && player.isPlaying}
      <li>
        <span class="rank">{index + 1}</span>
        <button class="cover" type="button" disabled={!canPlay(track)}
          aria-label={`${playing ? copy.pause : copy.play} · ${track.title}`}
          onclick={() => track.id === player.currentTrack?.id ? player.togglePlay() : queue.playQueue(chart, index, "explore")}>
          <CatalogArtwork source={track.cover_url} artworkId={track.album_id ?? track.id} title={track.title} kind="album" />
          <span class:playing>{#if playing}<Pause size={13} fill="currentColor" />{:else}<Play size={13} fill="currentColor" />{/if}</span>
        </button>
        <a class="title" href={"/tracks/" + track.id}>{track.title}</a>
        {#if track.artist_name}<a class="artist" href={"/artists/" + track.artist_id}>{track.artist_name}</a>{/if}
        <button class="add" type="button" disabled={!canPlay(track)} onclick={() => add(track)}
          aria-label={`${t("track.addToQueue")} · ${track.title}`}>
          {#if added === track.id}<Check size={12} />{:else}<Plus size={12} />{/if}

        </button>
      </li>
    {/each}
  </ol>
  <span class="sr-only" role="status">{added ? copy.added : ""}</span>
</section>

<style>
  .chart { container-type: inline-size; }
  ol { display: grid; }
  li { display: grid; grid-template-columns: 1.3rem 2.25rem minmax(0, 1fr) auto; align-items: center; column-gap: .65rem; min-height: 3.75rem; border-top: 1px solid hsl(var(--border)); }
  .rank { color: hsl(var(--muted-foreground)); font-size: .72rem; font-variant-numeric: tabular-nums; text-align: center; }
  .cover { position: relative; width: 2.25rem; height: 2.25rem; overflow: hidden; border-radius: .4rem; }
  .cover > span { position: absolute; inset: 0; display: grid; place-items: center; color: white; background: #0009; opacity: 0; }
  .cover:hover > span, .cover:focus-visible > span, .cover > .playing { opacity: 1; }
  .title, .artist { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .title { font-size: .84rem; font-weight: 565; }
  .artist { display: none; font-size: .74rem; color: hsl(var(--muted-foreground)); }
  a:hover { text-decoration: underline; }
  .add { grid-column: -2; display: grid; place-items: center; width: 2rem; height: 2rem; color: hsl(var(--muted-foreground)); }
  .add:hover { background: hsl(var(--muted)); color: hsl(var(--link)); }
  .add:disabled, .cover:disabled { opacity: .45; }
  @container (min-width: 610px) {
    ol { grid-template: repeat(var(--rows), auto) / repeat(2, minmax(0, 1fr)); grid-auto-flow: column; column-gap: 1.5rem; }
  }
  @container (min-width: 780px) {
    li { grid-template-columns: 1rem 2.25rem minmax(0, 1.15fr) minmax(0, .85fr) auto; }
    .artist { display: block; }
  }
  @container (max-width: 779px) {
    li:has(.artist) { grid-template: auto auto / 1.3rem 2.25rem minmax(0, 1fr) auto; row-gap: 1px; }
    li:has(.artist) .rank, li:has(.artist) .cover, li:has(.artist) .add { grid-row: 1 / 3; }
    li:has(.artist) .title { align-self: end; }
    .artist { display: block; grid-column: 3; grid-row: 2; align-self: start; }
  }
</style>
