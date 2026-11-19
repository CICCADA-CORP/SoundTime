<script lang="ts">
  import { CircleHelp, Ellipsis, Flag, ListEnd, ListPlus, ListStart, Pause, Play, Radio, Share2, Sparkles, ListMinus } from "lucide-svelte";
  import { tick } from "svelte";
  import type { PlaybackSource, Track } from "$lib/types";
  import { availabilityLabel, canPlay } from "$lib/catalog/availability";
  import { api } from "$lib/api";
  import { formatDuration } from "$lib/utils";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import { favoriteQuery } from "$lib/catalog/favorites.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import FavoriteButton from "./FavoriteButton.svelte";
  import TrackActions, { type TrackDialog } from "./catalog/TrackActions.svelte";

  let { tracks = [], showAlbum = true, showArtist = true, source = "collection", onremove }: {
    tracks?: Track[]; showAlbum?: boolean; showArtist?: boolean; source?: PlaybackSource; onremove?: (id: string) => void;
  } = $props();
  const player = getPlayerStore();
  const queue = getQueueStore();
  const auth = getAuthStore();
  const likes = favoriteQuery(() => auth.isAuthenticated && tracks.length ?
    api.get<string[]>(`/favorites/check?track_ids=${tracks.map(track => track.id).join(",")}`) : null);
  let likedIds = $derived(new Set(likes.data ?? []));
  let menu = $state<{ track: Track; x: number; y: number; origin: HTMLElement | null } | null>(null);
  let dialog = $state<{ track: Track; action: TrackDialog; origin: HTMLElement | null } | null>(null);
  let menuElement = $state<HTMLDivElement>();

  function play(index: number) {
    if (!canPlay(tracks[index])) return;
    if (player.currentTrack?.id === tracks[index].id) player.togglePlay();
    else queue.playQueue(tracks, index, source);
  }

  async function openMenu(event: MouseEvent, track: Track) {
    event.preventDefault();
    event.stopPropagation();
    const trigger = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
    const origin = trigger?.matches("button") ? trigger : trigger?.querySelector<HTMLButtonElement>("[aria-haspopup]") ?? null;
    const bounds = origin?.getBoundingClientRect();
    const keyboard = event.detail === 0 && event.type === "click";
    const x = keyboard ? bounds?.left ?? 8 : event.clientX;
    const y = keyboard ? bounds?.bottom ?? 8 : event.clientY;
    menu = { track, x, y, origin };
    await tick();
    if (!menu || !menuElement) return;
    menu.x = Math.max(8, Math.min(x, window.innerWidth - menuElement.offsetWidth - 8));
    menu.y = Math.max(8, Math.min(y, window.innerHeight - menuElement.offsetHeight - 8));
    menuElement.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }

  function closeMenu(restore = false) {
    if (restore) menu?.origin?.focus();
    menu = null;
  }

  function openDialog(action: TrackDialog) {
    if (menu) dialog = { action, track: menu.track, origin: menu.origin };
    closeMenu();
  }

  function menuKeys(event: KeyboardEvent) {
    if (!menu || !menuElement) return;
    if (event.key === "Escape") { closeMenu(true); return; }
    if (event.key === "Tab") { closeMenu(); return; }
    const buttons = [...menuElement.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    const position = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      let next = (position + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length;
      if (event.key === "Home") next = 0;
      if (event.key === "End") next = buttons.length - 1;
      buttons[next]?.focus();
    }
  }
</script>

<svelte:window onclick={() => closeMenu()} onkeydown={menuKeys} onresize={() => closeMenu()} />

<div class="track-table">
  {#if likes.failed}
    <div role="status" class="mb-2 flex flex-wrap gap-2 text-xs text-muted-foreground">{t("favorites.unavailable")} <button class="underline" onclick={likes.retry}>{t("radio.tryAgain")}</button></div>
  {/if}
  <div class="track-line heading text-xs text-muted-foreground">
    <span>#</span><span>{t("track.title")}</span>
    <span class="album">{showAlbum ? t("track.album") : showArtist ? t("track.artist") : ""}</span>
    <span class="plays">{t("track.plays")}</span><span class="quality">{t("track.quality")}</span>
    <span class="text-right">{t("track.duration")}</span><span class="like"></span><span></span>
  </div>
  {#each tracks as track, index (track.id + ":" + (track.position ?? index))}
    {@const active = player.currentTrack?.id === track.id}
    {@const unavailable = !canPlay(track)}
    <div role="group" aria-label={track.title} class:unavailable data-availability={track.availability} class="track-line group relative border-b border-border/70 text-sm {active ? 'bg-primary/5' : 'hover:bg-secondary/65'}"
      oncontextmenu={(e) => openMenu(e, track)}>
      <button type="button" disabled={unavailable} aria-label={t("track.play") + " " + track.title} class="play-cell {active ? 'text-primary' : 'text-muted-foreground'}"
        onclick={() => play(index)}>
        <span class="track-number">{source === "playlist" ? (track.position ?? index) + 1 : track.track_number ?? index + 1}</span>
        <span class="play-icon">{#if active && player.isPlaying}<Pause size={14} fill="currentColor" />{:else}<Play size={14} fill="currentColor" />{/if}</span>
      </button>
      <div class="min-w-0">
        <a class="block truncate font-medium hover:underline {active ? 'text-primary' : ''}" href="/tracks/{track.id}">{track.title}</a>
        {#if unavailable}<span class="block text-xs text-muted-foreground">{availabilityLabel(track)}</span>{/if}
        {#if showArtist}<span class="block truncate text-xs text-muted-foreground">{track.artist_name ?? ""}</span>{/if}
      </div>
      <div class="album truncate text-muted-foreground">
        {#if showAlbum && track.album_id}<a href="/albums/{track.album_id}" class="hover:underline">{track.album_title ?? ""}</a>
        {:else if showAlbum}{track.album_title ?? ""}{:else if showArtist}{track.artist_name ?? ""}{/if}
      </div>
      <span class="plays text-xs tabular-nums text-muted-foreground">{track.play_count ?? 0}</span>
      <span class="quality text-xs text-muted-foreground">
        {#if track.best_bitrate || track.bitrate}
          <span>{track.best_bitrate ?? track.bitrate}k</span> <span class="uppercase">{track.format}</span>
          {#if track.best_source && track.best_source !== "local"}<span class="text-primary" title={track.best_source}>Fed</span>{/if}
        {:else}—{/if}
      </span>
      <span class="text-right text-xs tabular-nums text-muted-foreground">{formatDuration(track.duration_secs)}</span>
      <span class="like">
        {#if auth.user}<FavoriteButton trackId={track.id} disabled={likes.loading || likes.failed} liked={likedIds.has(track.id)} size={15} />{/if}
      </span>
      <button class="grid size-8 place-items-center rounded-full text-muted-foreground hover:bg-secondary hover:text-primary"
        aria-label={t("track.options") + " " + track.title} aria-haspopup="menu" aria-expanded={menu?.track.id === track.id} onclick={e => openMenu(e, track)}><Ellipsis size={17} /></button>
    </div>
  {/each}
</div>

{#if menu}
  {@const track = menu.track}
  <div role="menu" aria-label={t("track.options")} class="track-menu fixed z-[200] max-h-[calc(100dvh-1rem)] overflow-y-auto rounded-xl border border-border bg-popover p-1 shadow-xl"
    style:left={menu.x + "px"} style:top={menu.y + "px"} bind:this={menuElement}>
    <button role="menuitem" disabled={!canPlay(track)} onclick={() => { queue.addNext(track); closeMenu(true); }}><ListStart size={16} />{t("track.playNext")}</button>
    <button role="menuitem" disabled={!canPlay(track)} onclick={() => { queue.addToQueue(track); closeMenu(true); }}><ListEnd size={16} />{t("track.addToQueue")}</button>
    <button role="menuitem" disabled={!canPlay(track)} onclick={() => { getRadioStore().startRadio("track", { seedId: track.id, label: track.title }); closeMenu(true); }}><Radio size={16} />{t("radio.startFromTrack")}</button>
    <button role="menuitem" disabled={!canPlay(track)} onclick={() => { getRadioStore().startRadio("similar", { seedId: track.id, label: track.title }); closeMenu(true); }}><Sparkles size={16} />{t("radio.playSimilar")}</button>
    {#if auth.isAuthenticated}<button role="menuitem" onclick={() => openDialog("playlist")}><ListPlus size={16} />{t("track.addToPlaylist")}</button>{/if}
    {#if onremove}<button role="menuitem" onclick={() => { closeMenu(true); onremove?.(track.id); }}><ListMinus size={16} />{t("playlists.removeTrack")}</button>{/if}
    <button role="menuitem" onclick={() => openDialog("credits")}><CircleHelp size={16} />{t("track.viewCredits")}</button>
    <button role="menuitem" onclick={() => openDialog("share")}><Share2 size={16} />{t("track.share")}</button>
    {#if auth.isAuthenticated}<button role="menuitem" class="text-destructive" onclick={() => openDialog("report")}><Flag size={16} />{t("track.report")}</button>{/if}
  </div>
{/if}
{#if dialog}
  <TrackActions track={dialog.track} action={dialog.action} close={() => { dialog?.origin?.focus(); dialog = null; }} />
{/if}

<style>
  .unavailable { color: hsl(var(--muted-foreground)); }
  .unavailable .play-cell { opacity: 0.45; }
  .track-menu button:disabled { opacity: 0.5; }
  .track-table { container-type: inline-size; }
  .track-line { display: grid; align-items: center; gap: .55rem; padding: .45rem .4rem; grid-template-columns: 1.75rem minmax(0,1fr) 2.7rem 2rem 2rem; }
  .heading { border-bottom: 1px solid hsl(var(--border)); padding-block: .5rem; }
  .heading > :first-child { text-align: center; }
  .album, .plays, .quality { display: none; }
  .like { display: grid; place-items: center; }
  .play-cell { display: grid; place-items: center; min-height: 2rem; }
  .play-cell::after { content: ""; position: absolute; inset: 0; }
  .track-line a, .track-line button:not(.play-cell), .like { position: relative; }
  .play-icon { display: none; }
  .track-line:hover .track-number, .play-cell:focus-visible .track-number { display: none; }
  .track-line:hover .play-icon, .play-cell:focus-visible .play-icon { display: block; }
  .track-menu button { display: flex; align-items: center; gap: .65rem; width: 100%; border-radius: .4rem; padding: .5rem .7rem; font-size: .875rem; text-align: left; }
  .track-menu button:hover, .track-menu button:focus-visible { background: hsl(var(--secondary)); }
  @container (min-width: 420px) {
    .track-line { grid-template-columns: 1.75rem minmax(0,1fr) 3rem 1.5rem 2rem; }
  }
  @container (min-width: 590px) {
    .track-line { grid-template-columns: 1.75rem minmax(0,1.5fr) minmax(0,1fr) 3rem 1.5rem 2rem; }
    .album { display: block; }
  }
  @container (min-width: 760px) {
    .track-line { grid-template-columns: 1.75rem minmax(0,1.6fr) minmax(0,1fr) 3rem 6rem 3rem 1.5rem 2rem; }
    .plays, .quality { display: block; }
  }
</style>
