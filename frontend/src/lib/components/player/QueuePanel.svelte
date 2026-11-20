<script lang="ts">
  import { tick } from "svelte";
  import { AudioLines, X, ListMusic, Play, Pause, RotateCw } from "lucide-svelte";
  import { canPlay } from "$lib/catalog/availability";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { formatDuration } from "$lib/utils";
  import type { Track } from "$lib/types";
  import CatalogArtwork from "../catalog/CatalogArtwork.svelte";
  import UpcomingTracks from "./UpcomingTracks.svelte";
  import { playerArtwork } from "./artwork";

  let { close, recent, title, loading }: { close: () => void; recent: Track[]; title: string; loading: boolean } = $props();
  const player = getPlayerStore();
  const queue = getQueueStore();
  const current = $derived(player.currentTrack);
  const upcoming = $derived(queue.queue.slice(queue.currentIndex + 1));
  const saved = $derived(upcoming.find(canPlay));
  const featured = $derived(current || saved || recent[0]);
  const suggestions = $derived(!current && !upcoming.length ? recent.slice(1) : []);
  let currentButton = $state<HTMLButtonElement>();

  async function focusCurrent() {
    await tick();
    currentButton?.focus();
  }

  function playFeatured() {
    if (current) player.togglePlay();
    else if (saved) queue.playQueue(queue.queue, queue.currentIndex + 1, queue.sourceContext ?? "queue");
    else if (recent.length) queue.playQueue(recent, 0, "collection");
  }
</script>

<div class="queue-panel">
  <div class="heading">
    <h2>{#if current}<AudioLines size={17} />{:else}<ListMusic size={17} />{/if}{title}</h2>
    <button type="button" data-close-queue aria-label={t("a11y.close")} title={t("a11y.close")} onclick={close}><X size={15} /></button>
  </div>
  <div class="scroll" aria-busy={loading && !featured}>
    {#if loading && !featured && !upcoming.length}
      <div class="skeleton" aria-hidden="true">
        <div class="feature-art"></div>
        <div class="skeleton-meta"><i></i><i></i></div>
        {#each Array(4) as _, index (index)}<div class="skeleton-row"></div>{/each}
      </div>
    {:else}
      {#if featured}
        <div class="feature">
          <div class="feature-art" aria-hidden="true">
            <CatalogArtwork source={playerArtwork(featured)} title={featured.album_title || featured.title} kind="album" artworkId={featured.album_id ?? featured.id} loading="eager" />
          </div>
          <div class="feature-meta">
            <div class="label">
              {#if !current}<span class="context">{t(saved ? "player.upNext" : "shell.listenNow")}</span>{/if}
              <p class="title">{featured.title}</p>
              {#if featured.artist_name}<p class="artist">{featured.artist_name}</p>{/if}
            </div>
            <button type="button" class="current" bind:this={currentButton} onclick={playFeatured}
              aria-label={t(current && player.failed ? "radio.tryAgain" : current && player.isPlaying ? "shell.pause" : "shell.play") + " · " + featured.title}>
              {#if current && player.failed}<RotateCw size={15} />{:else if current && player.isPlaying}<Pause size={14} fill="currentColor" />{:else}<Play size={14} fill="currentColor" />{/if}
            </button>
          </div>
        </div>
      {/if}
      {#if upcoming.length}
        <h3>{t("player.upNext")}</h3>
        <UpcomingTracks tracks={upcoming} compact onselect={focusCurrent} />
      {:else if suggestions.length}
        <h3>{t("explore.recentTracks")}</h3>
        <ol class="recent">
          {#each suggestions as track, index (track.id)}
            <li>
              <button type="button" onclick={() => { queue.playQueue(recent, index + 1, "collection"); void focusCurrent(); }} aria-label={t("shell.play") + " · " + track.title}>
                <span class="thumb" aria-hidden="true"><CatalogArtwork source={playerArtwork(track)} title={track.album_title || track.title} kind="album" artworkId={track.album_id ?? track.id} /></span>
                <span class="label"><span class="title">{track.title}</span>{#if track.artist_name}<span class="artist">{track.artist_name}</span>{/if}</span>
                <span class="duration">{formatDuration(track.duration_secs)}</span>
              </button>
            </li>
          {/each}
        </ol>
      {:else if !featured || current}
        <div class="empty"><ListMusic size={21} strokeWidth={1.5} /><p>{t("player.queueEmpty")}</p></div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .queue-panel { display: flex; flex-direction: column; height: 100%; min-height: 0; overflow: hidden; border-radius: 17px; background: var(--glass-panel); padding: 0 14px; }
  .heading { flex: none; display: flex; align-items: center; justify-content: space-between; min-height: 47px; gap: 8px; }
  h2 { display: flex; align-items: center; gap: 8px; font-size: .93rem; font-weight: 640; letter-spacing: -.025em; }
  h2 :global(svg) { color: hsl(var(--primary)); }
  .heading button, .current { display: grid; place-items: center; flex: none; width: 30px; height: 30px; border-radius: 50%; }
  .heading button { color: hsl(var(--muted-foreground)); }
  .heading button:hover, .recent button:hover { background: hsl(var(--card) / .65); }
  .scroll { min-height: 0; flex: 1; overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; padding: 0 2px 18px; }
  .feature-art { width: 100%; aspect-ratio: 1; overflow: hidden; border-radius: 12px; background: hsl(var(--muted)); box-shadow: 0 5px 16px -7px rgb(30 25 60 / .22); }
  .skeleton .feature-art { box-shadow: none; }
  .skeleton-meta { display: grid; align-content: center; gap: 6px; height: 76px; }
  .skeleton-meta i, .skeleton-row { display: block; width: 62%; height: 9px; border-radius: 4px; background: hsl(var(--muted)); }
  .skeleton-meta i + i { width: 40%; }
  .skeleton-row { width: 86%; height: 32px; margin-block: 11px; }
  .feature-meta { display: flex; align-items: center; justify-content: space-between; gap: 7px; padding: 13px 1px 15px; border-bottom: 1px solid hsl(var(--foreground) / .09); }
  .current { width: 32px; height: 32px; background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); }
  .label { min-width: 0; flex: 1; }
  .title, .artist, .context { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .feature-meta .title { font-weight: 650; letter-spacing: -.025em; font-size: 1.05rem; }
  .context { color: hsl(var(--muted-foreground)); font-size: .66rem; margin-bottom: 2px; }
  .artist, .duration { color: hsl(var(--muted-foreground)); font-size: .73rem; line-height: 1.4; }
  .duration { font-variant-numeric: tabular-nums; font-size: .68rem; }
  h3 { padding: 18px 1px 7px; font-weight: 600; font-size: .8rem; }
  .recent button { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 47px; padding: 3px; text-align: start; border-radius: 7px; }
  .recent .title { font-size: .78rem; font-weight: 550; }
  .recent .artist { font-size: .69rem; }
  .thumb { width: 33px; height: 33px; flex: none; overflow: hidden; border-radius: 6px; }
  .empty { display: flex; align-items: center; justify-content: center; min-height: 76px; gap: 8px; color: hsl(var(--muted-foreground)); font-size: .8rem; }
  :global(.workspace-sheet) .queue-panel { padding: 0 20px; }
  :global(.workspace-sheet) .feature-art { width: min(100%, 270px); margin: auto; }
  :global(.workspace-sheet) .heading { min-height: 56px; }
</style>
