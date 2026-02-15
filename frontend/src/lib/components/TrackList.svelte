<script lang="ts">
  import type { Track, Playlist, TrackCredits } from "$lib/types";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { api } from "$lib/api";
  import { formatDuration } from "$lib/utils";
  import { onMount } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import FavoriteButton from "./FavoriteButton.svelte";

  let { tracks = [], showAlbum = true, showArtist = true }: {
    tracks?: Track[];
    showAlbum?: boolean;
    showArtist?: boolean;
  } = $props();

  const player = getPlayerStore();
  const queue = getQueueStore();
  const auth = getAuthStore();

  let likedMap = $state<Record<string, boolean>>({});
  let reportingTrackId = $state<string | null>(null);
  let reportReason = $state("");
  let reportError = $state("");
  let reportSuccess = $state("");

  // Context menu state
  let menuTrackId = $state<string | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);

  // Add to playlist state
  let showPlaylistPicker = $state(false);
  let userPlaylists = $state<Playlist[]>([]);
  let playlistPickerTrackId = $state<string | null>(null);
  let playlistLoading = $state(false);

  // Credits modal state
  let creditsTrackId = $state<string | null>(null);
  let creditsData = $state<TrackCredits | null>(null);
  let creditsLoading = $state(false);

  // Share modal
  let shareTrackId = $state<string | null>(null);
  let shareCopied = $state(false);

  onMount(async () => {
    // Initialize likedMap for all tracks (prevents undefined → props_invalid_value)
    const map: Record<string, boolean> = {};
    for (const t of tracks) map[t.id] = false;
    likedMap = map;

    if (auth.isAuthenticated && tracks.length > 0) {
      const ids = tracks.map((t) => t.id).join(",");
      try {
        const likedIds = await api.get<string[]>(`/favorites/check?track_ids=${ids}`);
        const newMap: Record<string, boolean> = {};
        for (const t of tracks) newMap[t.id] = likedIds.includes(t.id);
        likedMap = newMap;
      } catch {
        // keep defaults
      }
    }
  });

  function playTrack(index: number) {
    queue.playQueue(tracks, index);
  }

  // Context menu handlers
  function openContextMenu(e: MouseEvent, trackId: string) {
    e.preventDefault();
    e.stopPropagation();
    menuTrackId = trackId;
    menuX = e.clientX;
    menuY = e.clientY;
    const maxX = window.innerWidth - 220;
    const maxY = window.innerHeight - 280;
    if (menuX > maxX) menuX = maxX;
    if (menuY > maxY) menuY = maxY;
  }

  function closeMenu() {
    menuTrackId = null;
  }

  function handlePlayNext(trackId: string) {
    const track = tracks.find(t => t.id === trackId);
    if (track) queue.addNext(track);
    closeMenu();
  }

  function handleAddToQueue(trackId: string) {
    const track = tracks.find(t => t.id === trackId);
    if (track) queue.addToQueue(track);
    closeMenu();
  }

  async function openPlaylistPicker(trackId: string) {
    closeMenu();
    playlistPickerTrackId = trackId;
    showPlaylistPicker = true;
    playlistLoading = true;
    try {
      const res = await api.get<{ data: Playlist[] }>("/playlists");
      userPlaylists = (res.data ?? []).filter(p => p.user_id === auth.user?.id || p.owner_id === auth.user?.id);
    } catch {
      userPlaylists = [];
    } finally {
      playlistLoading = false;
    }
  }

  async function addToPlaylist(playlistId: string) {
    if (!playlistPickerTrackId) return;
    try {
      await api.post(`/playlists/${playlistId}/tracks`, { track_id: playlistPickerTrackId });
    } catch { /* ignore duplicates */ }
    showPlaylistPicker = false;
    playlistPickerTrackId = null;
  }

  async function openCredits(trackId: string) {
    closeMenu();
    creditsTrackId = trackId;
    creditsLoading = true;
    creditsData = null;
    try {
      creditsData = await api.get(`/tracks/${trackId}/credits`);
    } catch {
      creditsData = null;
    } finally {
      creditsLoading = false;
    }
  }

  function openShare(trackId: string) {
    closeMenu();
    shareTrackId = trackId;
    shareCopied = false;
  }

  function copyShareLink() {
    if (!shareTrackId) return;
    const url = `${window.location.origin}/tracks/${shareTrackId}`;
    navigator.clipboard.writeText(url);
    shareCopied = true;
    setTimeout(() => { shareCopied = false; }, 2000);
  }

  function openReport(trackId: string) {
    closeMenu();
    reportingTrackId = trackId;
    reportReason = "";
    reportError = "";
    reportSuccess = "";
  }

  async function submitReport() {
    if (!reportingTrackId || !reportReason.trim()) return;
    reportError = "";
    try {
      await api.post(`/tracks/${reportingTrackId}/report`, { reason: reportReason.trim() });
      reportSuccess = t('track.reportSent');
      setTimeout(() => { reportingTrackId = null; reportSuccess = ""; }, 2000);
    } catch (e: unknown) {
      reportError = (e instanceof Error ? e.message : String(e)) ?? t('track.reportError');
    }
  }

  function handleWindowClick() {
    if (menuTrackId) closeMenu();
  }
</script>

<svelte:window onclick={handleWindowClick} />

<div class="w-full overflow-x-hidden">
  <!-- Header (desktop only) -->
  <div class="hidden md:grid grid-cols-[auto_1fr_1fr_auto_auto_auto_auto_auto] gap-4 px-4 py-2 text-xs text-[hsl(var(--muted-foreground))] uppercase tracking-wider border-b border-[hsl(var(--border))]">
    <span class="w-8 text-center">#</span>
    <span>{t('track.title')}</span>
    {#if showAlbum}
      <span>{t('track.album')}</span>
    {:else if showArtist}
      <span>{t('track.artist')}</span>
    {:else}
      <span></span>
    {/if}
    <span class="w-16 text-right">{t('track.plays')}</span>
    <span class="w-20 text-right">{t('track.quality')}</span>
    <span class="w-16 text-right">{t('track.duration')}</span>
    <span class="w-8"></span>
    <span class="w-8"></span>
  </div>

  <!-- Tracks -->
  {#each tracks as track, i}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="flex md:grid md:grid-cols-[auto_1fr_1fr_auto_auto_auto_auto_auto] gap-2 md:gap-4 px-3 md:px-4 py-2.5 md:py-2 w-full text-left hover:bg-[hsl(var(--secondary))] rounded transition group cursor-pointer items-center"
      class:bg-[hsl(var(--secondary))]={player.currentTrack?.id === track.id}
      onclick={() => playTrack(i)}
      oncontextmenu={(e) => openContextMenu(e, track.id)}
      onkeydown={(e) => e.key === 'Enter' && playTrack(i)}
      role="button"
      tabindex="0"
    >
      <!-- Track number (desktop) -->
      <span class="hidden md:block w-8 text-center text-sm text-[hsl(var(--muted-foreground))] group-hover:hidden">
        {track.track_number ?? i + 1}
      </span>
      <span class="hidden md:!hidden md:group-hover:!block w-8 text-center text-sm text-white">
        {#if player.currentTrack?.id === track.id && player.isPlaying}
          <svg class="w-4 h-4 mx-auto" fill="currentColor" viewBox="0 0 24 24"><path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
        {:else}
          <svg class="w-4 h-4 mx-auto" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
        {/if}
      </span>

      <!-- Mobile: track number -->
      <span class="md:hidden w-6 text-center text-xs text-[hsl(var(--muted-foreground))] flex-shrink-0">
        {#if player.currentTrack?.id === track.id && player.isPlaying}
          <svg class="w-3.5 h-3.5 mx-auto text-[hsl(var(--primary))]" fill="currentColor" viewBox="0 0 24 24"><path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
        {:else}
          {track.track_number ?? i + 1}
        {/if}
      </span>

      <!-- Title + artist -->
      <div class="min-w-0 flex-1">
        <p class="text-sm truncate" class:text-[hsl(var(--primary))]={player.currentTrack?.id === track.id}>
          <a href="/tracks/{track.id}" class="hover:underline" onclick={(e) => e.stopPropagation()}>{track.title}</a>
        </p>
        {#if showArtist}
          <p class="text-xs text-[hsl(var(--muted-foreground))] truncate">{track.artist_name ?? ""}</p>
        {/if}
      </div>

      <!-- Album/Artist column (desktop) -->
      <span class="hidden md:block text-sm text-[hsl(var(--muted-foreground))] truncate">
        {#if showAlbum}
          {track.album_title ?? ""}
        {:else if showArtist}
          {track.artist_name ?? ""}
        {/if}
      </span>

      <!-- Plays (desktop) -->
      <span class="hidden md:block w-16 text-right text-xs text-[hsl(var(--muted-foreground))] tabular-nums">
        {track.play_count ?? 0}
      </span>

      <!-- Quality (desktop) -->
      <span class="hidden md:flex w-20 text-right text-xs text-[hsl(var(--muted-foreground))] items-center justify-end gap-1">
        {#if track.best_bitrate || track.bitrate}
          <span class="font-mono">{track.best_bitrate ?? track.bitrate ?? 0}k</span>
          <span class="uppercase opacity-70">{track.format}</span>
          {#if track.best_source && track.best_source !== "local"}
            <span class="text-[9px] bg-blue-500/20 text-blue-400 px-1 rounded" title="Source: {track.best_source}">Fed</span>
          {/if}
        {:else}
          <span class="opacity-50">—</span>
        {/if}
      </span>

      <!-- Duration -->
      <span class="text-xs md:text-sm text-[hsl(var(--muted-foreground))] md:w-16 md:text-right flex-shrink-0">
        {formatDuration(track.duration_secs)}
      </span>

      <!-- Like button (desktop) -->
      <span class="hidden md:flex w-8 items-center justify-center gap-1">
        {#if auth.user}
          <FavoriteButton trackId={track.id} liked={likedMap[track.id] ?? false} size={14} />
        {/if}
      </span>

      <!-- More options button -->
      <span class="flex-shrink-0 flex items-center justify-center w-6 md:w-8">
        <button
          class="md:opacity-0 md:group-hover:opacity-60 hover:!opacity-100 text-[hsl(var(--muted-foreground))] hover:text-white transition p-1"
          title={t('track.options')}
          onclick={(e) => { e.stopPropagation(); openContextMenu(e, track.id); }}
        >
          <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 24 24"><circle cx="12" cy="5" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="12" cy="19" r="1.5"/></svg>
        </button>
      </span>
    </div>
  {/each}
</div>

<!-- Context Menu -->
{#if menuTrackId}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed z-[200] bg-[hsl(var(--card))] border border-[hsl(var(--border))] rounded-lg shadow-2xl py-1 min-w-[200px]"
    style="left: {menuX}px; top: {menuY}px;"
    onclick={(e) => e.stopPropagation()}
  >
    <button class="w-full text-left px-4 py-2 text-sm hover:bg-[hsl(var(--secondary))] transition flex items-center gap-3" onclick={() => handlePlayNext(menuTrackId!)}>
      <svg class="w-4 h-4 text-[hsl(var(--muted-foreground))]" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M13 5l7 7-7 7M5 5l7 7-7 7"/></svg>
      {t('track.playNext')}
    </button>
    <button class="w-full text-left px-4 py-2 text-sm hover:bg-[hsl(var(--secondary))] transition flex items-center gap-3" onclick={() => handleAddToQueue(menuTrackId!)}>
      <svg class="w-4 h-4 text-[hsl(var(--muted-foreground))]" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6v12m-6-6h12"/></svg>
      {t('track.addToQueue')}
    </button>
    {#if auth.isAuthenticated}
      <div class="border-t border-[hsl(var(--border))] my-1"></div>
      <button class="w-full text-left px-4 py-2 text-sm hover:bg-[hsl(var(--secondary))] transition flex items-center gap-3" onclick={() => openPlaylistPicker(menuTrackId!)}>
        <svg class="w-4 h-4 text-[hsl(var(--muted-foreground))]" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"/></svg>
        {t('track.addToPlaylist')}
      </button>
    {/if}
    <div class="border-t border-[hsl(var(--border))] my-1"></div>
    <button class="w-full text-left px-4 py-2 text-sm hover:bg-[hsl(var(--secondary))] transition flex items-center gap-3" onclick={() => openCredits(menuTrackId!)}>
      <svg class="w-4 h-4 text-[hsl(var(--muted-foreground))]" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg>
      {t('track.viewCredits')}
    </button>
    <button class="w-full text-left px-4 py-2 text-sm hover:bg-[hsl(var(--secondary))] transition flex items-center gap-3" onclick={() => openShare(menuTrackId!)}>
      <svg class="w-4 h-4 text-[hsl(var(--muted-foreground))]" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M8.684 13.342C8.886 12.938 9 12.482 9 12c0-.482-.114-.938-.316-1.342m0 2.684a3 3 0 110-2.684m0 2.684l6.632 3.316m-6.632-6l6.632-3.316m0 0a3 3 0 105.367-2.684 3 3 0 00-5.367 2.684zm0 9.316a3 3 0 105.368 2.684 3 3 0 00-5.368-2.684z"/></svg>
      {t('track.share')}
    </button>
    {#if auth.isAuthenticated}
      <div class="border-t border-[hsl(var(--border))] my-1"></div>
      <button class="w-full text-left px-4 py-2 text-sm hover:bg-[hsl(var(--secondary))] text-red-400 transition flex items-center gap-3" onclick={() => openReport(menuTrackId!)}>
        <svg class="w-4 h-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3 3v18m0-18l9 6-9 6"/></svg>
        {t('track.report')}
      </button>
    {/if}
  </div>
{/if}

<!-- Playlist Picker Modal -->
{#if showPlaylistPicker}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 bg-black/60 z-[201] flex items-center justify-center p-4" onclick={() => showPlaylistPicker = false} onkeydown={(e) => e.key === 'Escape' && (showPlaylistPicker = false)} role="dialog" tabindex="-1">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="bg-[hsl(var(--card))] rounded-xl p-6 w-full max-w-sm shadow-2xl" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <h3 class="text-lg font-semibold mb-4">{t('track.addToPlaylist')}</h3>
      {#if playlistLoading}
        <div class="flex justify-center py-6">
          <div class="w-6 h-6 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
        </div>
      {:else if userPlaylists.length === 0}
        <p class="text-sm text-[hsl(var(--muted-foreground))] py-4">{t('track.noPlaylists')}</p>
      {:else}
        <div class="space-y-1 max-h-60 overflow-y-auto">
          {#each userPlaylists as pl}
            <button
              class="w-full text-left px-3 py-2.5 rounded-lg hover:bg-[hsl(var(--secondary))] transition text-sm flex items-center gap-3"
              onclick={() => addToPlaylist(pl.id)}
            >
              <span class="text-lg">🎶</span>
              <span class="truncate">{pl.name}</span>
            </button>
          {/each}
        </div>
      {/if}
      <div class="mt-4 flex justify-end">
        <button class="px-4 py-2 text-sm rounded-lg bg-[hsl(var(--secondary))] hover:opacity-80 transition" onclick={() => showPlaylistPicker = false}>{t('common.cancel')}</button>
      </div>
    </div>
  </div>
{/if}

<!-- Credits Modal -->
{#if creditsTrackId}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 bg-black/60 z-[201] flex items-center justify-center p-4" onclick={() => creditsTrackId = null} onkeydown={(e) => e.key === 'Escape' && (creditsTrackId = null)} role="dialog" tabindex="-1">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="bg-[hsl(var(--card))] rounded-xl p-6 w-full max-w-md shadow-2xl" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <h3 class="text-lg font-semibold mb-4">{t('track.viewCredits')}</h3>
      {#if creditsLoading}
        <div class="flex justify-center py-6">
          <div class="w-6 h-6 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
        </div>
      {:else if creditsData}
        <div class="space-y-3 text-sm">
          {#if creditsData.artist}<div><span class="text-[hsl(var(--muted-foreground))]">{t('track.artist')}:</span> <span class="font-medium">{creditsData.artist}</span></div>{/if}
          {#if creditsData.album}<div><span class="text-[hsl(var(--muted-foreground))]">{t('track.album')}:</span> <span class="font-medium">{creditsData.album}</span></div>{/if}
          {#if creditsData.genre}<div><span class="text-[hsl(var(--muted-foreground))]">{t('track.genre')}:</span> <span class="font-medium">{creditsData.genre}</span></div>{/if}
          {#if creditsData.year}<div><span class="text-[hsl(var(--muted-foreground))]">{t('track.year')}:</span> <span class="font-medium">{creditsData.year}</span></div>{/if}
          {#if creditsData.format}<div><span class="text-[hsl(var(--muted-foreground))]">{t('track.format')}:</span> <span class="font-medium uppercase">{creditsData.format}</span></div>{/if}
          {#if creditsData.bitrate}<div><span class="text-[hsl(var(--muted-foreground))]">{t('track.bitrate')}:</span> <span class="font-medium">{creditsData.bitrate} kbps</span></div>{/if}
          {#if creditsData.sample_rate}<div><span class="text-[hsl(var(--muted-foreground))]">{t('track.sampleRate')}:</span> <span class="font-medium">{creditsData.sample_rate} Hz</span></div>{/if}
          {#if creditsData.musicbrainz_id}<div><span class="text-[hsl(var(--muted-foreground))]">MusicBrainz:</span> <a href="https://musicbrainz.org/recording/{creditsData.musicbrainz_id}" target="_blank" class="text-[hsl(var(--primary))] hover:underline font-mono text-xs">{creditsData.musicbrainz_id}</a></div>{/if}
          {#if creditsData.uploaded_by_username}<div><span class="text-[hsl(var(--muted-foreground))]">{t('track.uploadedBy')}:</span> <span class="font-medium">{creditsData.uploaded_by_username}</span></div>{/if}
        </div>
      {:else}
        <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.noCredits')}</p>
      {/if}
      <div class="mt-4 flex justify-end">
        <button class="px-4 py-2 text-sm rounded-lg bg-[hsl(var(--secondary))] hover:opacity-80 transition" onclick={() => creditsTrackId = null}>{t('common.close')}</button>
      </div>
    </div>
  </div>
{/if}

<!-- Share Modal -->
{#if shareTrackId}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 bg-black/60 z-[201] flex items-center justify-center p-4" onclick={() => shareTrackId = null} onkeydown={(e) => e.key === 'Escape' && (shareTrackId = null)} role="dialog" tabindex="-1">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="bg-[hsl(var(--card))] rounded-xl p-6 w-full max-w-sm shadow-2xl" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <h3 class="text-lg font-semibold mb-4">{t('track.share')}</h3>
      <div class="flex gap-2">
        <input type="text" readonly value="{typeof window !== 'undefined' ? window.location.origin : ''}/tracks/{shareTrackId}" class="flex-1 px-3 py-2 rounded-lg bg-[hsl(var(--secondary))] text-sm border border-[hsl(var(--border))] outline-none" />
        <button class="px-4 py-2 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium hover:opacity-90 transition" onclick={copyShareLink}>
          {shareCopied ? t('common.copied') : t('common.copy')}
        </button>
      </div>
      <div class="mt-4 flex justify-end">
        <button class="px-4 py-2 text-sm rounded-lg bg-[hsl(var(--secondary))] hover:opacity-80 transition" onclick={() => shareTrackId = null}>{t('common.close')}</button>
      </div>
    </div>
  </div>
{/if}

<!-- Report Modal -->
{#if reportingTrackId}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 bg-black/60 z-[201] flex items-center justify-center p-4" onclick={() => reportingTrackId = null} onkeydown={(e) => e.key === 'Escape' && (reportingTrackId = null)} role="dialog" tabindex="-1">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="bg-[hsl(var(--card))] rounded-xl p-6 w-full max-w-md shadow-2xl" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <h3 class="text-lg font-semibold mb-4">{t('track.reportTitle')}</h3>
      {#if reportSuccess}
        <p class="text-green-400 text-sm">{reportSuccess}</p>
      {:else}
        {#if reportError}
          <p class="text-red-400 text-sm mb-3">{reportError}</p>
        {/if}
        <textarea bind:value={reportReason} placeholder={t('track.reportPlaceholder')} class="w-full px-3 py-2 rounded-lg bg-[hsl(var(--secondary))] text-sm resize-none h-28 border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" maxlength="500"></textarea>
        <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1 text-right">{reportReason.length}/500</p>
        <div class="flex gap-3 mt-4 justify-end">
          <button class="px-4 py-2 text-sm rounded-lg bg-[hsl(var(--secondary))] hover:opacity-80 transition" onclick={() => reportingTrackId = null}>{t('common.cancel')}</button>
          <button class="px-4 py-2 text-sm rounded-lg bg-red-500/20 text-red-400 hover:bg-red-500/30 font-medium transition disabled:opacity-50" disabled={!reportReason.trim()} onclick={submitReport}>{t('track.sendReport')}</button>
        </div>
      {/if}
    </div>
  </div>
{/if}