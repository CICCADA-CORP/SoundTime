<script lang="ts">
  import { ExternalLink, Pause, Play, RotateCw, Square } from "lucide-svelte";
  import PlayerRange from "$lib/components/player/PlayerRange.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { formatDuration } from "$lib/utils";
  import PodcastCover from "./PodcastCover.svelte";
  import { podcast } from "./podcast.svelte";

  const player = getPlayerStore();
  const show = $derived(podcast.show);
  const episode = $derived(podcast.episode);

  function elapsed(seconds: number) {
    if (seconds < 3600) return formatDuration(seconds);
    return Math.floor(seconds / 3600) + ":" + formatDuration(seconds % 3600).padStart(5, "0");
  }
</script>

{#if show && episode}
  <section class="episode-bar" aria-label={t("player.nowPlaying")}>
    <div class="art"><PodcastCover source={show.artwork_url} /></div>
    <div class="identity">
      <p class="truncate text-sm font-semibold">{episode.title}</p>
      <a href={podcast.showUrl} class="block truncate text-xs text-muted-foreground hover:underline">{show.title}</a>
    </div>
    <div class="transport">
      {#if podcast.phase === "failed"}
        <button type="button" aria-label={t("radio.tryAgain")} onclick={() => player.playPodcast(show, episode, podcast.country)}><RotateCw size={17} /></button>
      {:else if podcast.phase !== "unsupported"}
        <button type="button" disabled={podcast.phase === "loading"}
          aria-label={t(podcast.phase === "playing" ? "webradios.podcastPause" : "webradios.podcastPlay", { title: episode.title })}
          onclick={() => podcast.phase === "playing" ? podcast.pause() : podcast.resume()}>
          {#if podcast.phase === "playing"}<Pause size={17} fill="currentColor" />{:else}<Play size={17} fill="currentColor" />{/if}
        </button>
      {/if}
      <button type="button" aria-label={t("webradios.podcastStop", { title: episode.title })} onclick={podcast.stop}><Square size={15} /></button>
    </div>
    {#if podcast.phase === "failed" || podcast.phase === "unsupported"}
      <p class="timeline text-xs text-muted-foreground" role="status">
        {t(podcast.phase === "unsupported" ? "webradios.podcastInsecure" : "webradios.playError")}
        {#if podcast.url}
          <a href={podcast.url} target="_blank" rel="noreferrer" class="inline-flex items-center gap-1 underline">{t("webradios.podcastExternal")}<ExternalLink size={13} /></a>
        {/if}
      </p>
    {:else}
      <div class="timeline">
        <span>{elapsed(podcast.progress)}</span>
        <PlayerRange label={t("shell.seek")} valueText={elapsed(podcast.progress)} value={podcast.progress} max={podcast.duration || 1}
          disabled={!podcast.duration || podcast.phase === "loading"} step="any" onchange={podcast.seek} />
        <span>{podcast.duration ? elapsed(podcast.duration) : "–"}</span>
      </div>
    {/if}
    <div class="volume"><PlayerRange label={t("player.volume")} max={1} step={0.01} value={player.volume} onchange={player.setVolume} /></div>
  </section>
{/if}

<style>
  .episode-bar { position: fixed; z-index: 46; inset: auto 0 0 var(--workspace-left, var(--rail-width)); min-height: 75px;
    display: grid; grid-template-columns: 44px minmax(90px, 1fr) auto minmax(100px, 1fr) minmax(60px, 96px);
    align-items: center; gap: 12px; padding: 8px clamp(16px, 3vw, 40px);
    border-top: 1px solid hsl(var(--border)); background: hsl(var(--background)); }
  .art { width: 44px; height: 44px; }
  .identity, .timeline { min-width: 0; }
  .transport, .timeline { display: flex; align-items: center; gap: 8px; }
  .transport button { width: 40px; height: 40px; display: grid; place-items: center; border-radius: 50%; border: 1px solid hsl(var(--border)); }
  .transport button:disabled { opacity: .5; }
  .timeline span { font-size: 11px; color: hsl(var(--muted-foreground)); font-variant-numeric: tabular-nums; }
  @media (max-width: 767px) {
    .episode-bar { inset: auto 0 var(--mobile-nav); grid-template-columns: 36px minmax(0, 1fr) auto; gap: 1px 10px; padding: 4px 12px; }
    .art { width: 36px; height: 36px; }
    .timeline { grid-column: 1 / -1; }
    .volume { display: none; }
  }
</style>
