<script lang="ts">
  import { Dialog } from "bits-ui";
  import { AudioLines, ChevronDown, Clock3, Disc3, Infinity as Autoplay, ListMusic, Mic2, Pause, Play, Repeat2, Shuffle, SkipBack, SkipForward, RotateCw, Square, Volume1, Volume2, VolumeX } from "lucide-svelte";
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { showcaseAudio } from "$lib/showcase";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import PlayerRange from "./player/PlayerRange.svelte";
  import UpcomingTracks from "./player/UpcomingTracks.svelte";
  import { playerArtwork } from "./player/artwork";
  import SoundTimeSignature from "./SoundTimeSignature.svelte";
  import { formatDuration } from "$lib/utils";
  import { Button } from "$lib/components/ui/button";
  import PlayerSaveActions from "./player/PlayerSaveActions.svelte";

  type Panel = "track" | "queue" | "lyrics";
  let { open = $bindable(), panel = $bindable<Panel>("track"), onclose }: {
    open: boolean;
    panel?: Panel;
    onclose: () => void;
  } = $props();
  const auth = getAuthStore();
  const player = getPlayerStore();
  const queue = getQueueStore();
  const radio = getRadioStore();
  const track = $derived(player.currentTrack);
  const cover = $derived(track && playerArtwork(track));
  const visible = $derived(open && track !== null);
  const catalogId = $derived(visible && track && !showcaseAudio(track.id) ? track.id : null);
  const upcoming = $derived(queue.currentIndex < 0 ? [] : queue.queue.slice(queue.currentIndex + 1));
  const upcomingDuration = $derived(upcoming.reduce((time, song) => time + Math.max(0, song.duration_secs || 0), 0));
  const lyrics = catalogQuery(() => catalogId && panel === "lyrics"
    ? api.get<{ lyrics: string | null; source: string | null }>(`/tracks/${catalogId}/lyrics`) : null);
  function close() {
    panel = "track";
    open = false;
    onclose();
  }

  const panels = [
    { id: "track", label: "player.track", icon: Disc3 },
    { id: "queue", label: "player.upNext", icon: ListMusic },
    { id: "lyrics", label: "player.lyrics", icon: Mic2 },
  ] as const;
</script>

<Dialog.Root open={visible} onOpenChange={value => { if (!value) close(); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-[100] bg-[#151518]" />
    <Dialog.Content class="expanded-dialog fixed inset-0 z-[101] overflow-y-auto">
      {#if track}
        <div class="canvas">
          {#if cover}
            <div class="ambience" aria-hidden="true"><img src={cover} alt="" /></div>
          {/if}
          <header class="player-header">
            <span class="signature"><SoundTimeSignature width={122} /></span>
            <Dialog.Title class="player-caption"><AudioLines size={17} />{t("player.nowPlaying")}</Dialog.Title>
            <Button variant="ghost" size="icon" class="collapse-player" onclick={close}
              aria-label={t("a11y.close")} title={t("a11y.close")}>
              <ChevronDown size={20} />
            </Button>
          </header>
          <Dialog.Description class="sr-only">{track.title} · {track.artist_name ?? t("track.unknownArtist")}</Dialog.Description>

          <div class="stage" data-panel={panel}>
            <div class="deck">
              <div class="album-art">
                {#if cover}<img src={cover} alt="" />{:else}<Disc3 size={76} strokeWidth={1} />{/if}
              </div>

              <div class="track-heading">
                <div class="min-w-0 flex-1">
                  <h2>{track.title}</h2>
                  <p class="byline">
                    {#if catalogId && track.artist_id}<a href={"/artists/" + track.artist_id} onclick={close}>{track.artist_name ?? t("track.unknownArtist")}</a>
                    {:else}{track.artist_name ?? t("track.unknownArtist")}{/if}
                    {#if track.album_title}
                      <span class="separator" aria-hidden="true">·</span>
                      {#if catalogId && track.album_id}<a class="album" href={"/albums/" + track.album_id} onclick={close}>{track.album_title}</a>
                      {:else}<span class="album">{track.album_title}</span>{/if}
                    {/if}
                  </p>
                </div>
                {#if catalogId && auth.isAuthenticated}
                  {#key catalogId}<PlayerSaveActions {track} />{/key}
                {/if}
              </div>

              {#if player.failed}<p role="alert">{t("player.playbackError")}</p>{/if}
              <div class="seek">
                <PlayerRange max={Math.max(1, player.duration)} disabled={!player.duration} label={t("shell.seek")}
                  valueText={formatDuration(player.progress)} value={player.progress} onchange={player.seek} />
                <div class="time"><span>{formatDuration(player.progress)}</span><span>{formatDuration(player.duration)}</span></div>
              </div>
              <div class="transport">
                <button class="mode" aria-label={t("player.shuffle")} aria-pressed={player.shuffle} onclick={player.toggleShuffle}><Shuffle size={18} /></button>
                <button class="skip" aria-label={t("player.previous")} onclick={queue.previous}><SkipBack size={25} fill="currentColor" /></button>
                <Button class="main-play" aria-label={t(player.failed ? "radio.tryAgain" : player.isPlaying ? "shell.pause" : "shell.play")} onclick={player.togglePlay}>
                  {#if player.failed}<RotateCw />{:else if player.isPlaying}<Pause fill="currentColor" />{:else}<Play fill="currentColor" />{/if}
                </Button>
                <button class="skip" aria-label={t("player.next")} onclick={queue.next}><SkipForward size={25} fill="currentColor" /></button>
                <button class="mode" aria-label={t("player.repeat") + " · " + player.repeat} aria-pressed={player.repeat !== "none"} onclick={player.cycleRepeat}>
                  <Repeat2 size={18} />{#if player.repeat === "one"}<span class="repeat-one">1</span>{/if}
                </button>
              </div>
              <div class="deck-footer">
                {#if track.format || track.bitrate || track.sample_rate}
                  <div class="quality" aria-label={t("track.quality")}>
                    <AudioLines size={13} />
                    {#if track.format}<span class="format">{track.format}</span>{/if}
                    {#if track.bitrate}<span>{track.bitrate} kbps</span>{/if}
                    {#if track.sample_rate}<span>{(track.sample_rate / 1000).toFixed(1)} kHz</span>{/if}
                  </div>
                {/if}
                {#if radio.active}
                  <div class="radio-status">
                    <span>{radio.autoplayMode ? t("player.autoplayActive") : t("radio.nowPlayingFrom") + " " + radio.seedLabel}</span>
                    <Button variant="ghost" size="sm" onclick={radio.stopRadio}><Square size={12} />{t("radio.stop")}</Button>
                  </div>
                {/if}
              </div>
            </div>

            <section class="side-panel" aria-label={t(panel === "lyrics" ? "player.lyrics" : "player.queue")}>
              <div class="panel-head">
                <div class="desktop-tabs">
                  <button type="button" aria-pressed={panel !== "lyrics"} onclick={() => panel = "queue"}><ListMusic size={17} />{t("player.queue")}</button>
                  <button type="button" aria-pressed={panel === "lyrics"} onclick={() => panel = "lyrics"}><Mic2 size={16} />{t("player.lyrics")}</button>
                </div>
                {#if catalogId}
                  <button class="autoplay" title={t("player.autoplay")} aria-label={t("player.autoplay")} aria-pressed={queue.autoplay} onclick={queue.toggleAutoplay}>
                    <Autoplay size={19} />
                  </button>
                {/if}
              </div>

              <div class="panel-content">
                {#if panel === "lyrics"}
                  <div class="lyrics">
                    {#if lyrics.loading}<p role="status" class="empty">{t("common.loading")}</p>
                    {:else if lyrics.failed}
                      <p role="alert" class="empty">{t("player.lyricsError")}</p>
                      <Button variant="outline" size="sm" onclick={lyrics.retry}>{t("radio.tryAgain")}</Button>
                    {:else if lyrics.data?.lyrics}
                      <p class="lyrics-text">{lyrics.data.lyrics}</p>
                      {#if lyrics.data.source}<p class="mt-3 text-xs text-muted-foreground">{t("player.source")} · {lyrics.data.source}</p>{/if}
                    {:else}
                      <div class="empty empty-lyrics"><Mic2 size={27} strokeWidth={1.35} /><span>{t("player.noLyrics")}</span></div>
                    {/if}
                  </div>
                {:else}
                  <div class="current-row">
                    <span class="current-cover">{#if cover}<img src={cover} alt="" />{:else}<Disc3 size={20} />{/if}</span>
                    <div class="min-w-0 flex-1">
                      <p class="current-label">{t("player.nowPlaying")}</p>
                      <p class="current-title">{track.title}</p>
                      <p class="row-artist">{track.artist_name ?? t("track.unknownArtist")}</p>
                    </div>
                    <div class="level" data-playing={player.isPlaying} aria-hidden="true"><i></i><i></i><i></i><i></i></div>
                  </div>
                  <div class="queue-heading">
                    <h3>{t("player.upNext")}{#if upcoming.length}<span class="queue-count">{upcoming.length}</span>{/if}</h3>
                    {#if upcomingDuration > 0}
                      <span class="queue-duration" title={t("album.totalDuration")}><Clock3 size={12} /><span class="sr-only">{t("album.totalDuration")} </span>{formatDuration(upcomingDuration)}</span>
                    {/if}
                  </div>
                  {#if !upcoming.length}
                    <p class="empty">{t("player.queueEmpty")}</p>
                  {:else}
                    <UpcomingTracks tracks={upcoming}
                      onplay={offset => queue.playQueue(queue.queue, queue.currentIndex + offset + 1, queue.sourceContext ?? "queue")}
                      onmove={(offset, direction) => {
                        const index = queue.currentIndex + offset + 1;
                        queue.moveInQueue(index, index + direction);
                      }}
                      onremove={offset => queue.removeFromQueue(queue.currentIndex + offset + 1)} />
                  {/if}
                {/if}
              </div>
              <div class="volume">
                {#if !player.volume}<VolumeX size={16} />{:else if player.volume < .5}<Volume1 size={16} />{:else}<Volume2 size={16} />{/if}
                <div class="volume-range">
                  <PlayerRange label={t("player.volume")} max={1} step={.01} value={player.volume} onchange={player.setVolume} />
                </div>
                <span>{Math.round(player.volume * 100)}%</span>
              </div>
            </section>
          </div>

          <nav class="mobile-tabs" aria-label={t("player.nowPlaying")}>
            {#each panels as item (item.id)}
              <button type="button" aria-pressed={panel === item.id} onclick={() => panel = item.id}>
                <item.icon size={18} />{t(item.label)}
              </button>
            {/each}
          </nav>
        </div>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.expanded-dialog) {
    --foreground: 0 0% 96%;
    --background: 240 6% 9%;
    --muted: 240 5% 20%;
    --muted-foreground: 240 5% 70%;
    --border: 240 4% 25%;
    --popover: 240 6% 16%;
    --popover-foreground: var(--foreground);
    --accent: 240 5% 23%;
    --accent-foreground: var(--foreground);
    --signal: color-mix(in srgb, hsl(var(--primary)) 55%, #ffb5ca);
    --slider-track: rgb(255 255 255 / .2);
    --slider-fill: #f2f0f0;
    --slider-height: 3px;
    --slider-thumb-size: 9px;
    height: 100dvh; overscroll-behavior: contain; color-scheme: dark;
    background: hsl(var(--background)); color: hsl(var(--foreground));
  }
  .canvas {
    position: relative; isolation: isolate; min-height: 100%; display: flex; flex-direction: column;
    padding: 0 clamp(24px, 5vw, 84px); background: radial-gradient(ellipse at 0% 5%, #ffffff08, transparent 75%);
  }
  .ambience { position: absolute; z-index: -1; inset: 0; overflow: hidden; pointer-events: none; }
  .ambience img { position: absolute; top: -38%; left: -22%; width: 105%; height: 140%; object-fit: cover; filter: blur(130px) saturate(.8); opacity: .32; }
  .ambience::after { content: ""; position: absolute; inset: 0; background: linear-gradient(100deg, transparent 2%, #10101315 45%, #101013c9 90%), linear-gradient(0deg, #111115b3, transparent 65%); }
  .player-header { flex: none; display: flex; align-items: center; gap: 23px; height: 70px; border-bottom: 1px solid rgb(255 255 255 / .09); }
  .signature { color: #f4f3f4; }
  :global(.player-caption) { display: inline-flex; align-items: center; gap: 8px; padding-left: 23px; border-left: 1px solid rgb(255 255 255 / .18); color: #c9c5c8; font-size: .78rem; font-weight: 550; }
  :global(.player-caption svg) { color: var(--signal); }
  :global(.collapse-player) { margin-left: auto; width: 40px; height: 40px; border: 1px solid #ffffff23; border-radius: 50%; color: #f1edef; background: #ffffff09; }
  :global(.collapse-player svg) { width: 19px; height: 19px; }
  .stage {
    flex: 1; width: 100%; max-width: 1010px; min-height: calc(100dvh - 70px); margin-inline: auto;
    display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 1fr); align-items: center; gap: clamp(48px, 7vw, 112px);
    padding-block: clamp(22px, 4.5dvh, 64px);
  }
  .deck { width: 100%; max-width: min(100%, max(235px, calc(100dvh - 372px)), 465px); justify-self: center; }
  .album-art {
    display: grid; aspect-ratio: 1; place-items: center; overflow: hidden; color: #a8a5ae; background: linear-gradient(135deg, #48464d, #27262b);
    border-radius: 12px; box-shadow: 0 0 0 1px #ffffff19, 0 24px 50px -18px #000a;
  }
  .album-art img, .current-cover img { width: 100%; height: 100%; object-fit: cover; }
  .track-heading { display: flex; align-items: center; gap: .7rem; margin-top: clamp(18px, 3dvh, 31px); }
  .track-heading h2 { margin: 0; font-size: clamp(1.8rem, 3vw, 2.55rem); line-height: 1.06; font-weight: 680; letter-spacing: -.048em; overflow-wrap: anywhere; }
  .byline { margin: 6px 0 0; color: #dedadd; font-size: .96rem; line-height: 1.46; }
  .album, .separator { color: #b2adb4; }
  .separator { margin: 0 .12rem; }
  .byline a:hover { text-decoration: underline; }
  .seek { margin-top: 13px; }
  .time, .volume { font-size: .7rem; font-variant-numeric: tabular-nums; color: hsl(var(--muted-foreground)); }
  .time { display: flex; justify-content: space-between; margin-top: -3px; }
  .transport { display: flex; justify-content: space-between; align-items: center; gap: 4px; margin: 8px auto 0; }
  .mode, .skip, .autoplay { position: relative; display: grid; place-items: center; flex: none; width: 42px; height: 42px; border-radius: 50%; }
  .mode, .autoplay { color: hsl(var(--muted-foreground)); }
  .mode:hover, .skip:hover, .autoplay:hover { background: #ffffff0c; color: white; }
  .mode[aria-pressed="true"], .autoplay[aria-pressed="true"] { color: var(--signal); background: #ffffff0b; }
  .repeat-one { position: absolute; font-size: .53rem; font-weight: 700; line-height: 1; }
  :global(.main-play) { width: 57px; height: 57px; padding: 0; border-radius: 50%; color: #1a171b; background: #f5f3f3; box-shadow: 0 2px 9px #0002; }
  :global(.main-play:hover) { color: #121114; background: white; }
  :global(.main-play svg) { width: 22px; height: 22px; }
  .deck-footer { margin-top: 16px; }
  .quality { display: flex; align-items: center; justify-content: center; flex-wrap: wrap; column-gap: .44rem; row-gap: .2rem; color: #b4afb7; font-size: .67rem; letter-spacing: .01em; font-variant-numeric: tabular-nums; }
  .quality .format { text-transform: uppercase; font-weight: 650; }
  .quality span + span::before { content: "·"; margin-right: .44rem; }
  .radio-status { display: flex; justify-content: center; align-items: center; gap: 8px; margin-top: 4px; color: hsl(var(--muted-foreground)); font-size: .76rem; }
  .side-panel { display: flex; flex-direction: column; align-self: center; width: 100%; min-width: 0; max-width: 440px; overflow: hidden; border: 1px solid #ffffff1b; border-radius: 20px; background: linear-gradient(155deg, #ffffff09, transparent 52%), rgb(24 24 29 / .88); box-shadow: 0 18px 60px -36px #0008; }
  .panel-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 64px; margin: 0 22px; border-bottom: 1px solid #ffffff16; }
  .desktop-tabs { display: flex; align-items: center; gap: 18px; }
  .desktop-tabs button { position: relative; display: flex; align-items: center; justify-content: center; gap: 7px; height: 64px; color: hsl(var(--muted-foreground)); font-size: .85rem; font-weight: 550; }
  .desktop-tabs button[aria-pressed="true"] { color: white; }
  .desktop-tabs button[aria-pressed="true"]::after { content: ""; position: absolute; inset: auto 0 -1px; height: 2px; border-radius: 2px; background: var(--signal); }
  .panel-content { overflow-y: auto; max-height: min(57dvh, 620px); padding: 19px 22px 11px; overscroll-behavior-y: contain; scrollbar-width: thin; }
  .current-row { display: flex; align-items: center; gap: 13px; border-radius: 10px; padding: 9px; background: #ffffff0c; }
  .current-cover { display: grid; flex: none; place-items: center; width: 55px; height: 55px; overflow: hidden; border-radius: 6px; background: hsl(var(--muted)); }
  .current-label { margin-bottom: 2px; color: var(--signal); font-size: .64rem; font-weight: 650; letter-spacing: .025em; }
  .current-title, .row-artist { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .current-title { font-size: .91rem; font-weight: 650; line-height: 1.35; }
  .row-artist { margin-top: 2px; font-size: .735rem; color: hsl(var(--muted-foreground)); }
  .level { display: inline-flex; height: 16px; align-items: center; gap: 2px; margin-inline: 4px 3px; color: var(--signal); }
  .level i { display: block; width: 2px; height: 12px; border-radius: 2px; background: currentColor; transform: scaleY(.4); }
  .level i:nth-child(2) { height: 20px; } .level i:nth-child(3) { height: 15px; }
  .level[data-playing="true"] i { animation: meter 730ms ease-in-out infinite alternate; }
  .level[data-playing="true"] i:nth-child(2) { animation-delay: -420ms; }
  .level[data-playing="true"] i:nth-child(3) { animation-delay: -250ms; }
  .level[data-playing="true"] i:nth-child(4) { animation-delay: -550ms; }
  @keyframes meter { to { transform: scaleY(1); } }
  .queue-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; margin: 23px 0 8px; }
  .queue-heading h3 { display: flex; align-items: center; gap: 7px; margin: 0; font-size: 1.05rem; font-weight: 640; letter-spacing: -.025em; }
  .queue-count { padding: 1px 6px; border: 1px solid #ffffff1f; border-radius: 6px; color: #c7c3c9; font-size: .64rem; font-weight: 500; letter-spacing: 0; }
  .queue-duration { display: flex; align-items: center; gap: 5px; color: hsl(var(--muted-foreground)); font-size: .72rem; font-variant-numeric: tabular-nums; }
  .lyrics { min-height: 225px; }
  .lyrics-text { white-space: pre-wrap; font-size: clamp(1.1rem, 2vw, 1.35rem); line-height: 1.55; font-weight: 600; }
  .empty { padding: 8px 0 17px; color: hsl(var(--muted-foreground)); font-size: .83rem; }
  .empty-lyrics { min-height: 210px; display: grid; place-content: center; justify-items: center; gap: 12px; }
  .volume { display: flex; align-items: center; gap: 12px; padding: 12px 23px; border-top: 1px solid #ffffff12; background: #00000010; --slider-fill: #b9b6be; }
  .volume-range { flex: 1; max-width: 145px; }
  .volume > span { margin-left: auto; }
  .mobile-tabs { display: none; }
  @media (max-width: 740px) {
    .canvas { padding: 0 clamp(18px, 6vw, 38px); min-height: 100dvh; }
    .player-header { height: 62px; gap: 0; }
    .signature { display: none; }
    :global(.player-caption) { padding-left: 0; border: 0; }
    :global(.collapse-player) { width: 36px; height: 36px; }
    .stage { display: flex; align-items: center; justify-content: center; width: 100%; min-height: auto; max-width: 480px; flex: 1; padding-block: clamp(16px, 3dvh, 32px); }
    .deck { width: 100%; max-width: min(100%, max(215px, calc(100dvh - 360px)), 365px); }
    .album-art { border-radius: 11px; }
    .track-heading { margin-top: 22px; }
    .track-heading h2 { font-size: clamp(1.8rem, 7vw, 2.25rem); }
    .transport { margin-top: 9px; }
    .mode, .skip { width: 44px; height: 44px; }
    .deck-footer { margin-top: 16px; }
    .ambience img { inset: -18% auto auto -40%; width: 180%; height: 112%; opacity: .32; }
    .stage[data-panel="track"] .side-panel, .stage:not([data-panel="track"]) .deck { display: none; }
    .stage:not([data-panel="track"]) { align-items: stretch; }
    .side-panel { max-width: none; align-self: stretch; border: 0; border-radius: 0; background: none; box-shadow: none; }
    .desktop-tabs { display: none; }
    .panel-head { min-height: 0; margin: 0; border: 0; justify-content: end; }
    .panel-content { max-height: none; padding: 0; flex: 1; overflow: visible; }
    .volume { margin-top: 14px; padding: 9px 2px; background: transparent; }
    .mobile-tabs { position: sticky; bottom: 0; display: flex; justify-content: center; gap: 6px; margin-inline: calc(clamp(18px, 6vw, 38px) * -1); padding: 8px 12px max(8px, env(safe-area-inset-bottom)); border-top: 1px solid #ffffff18; background: #19191df5; }
    .mobile-tabs button { display: grid; place-content: center; justify-items: center; gap: 2px; min-height: 44px; min-width: 70px; padding: 1px 10px; border-radius: 9px; color: hsl(var(--muted-foreground)); font-size: .66rem; font-weight: 550; }
    .mobile-tabs button[aria-pressed="true"] { background: #ffffff0d; color: var(--signal); }
  }
  @media (prefers-reduced-motion: reduce) { .level[data-playing="true"] i { animation: none; } }
</style>
