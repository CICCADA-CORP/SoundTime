<script lang="ts">
  import { availabilityLabel, canPlay } from "$lib/catalog/availability";
  import { ChevronUp, Disc3, Play, Pause, SkipBack, SkipForward, RotateCw, Repeat, Shuffle, Infinity as Autoplay, ListMusic, Volume1, Volume2, VolumeX, Square, Maximize2 } from "lucide-svelte";
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
  import FavoriteButton from "./FavoriteButton.svelte";
  import PodcastPlayer from "$lib/webradios/PodcastPlayer.svelte";
  import { podcast } from "$lib/webradios/podcast.svelte";
  import LivePlayer from "$lib/webradios/LivePlayer.svelte";
  import { live } from "$lib/webradios/live.svelte";
  import ExpandedPlayer from "./ExpandedPlayer.svelte";

  const player = getPlayerStore();
  const queue = getQueueStore();
  const auth = getAuthStore();
  const radio = getRadioStore();
  const track = $derived(player.currentTrack);
  const demo = $derived(track && showcaseAudio(track.id));
  const cover = $derived(track && playerArtwork(track));
  const progress = $derived(player.duration > 0 ? Math.min(100, Math.max(0, player.progress / player.duration * 100)) : 0);

  const favorites = favoriteQuery(() => track && !demo && auth.isAuthenticated
    ? api.get<string[]>("/favorites/check?track_ids=" + encodeURIComponent(track.id))
    : null);
  let expanded = $state(false);
  let panel = $state<"track" | "queue" | "lyrics">("track");
  let lastAudibleVolume = 0.8;

  function openPlayer(view: "track" | "queue") {
    panel = view;
    expanded = true;
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
{:else if track}
  <section class="player" aria-label={t("player.nowPlaying")} data-testid="audio-player" style:--progress={progress + "%"}>
    <div class="player-body">
      <div class="identity">
        <button type="button" class="open-player" onclick={() => openPlayer("track")} aria-haspopup="dialog"
          aria-label={t("player.expand") + " · " + track.title}>
          <span class="artwork">
            {#if cover}<img src={cover} alt="" />{:else}<Disc3 size={25} strokeWidth={1.5} />{/if}
          </span>
          <span class="metadata">
            <span class="song">{track.title}</span>
            {#if player.failed}<span class="artist" role="alert">{availabilityLabel(track) || t("player.playbackError")}</span>
            {:else if track.artist_name}<span class="artist">{track.artist_name}</span>{/if}
            {#if radio.active}<span class="radio-status">{t(radio.autoplayMode ? "player.autoplayActive" : "radio.nowPlaying")}</span>{/if}
          </span>
          <ChevronUp size={15} class="mobile-chevron" />
        </button>
        {#if !demo && auth.isAuthenticated}
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
          <button type="button" class="skip previous" title={t("player.previous")} onclick={queue.previous}><SkipBack size={19} fill="currentColor" /></button>
          <button type="button" class="play" disabled={!canPlay(track) && track.availability !== "temporarily_unavailable"} aria-label={t(player.failed ? "radio.tryAgain" : player.isPlaying ? "shell.pause" : "shell.play")} onclick={player.togglePlay}>
            {#if player.failed}<RotateCw size={17} />{:else if player.isPlaying}<Pause size={17} fill="currentColor" />{:else}<Play size={17} fill="currentColor" />{/if}
          </button>
          <button type="button" class="skip" title={t("player.next")} onclick={queue.next}><SkipForward size={19} fill="currentColor" /></button>
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
          <button type="button" title={t("player.queue")} aria-haspopup="dialog" onclick={() => openPlayer("queue")}><ListMusic size={19} /></button>
          <button type="button" class="expand" title={t("player.expand")} aria-haspopup="dialog" onclick={() => openPlayer("track")}><Maximize2 size={16} /></button>
        </div>
      </div>
    </div>
  </section>
{/if}

<ExpandedPlayer bind:open={expanded} bind:panel onclose={() => expanded = false} />

<style>
  .player {
    --slider-fill: hsl(var(--foreground));
    --slider-track: hsl(var(--border));
    --slider-height: 3px;
    --slider-thumb-size: 7px;
    position: fixed;
    z-index: 46;
    inset: auto 0 0 var(--workspace-left, var(--rail-width));
    container: player / inline-size;
    border-top: 1px solid hsl(var(--border));
    background: hsl(var(--background));
    color: hsl(var(--foreground));
  }
  .player-body {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(180px, 1.15fr) minmax(0, 1fr);
    align-items: center;
    gap: clamp(12px, 2cqi, 28px);
    min-height: 75px;
    padding: 7px clamp(16px, 3cqi, 40px);
  }
  .identity, .open-player, .controls, .utilities, .volume, .views { display: flex; align-items: center; }
  .identity { min-width: 0; gap: 8px; }
  .open-player { min-width: 0; gap: 12px; border-radius: 4px; text-align: left; }
  .open-player:hover .song { text-decoration: underline; text-underline-offset: 3px; }
  .artwork {
    display: grid; flex: none; place-items: center; width: 48px; height: 48px;
    overflow: hidden; border-radius: 5px; background: hsl(var(--muted)); color: hsl(var(--muted-foreground));
  }
  .artwork img { width: 100%; height: 100%; object-fit: cover; }
  .metadata { min-width: 0; }
  .metadata span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .song { font-size: 14px; line-height: 1.4; font-weight: 600; }
  .artist, .radio-status { color: hsl(var(--muted-foreground)); font-size: 12px; line-height: 1.5; }
  .radio-status { font-size: 10px; }
  .open-player :global(.mobile-chevron) { display: none; }
  .favorite { display: grid; place-items: center; flex-shrink: 0; }
  .transport { width: 100%; max-width: 376px; min-width: 0; justify-self: center; }
  .controls { justify-content: center; gap: clamp(6px, 1cqi, 14px); }
  .controls button, .utilities button {
    display: grid; flex: none; place-items: center; width: 32px; height: 32px;
    border-radius: 5px; color: hsl(var(--muted-foreground)); transition: background-color .15s, color .15s;
  }
  .controls button:hover, .utilities button:hover, .controls [aria-pressed="true"], .utilities [aria-pressed="true"] {
    background: hsl(var(--secondary)); color: hsl(var(--foreground));
  }
  .controls .skip { color: hsl(var(--foreground)); }
  .controls .play {
    width: 34px; height: 34px; border-radius: 50%;
    background: hsl(var(--foreground)); color: hsl(var(--background));
  }
  .controls .play:hover { background: hsl(var(--foreground) / .83); color: hsl(var(--background)); }
  .repeat-count { position: absolute; top: 7px; left: 13px; font-size: 9px; font-weight: 700; }
  .timeline {
    display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center;
    gap: 8px; color: hsl(var(--muted-foreground)); font-size: 10px; font-variant-numeric: tabular-nums;
  }
  .utilities { justify-content: flex-end; gap: 2px; }
  .volume { gap: 1px; min-width: 0; width: clamp(68px, 9cqi, 100px); }
  .views { gap: 1px; border-left: 1px solid hsl(var(--border)); padding-left: 7px; margin-left: 7px; }

  @container player (max-width: 825px) {
    .player-body { grid-template-columns: minmax(0, 1fr) minmax(160px, 1.05fr) auto; }
    .volume { width: 72px; }
  }
  @container player (max-width: 665px) {
    .volume :global(input), .views .expand { display: none; }
    .volume { width: auto; }
    .views { margin: 0; padding: 0; border: 0; }
  }
  @media (max-width: 767px) {
    .player { inset: auto 0 var(--mobile-nav); }
    .player-body { grid-template-columns: minmax(0, 1fr) auto; gap: 7px; min-height: 61px; padding: 6px 12px; }
    .player::after {
      content: ""; position: absolute; inset: auto 0 0; height: 2px; pointer-events: none;
      background: linear-gradient(to right, var(--slider-fill) var(--progress), var(--slider-track) var(--progress));
    }
    .artwork { width: 43px; height: 43px; }
    .open-player { min-height: 44px; gap: 10px; }
    .open-player :global(.mobile-chevron) { display: block; flex: none; color: hsl(var(--muted-foreground)); }
    .transport { width: auto; }
    .controls { gap: 0; }
    .controls button { width: 44px; height: 44px; }
    .controls .play {
      width: 44px; height: 44px; border: 5px solid transparent; background-clip: padding-box;
    }
    .controls .mode, .timeline, .utilities { display: none; }
  }
</style>
