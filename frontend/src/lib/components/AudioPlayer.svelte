<script lang="ts">
  import { availabilityLabel, canPlay } from "$lib/catalog/availability";
  import { ChevronUp, Music2, Play, Pause, SkipBack, SkipForward, RotateCw, Repeat, Shuffle, Infinity as Autoplay, ListMusic, Volume1, Volume2, VolumeX, Square, Maximize2 } from "lucide-svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import { showcaseAudio } from "$lib/showcase";
  import { favoriteQuery } from "$lib/catalog/favorites.svelte";
  import { api } from "$lib/api";
  import { playerArtwork } from "./player/artwork";
  import PlayerRange from "./player/PlayerRange.svelte";
  import { formatDuration } from "$lib/utils";
  import { t } from "$lib/i18n/index.svelte";
  import CatalogArtwork from "./catalog/CatalogArtwork.svelte";
  import FavoriteButton from "./FavoriteButton.svelte";
  import PodcastPlayer from "$lib/webradios/PodcastPlayer.svelte";
  import { podcast } from "$lib/webradios/podcast.svelte";
  import LivePlayer from "$lib/webradios/LivePlayer.svelte";
  import { live } from "$lib/webradios/live.svelte";
  import ExpandedPlayer from "./ExpandedPlayer.svelte";

  let { queueVisible = false, queueAsDialog = true, toggleQueue }: {
    queueVisible?: boolean; queueAsDialog?: boolean; toggleQueue?: (trigger: HTMLElement) => void;
  } = $props();
  const player = getPlayerStore();
  const queue = getQueueStore();
  const auth = getAuthStore();
  const radio = getRadioStore();
  const track = $derived(player.currentTrack);
  const demo = $derived(track && showcaseAudio(track.id));
  const cover = $derived(track && playerArtwork(track));
  const progress = $derived(player.duration > 0 ? Math.min(100, Math.max(0, player.progress / player.duration * 100)) : 0);

  const favorites = favoriteQuery(signal => track && !demo && auth.isAuthenticated
    ? api.get<string[]>("/favorites/check?track_ids=" + encodeURIComponent(track.id), signal)
    : null);
  let expanded = $state(false);
  let panel = $state<"track" | "queue" | "lyrics">("track");
  let lastAudibleVolume = 0.8;

  function openPlayer(view: "track" | "queue") {
    panel = view;
    expanded = true;
  }

  function playOrPause() {
    if (track) player.togglePlay();
    else queue.playQueue(queue.queue, 0, queue.sourceContext ?? "queue");
  }

  function toggleMute() {
    if (player.volume > 0) {
      lastAudibleVolume = player.volume;
      player.setVolume(0);
    } else {
      player.setVolume(lastAudibleVolume);
    }
  }

  function adjustVolume(value: number) {
    if (value > 0) lastAudibleVolume = value;
    player.setVolume(value);
  }
</script>

{#if podcast.episode}
  <PodcastPlayer />
{:else if live.station}
  <LivePlayer />
{:else}
<section class={["player", { idle: !track }]} aria-label={t("player.nowPlaying")} data-testid={track ? "audio-player" : "idle-player"} style:--progress={progress + "%"}>
  <div class="player-body">
    <div class="identity">
      {#if track}
        <button type="button" class="open-player" onclick={() => openPlayer("track")} aria-haspopup="dialog"
          aria-label={t("player.expand") + " · " + track.title}>
          <span class="artwork" aria-hidden="true">
            <CatalogArtwork source={cover} title={track.album_title || track.title} kind="album" artworkId={track.album_id ?? track.id} loading="eager" />
          </span>
          <span class="metadata">
            <span class="song">{track.title}</span>
            {#if player.failed}<span class="artist" role="alert">{availabilityLabel(track) || t("player.playbackError")}</span>
            {:else if track.artist_name}<span class="artist">{track.artist_name}</span>{/if}
            {#if radio.active}<span class="radio-status">{t(radio.autoplayMode ? "player.autoplayActive" : "radio.nowPlaying")}</span>{/if}
          </span>
          <ChevronUp size={15} class="mobile-chevron" />
        </button>
      {:else}
        <div class="open-player resting">
          <span class="artwork"><Music2 size={20} strokeWidth={1.4} /></span>
          <span class="metadata"><span class="song">{queue.queue[0]?.title ?? "SoundTime"}</span><span class="artist">{t(queue.queue.length ? "player.upNext" : "player.queueEmpty")}</span></span>
        </div>
      {/if}
      {#if track && !demo && auth.isAuthenticated}
        <span class="favorite">
          {#if favorites.failed}
            <button type="button" title={t("radio.tryAgain")} onclick={favorites.retry}><RotateCw size={16} /></button>
          {:else}
            <FavoriteButton trackId={track.id} liked={favorites.data?.includes(track.id) ?? false} disabled={favorites.loading} size={16} />
          {/if}
        </span>
      {/if}
    </div>

    <div class="transport">
      <div class="controls">
        <button type="button" class="mode" aria-pressed={player.shuffle} title={t("player.shuffle")} onclick={player.toggleShuffle}><Shuffle size={16}/></button>
        <button type="button" class="skip previous" disabled={!track} title={t("player.previous")} onclick={queue.previous}><SkipBack size={19} fill="currentColor" /></button>
        <button type="button" class="play" disabled={track ? !canPlay(track) && track.availability !== "temporarily_unavailable" : !queue.queue.some(canPlay)} aria-label={t(player.failed ? "radio.tryAgain" : player.isPlaying ? "shell.pause" : "shell.play")} onclick={playOrPause}>
          {#if player.failed}<RotateCw size={17} />{:else if player.isPlaying}<Pause size={17} fill="currentColor" />{:else}<Play size={17} fill="currentColor" />{/if}
        </button>
        <button type="button" class="skip" disabled={!track} title={t("player.next")} onclick={queue.next}><SkipForward size={19} fill="currentColor" /></button>
        <button type="button" class="mode relative" aria-pressed={player.repeat !== "none"} title={t("player.repeat") + " · " + player.repeat} onclick={player.cycleRepeat}>
          <Repeat size={16}/>{#if player.repeat === "one"}<span class="repeat-count">1</span>{/if}
        </button>
      </div>
      <div class="timeline">
        <span>{formatDuration(player.progress)}</span>
        <PlayerRange label={t("shell.seek")} valueText={formatDuration(player.progress)} max={player.duration || 1}
          step="any" value={player.progress} disabled={!player.duration} onchange={player.seek} />
        <span>{formatDuration(player.duration)}</span>
      </div>
    </div>
    <div class="utilities">
      {#if radio.active}<button type="button" title={t("radio.stop")} onclick={radio.stopRadio}><Square size={14} /></button>{/if}
      {#if !demo}<button type="button" class="autoplay" title={t("player.autoplay")} aria-pressed={queue.autoplay} onclick={queue.toggleAutoplay}><Autoplay size={18} /></button>{/if}
      <div class="volume">
        <button type="button" title={t(player.volume > 0 ? "player.mute" : "player.unmute")} onclick={toggleMute}>
          {#if player.volume === 0}<VolumeX size={17} />{:else if player.volume < .5}<Volume1 size={17} />{:else}<Volume2 size={17} />{/if}
        </button>
        <PlayerRange label={t("player.volume")} max={1} step={0.01} value={player.volume} onchange={adjustVolume} />
      </div>
      <div class="views">
        <button type="button" title={t("player.queue")} data-dock-queue aria-expanded={queueVisible}
          aria-controls={queueVisible ? "workspace-queue" : undefined} aria-haspopup={queueAsDialog ? "dialog" : undefined}
          disabled={!track && !toggleQueue} onclick={event => toggleQueue ? toggleQueue(event.currentTarget) : openPlayer("queue")}><ListMusic size={19} /></button>
        <button type="button" class="expand" disabled={!track} title={t("player.expand")} aria-haspopup="dialog" onclick={() => openPlayer("track")}><Maximize2 size={16} /></button>
      </div>
    </div>
  </div>
</section>
{/if}

<ExpandedPlayer bind:open={expanded} bind:panel onclose={() => expanded = false} />

<style>
  .player {
    --slider-fill: hsl(var(--foreground)); --slider-track: hsl(var(--foreground) / .19);
    --slider-height: 3px; --slider-thumb-size: 8px;
    width: 100%; flex: none; position: relative; z-index: 3; container: player / inline-size;
    color: hsl(var(--foreground)); background: hsl(var(--card)); border-top: 1px solid hsl(var(--border));
  }
  .player-body { display: grid; grid-template-columns: minmax(0,1fr) minmax(200px,1.25fr) minmax(0,1fr); align-items: center; gap: clamp(16px, 2.6cqi, 44px); min-height: 68px; margin: auto; padding: 5px clamp(14px,2cqi,30px); }
  .identity, .open-player, .controls, .utilities, .volume, .views { display: flex; align-items: center; }
  .identity { min-width: 0; gap: 9px; }
  .open-player { min-width: 0; gap: 12px; text-align: start; border-radius: 8px; }
  button.open-player:hover .song { text-decoration: underline; text-underline-offset: 3px; }
  .artwork { display: grid; place-items: center; flex: none; width: 43px; height: 43px; overflow: hidden; border-radius: 8px; background: hsl(var(--muted)); color: hsl(var(--muted-foreground)); }
  .metadata { min-width: 0; }
  .metadata span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .song { font-size: .88rem; line-height: 1.45; font-weight: 600; }
  .artist, .radio-status { color: hsl(var(--muted-foreground)); font-size: .76rem; line-height: 1.45; }
  .radio-status { font-size: .68rem; }
  .open-player :global(.mobile-chevron) { display: none; }
  .favorite { display: grid; place-items: center; flex: none; }
  .transport { width: 100%; max-width: 480px; justify-self: center; }
  .controls { justify-content: center; gap: clamp(4px, .7cqi, 12px); }
  .controls button, .utilities button { display: grid; place-items: center; flex: none; width: 32px; height: 32px; border-radius: 7px; }
  .controls button:hover:enabled, .utilities button:hover:enabled { background: hsl(var(--muted)); }
  .controls button:disabled, .utilities button:disabled { opacity: .42; }
  .controls .mode, .utilities { color: hsl(var(--muted-foreground)); }
  .controls [aria-pressed="true"], .utilities [aria-pressed="true"], .utilities [aria-expanded="true"] { color: hsl(var(--link)); }
  .controls .play { width: 35px; height: 35px; border-radius: 50%; background: hsl(var(--foreground)); color: hsl(var(--background)); }
  .controls .play:hover:enabled { background: hsl(var(--foreground) / .85); }
  .repeat-count { position: absolute; font-size: 8px; line-height: 1; font-weight: 700; }
  .timeline { display: grid; grid-template-columns: auto minmax(0,1fr) auto; align-items: center; gap: 9px; color: hsl(var(--muted-foreground)); font-size: .66rem; line-height: 1; font-variant-numeric: tabular-nums; }
  .utilities { justify-content: flex-end; gap: 4px; }
  .volume { gap: 3px; min-width: 0; width: clamp(72px, 8cqi, 113px); --slider-fill: hsl(var(--muted-foreground)); }
  .views { gap: 2px; margin-inline-start: 6px; }
  @media (max-width: 999px) {
    .player-body { gap: 12px; padding-inline: 18px; grid-template-columns: minmax(0,1fr) minmax(180px,1fr) auto; }
    .utilities { gap: 1px; }
    .volume { width: 70px; }
  }
  @media (max-width: 767px) {
    .player.idle { display: none; }
    .player-body { grid-template-columns: minmax(0,1fr) auto; min-height: 64px; gap: 9px; padding: 6px 12px max(6px, env(safe-area-inset-bottom)); }
    .player::before { content: ""; position: absolute; inset: 0 0 auto; height: 2px; pointer-events: none; background: linear-gradient(to right,var(--slider-fill) var(--progress),transparent var(--progress)); }
    .open-player { min-height: 44px; gap: 10px; }
    .artwork { width: 43px; height: 43px; }
    .open-player :global(.mobile-chevron) { display: block; flex: none; color: hsl(var(--muted-foreground)); }
    .transport { width: auto; }
    .controls { gap: 3px; }
    .controls button { width: 38px; height: 44px; }
    .controls .play { width: 39px; height: 39px; }
    .controls .mode, .controls .previous, .timeline, .utilities { display: none; }
  }
</style>
