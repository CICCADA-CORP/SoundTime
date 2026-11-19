import { webPreferences } from "$lib/webradios/preferences.svelte";
import { live } from "$lib/webradios/live.svelte";
import { podcast, type PodcastEpisode, type PodcastShow } from "$lib/webradios/podcast.svelte";
import type { WebradioStation } from "$lib/webradios/types";
import { canPlay } from "$lib/catalog/availability";
import type { Track } from "$lib/types";
import { streamUrl, api, API_BASE, lastfmApi, prepareMediaSession } from "$lib/api";
import { showcaseAudio } from "$lib/showcase";
import { getQueueStore } from "./queue.svelte";
import { getRadioStore } from "./radio.svelte";

let currentTrack = $state<Track | null>(null);
let isPlaying = $state(false);
let failed = $state(false);
const volumeKey = "soundtime_volume";
function savedVolume() {
  try {
    const saved = localStorage.getItem(volumeKey);
    if (saved?.trim()) {
      const value = Number(saved);
      if (Number.isFinite(value) && value >= 0 && value <= 1) return value;
    }
  } catch { /* Browser storage can be unavailable. */ }
  return 0.8;
}
let volume = $state(savedVolume());
let progress = $state(0);
let duration = $state(0);
let shuffle = $state(false);
let repeat = $state<"none" | "one" | "all">("none");
let audio: HTMLAudioElement | null = null;
let lastPositionUpdate = 0;
let playbackRequest = 0;
let automaticallySelected = false;
let loadedTrackId: string | null = null;
let restorePosition: number | null = null;
let completionLogged = false;

/** The original favicon href so we can restore it when playback stops. */
let originalFaviconHref: string | null = null;

/** Resolve a relative media URL to an absolute one. */
function resolveMediaUrl(url: string): string {
  if (url.startsWith("http") || url.startsWith("/art/")) return url;
  const base = API_BASE.replace(/\/api$/, "");
  return `${base}${url}`;
}

/** Update the Media Session metadata (lock screen, notification center). */
function updateMediaSession(track: Track) {
  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) return;

  const artwork: MediaImage[] = [];
  if (track.cover_url) {
    const url = resolveMediaUrl(track.cover_url);
    artwork.push(
      { src: url, sizes: "96x96", type: "image/jpeg" },
      { src: url, sizes: "128x128", type: "image/jpeg" },
      { src: url, sizes: "192x192", type: "image/jpeg" },
      { src: url, sizes: "256x256", type: "image/jpeg" },
      { src: url, sizes: "384x384", type: "image/jpeg" },
      { src: url, sizes: "512x512", type: "image/jpeg" },
    );
  }

  navigator.mediaSession.metadata = new MediaMetadata({
    title: track.title,
    artist: track.artist_name ?? "",
    album: track.album_title ?? "",
    artwork,
  });
}

/** Swap the page favicon to the track's cover art, or restore the default. */
function updateFavicon(coverUrl: string | null | undefined) {
  if (typeof document === "undefined") return;

  let link = document.querySelector<HTMLLinkElement>('link[rel="icon"]');

  // Save the original favicon once
  if (originalFaviconHref === null) {
    originalFaviconHref = link?.href ?? "/soundtime-logo.svg";
  }

  if (!coverUrl) {
    // Restore original favicon
    if (link) link.href = originalFaviconHref;
    return;
  }

  const url = resolveMediaUrl(coverUrl);

  if (!link) {
    link = document.createElement("link");
    link.rel = "icon";
    document.head.appendChild(link);
  }

  link.href = url;
}

/** Update the Media Session playback position state. */
function updatePositionState() {
  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) return;
  if (!audio || !isFinite(audio.duration) || audio.duration <= 0) return;

  try {
    navigator.mediaSession.setPositionState({
      duration: audio.duration,
      playbackRate: audio.playbackRate,
      position: Math.min(audio.currentTime, audio.duration),
    });
  } catch {
    // Some browsers throw if position > duration during seek
  }
}

/** Register Media Session action handlers. */
function registerMediaSessionHandlers() {
  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) return;

  navigator.mediaSession.setActionHandler("play", () => {
    resume();
  });
  navigator.mediaSession.setActionHandler("pause", () => {
    pause();
  });
  navigator.mediaSession.setActionHandler("previoustrack", () => {
    const event = new CustomEvent("soundtime:previoustrack");
    window.dispatchEvent(event);
  });
  navigator.mediaSession.setActionHandler("nexttrack", () => {
    const event = new CustomEvent("soundtime:nexttrack");
    window.dispatchEvent(event);
  });
  navigator.mediaSession.setActionHandler("seekto", (details) => {
    if (details.seekTime != null) seek(details.seekTime);
  });
  navigator.mediaSession.setActionHandler("seekbackward", (details) => {
    if (audio) seek(Math.max(audio.currentTime - (details.seekOffset ?? 10), 0));
  });
  navigator.mediaSession.setActionHandler("seekforward", (details) => {
    if (audio) seek(Math.min(audio.currentTime + (details.seekOffset ?? 10), duration));
  });
}

function initAudio() {
  if (typeof window === "undefined") return;
  if (!audio) {
    audio = new Audio();
    audio.volume = volume;

    // Register Media Session handlers once
    registerMediaSessionHandlers();

    audio.addEventListener("timeupdate", () => {
      if (!audio || loadedTrackId !== currentTrack?.id) return;
      if (restorePosition === null && !failed) progress = audio.currentTime;
      // Update Media Session position every ~5s so iOS lock screen stays in sync
      const now = Date.now();
      if (now - lastPositionUpdate > 5000) {
        lastPositionUpdate = now;
        updatePositionState();
        if (isPlaying && !showcaseAudio(loadedTrackId)) prepareMediaSession()?.catch(() => {});
      }
    });

    audio.addEventListener("loadedmetadata", () => {
      if (!audio || loadedTrackId !== currentTrack?.id) return;
      duration = audio.duration;
      if (restorePosition !== null) {
        audio.currentTime = Math.min(restorePosition, duration);
        restorePosition = null;
      }
      updatePositionState();
    });

    audio.addEventListener("ended", () => {
      if (loadedTrackId !== currentTrack?.id || failed || completionLogged) return;
      // Track finished naturally — log as completed (not skipped).
      // Uses the full duration as the listen length because the track
      // played to the end; falls back to progress if duration is unknown.
      if (currentTrack) {
        logListen(currentTrack, duration > 0 ? duration : progress, true);
        completionLogged = true;
      }

      if (repeat === "one") {
        if (!audio) return;
        audio.currentTime = 0;
        resume();
        updatePositionState();
      } else {
        isPlaying = false;
        updateFavicon(null);
        if (typeof navigator !== "undefined" && "mediaSession" in navigator) {
          navigator.mediaSession.playbackState = "paused";
        }
        // Queue handles next track
        const event = new CustomEvent("soundtime:trackended");
        window.dispatchEvent(event);
      }
    });

    audio.addEventListener("pause", () => {
      if (live.station || podcast.episode) return;
      isPlaying = false;
      if (typeof navigator !== "undefined" && "mediaSession" in navigator) {
        navigator.mediaSession.playbackState = "paused";
      }
    });

    audio.addEventListener("play", () => {
      if (loadedTrackId !== currentTrack?.id) return;
      isPlaying = true;
      completionLogged = false;
      if (typeof navigator !== "undefined" && "mediaSession" in navigator) {
        navigator.mediaSession.playbackState = "playing";
      }
      updatePositionState();
    });
    audio.addEventListener("error", () => {
      if (loadedTrackId === currentTrack?.id && audio?.error?.code !== 1) failPlayback();
    });
  }
}

/**
 * Log a listen event with behavioral signals to `POST /history`.
 *
 * Sends the track's `duration_listened`, whether it `completed` naturally,
 * and whether it was `skipped` (i.e. the user moved to another track before
 * the current one finished). The `source_context` is read from the queue
 * store (set when `playQueue()` is called) so the backend knows *where*
 * the track was played from (album, playlist, radio, etc.).
 *
 * When the track was skipped (`completed === false`) and the user listened
 * for more than 0 seconds, `skip_position` is included so the recommendation
 * engine can distinguish early skips from near-completion skips.
 *
 * Failures are silently swallowed — listen logging is best-effort.
 */
function logListen(track: Track, durationListened: number, completed: boolean) {
  if (showcaseAudio(track.id)) return;
  let source: string | undefined;
  try {
    const q = getQueueStore();
    source = q.sourceContext ?? undefined;
  } catch {}

  const body: Record<string, unknown> = {
    track_id: track.id,
    duration_listened: durationListened,
    completed,
    skipped: !completed,
  };

  if (source) body.source_context = source;
  if (!completed && durationListened > 0) body.skip_position = durationListened;

  api.post("/history", body).catch(() => {});
}

function parkCatalogue() {
  live.stop();
  podcast.stop();
  if (audio && currentTrack && loadedTrackId === currentTrack.id && restorePosition === null && Number.isFinite(audio.currentTime)) {
    progress = audio.currentTime;
  }
  loadedTrackId = null;
  pause();
  if (audio) {
    audio.removeAttribute("src");
    audio.load();
  }
  getRadioStore().stopRadio();
  updateFavicon(null);
}

function restoreCatalogueSession() {
  if (!("mediaSession" in navigator)) return;
  navigator.mediaSession.playbackState = "paused";
  try {
    navigator.mediaSession.setPositionState?.();
    navigator.mediaSession.setActionHandler("stop", null);
  } catch { /* optional controls */ }
  registerMediaSessionHandlers();
  if (currentTrack) updateMediaSession(currentTrack);
  else navigator.mediaSession.metadata = null;
}

function playStation(station: WebradioStation) {
  if (typeof window === "undefined" || (webPreferences.status && !webPreferences.enabled)) return;
  parkCatalogue();
  if ("mediaSession" in navigator) {
    if (typeof MediaMetadata === "function") navigator.mediaSession.metadata = new MediaMetadata({ title: station.name });
    try { navigator.mediaSession.setPositionState?.(); } catch { /* live streams have no seekable length */ }
    for (const action of ["play", "previoustrack", "nexttrack", "seekto", "seekbackward", "seekforward"] as const) {
      try { navigator.mediaSession.setActionHandler(action, null); } catch { /* unsupported action */ }
    }
    navigator.mediaSession.setActionHandler("pause", live.stop);
    try { navigator.mediaSession.setActionHandler("stop", live.stop); } catch { /* optional system control */ }
  }
  void live.start(station, volume, restoreCatalogueSession);
}

function playPodcast(show: PodcastShow, episode: PodcastEpisode, country?: string) {
  if (typeof window === "undefined" || !webPreferences.enabled) return;
  parkCatalogue();
  if ("mediaSession" in navigator) {
    if (typeof MediaMetadata === "function") {
      navigator.mediaSession.metadata = new MediaMetadata({ title: episode.title, artist: show.author, album: show.title });
    }
    const session = navigator.mediaSession;
    try { session.setPositionState?.(); } catch { /* optional control */ }
    const handlers: Partial<Record<MediaSessionAction, MediaSessionActionHandler | null>> = {
      play: podcast.resume, pause: podcast.pause, stop: podcast.stop,
      previoustrack: null, nexttrack: null,
      seekto: ({ seekTime }) => { if (seekTime != null) podcast.seek(seekTime); },
      seekbackward: ({ seekOffset }) => podcast.seek(podcast.progress - (seekOffset ?? 10)),
      seekforward: ({ seekOffset }) => podcast.seek(podcast.progress + (seekOffset ?? 10)),
    };
    for (const [action, handler] of Object.entries(handlers)) {
      try { session.setActionHandler(action as MediaSessionAction, handler); } catch { /* optional OS control */ }
    }
  }
  void podcast.start(show, episode, volume, webPreferences.verifyPlayback, restoreCatalogueSession, country);
}

function play(track: Track, automatic = false) {
  if (!canPlay(track)) return;
  live.stop();
  podcast.stop();
  initAudio();
  if (!audio) return;

  // Log a skip for the previous track if it played for at least 5 seconds.
  // The 5-second threshold avoids logging spurious skips from rapid
  // next-track taps or accidental plays.
  if (currentTrack && progress > 5 && !completionLogged && !failed) {
    logListen(currentTrack, progress, false);
  }

  automaticallySelected = automatic;
  currentTrack = track;
  progress = 0;
  duration = track.duration_secs;
  isPlaying = false;
  failed = false;
  completionLogged = false;
  loadedTrackId = null;
  restorePosition = null;
  resume();

  // Update lock screen / notification with track info
  updateMediaSession(track);
  updateFavicon(track.cover_url);

  // Notify Last.fm of now-playing track
  if (!showcaseAudio(track.id)) lastfmApi.nowPlaying(track.id).catch(() => {});
}

function failPlayback() {
  if (failed) return;
  failed = true;
  pause();
  const id = currentTrack?.id;
  const request = playbackRequest;
  if (!id || showcaseAudio(id)) return;
  void api.get<Track>(`/tracks/${id}`).then(track => {
    if (request !== playbackRequest || currentTrack?.id !== id || !track || canPlay(track)) return;
    currentTrack = track;
    const advance = automaticallySelected || track.availability !== "temporarily_unavailable";
    getQueueStore().updateAvailability(track, advance);
  }).catch(() => {});
}

function pause() {
  playbackRequest++;
  audio?.pause();
  isPlaying = false;
  if (typeof navigator !== "undefined" && "mediaSession" in navigator) {
    navigator.mediaSession.playbackState = "paused";
  }
}

function resume() {
  live.stop();
  podcast.stop();
  if (!audio || !currentTrack) return;
  const track = currentTrack;
  if (!canPlay(track)) {
    if (track.availability !== "temporarily_unavailable") return;
    const request = ++playbackRequest;
    void api.get<Track>(`/tracks/${track.id}`).then(latest => {
      if (request !== playbackRequest || latest?.id !== track.id) return;
      currentTrack = latest;
      getQueueStore().updateAvailability(latest, false);
      if (canPlay(latest)) resume();
    }).catch(() => {});
    return;
  }
  const retry = failed;
  const localAudio = showcaseAudio(track.id);
  const request = ++playbackRequest;
  const start = () => {
    if (!audio || request !== playbackRequest) return;
    if (retry || loadedTrackId !== track.id) {
      restorePosition = progress > 0 ? progress : null;
      loadedTrackId = track.id;
      audio.src = localAudio ?? streamUrl(track.id);
    }
    failed = false;
    audio.play().catch((error: unknown) => {
      if (request !== playbackRequest || (error instanceof DOMException && error.name === "AbortError")) return;
      // A browser may require a new gesture after asynchronous cookie renewal.
      if (error instanceof DOMException && error.name === "NotAllowedError") return pause();
      failPlayback();
    });
    updatePositionState();
  };
  const waiting = localAudio ? undefined : prepareMediaSession(retry);
  if (!waiting) return start();
  audio.pause();
  void waiting.then(start, () => { if (request === playbackRequest) failPlayback(); });
}

function togglePlay() {
  if (isPlaying) pause();
  else resume();
}

function seek(time: number) {
  if (!audio || !currentTrack || !Number.isFinite(time)) return;
  const request = playbackRequest;
  const apply = () => {
    if (!audio || request !== playbackRequest) return;
    progress = time;
    if (failed || loadedTrackId !== currentTrack?.id) return;
    if (restorePosition !== null) restorePosition = time;
    audio.currentTime = time;
    updatePositionState();
  };
  const waiting = showcaseAudio(currentTrack.id) ? undefined : prepareMediaSession();
  if (!waiting) return apply();
  void waiting.then(apply, () => { if (request === playbackRequest) failPlayback(); });
}

function stopOnLogout() {
  live.stop();
  podcast.stop();
  pause();
  currentTrack = null;
  loadedTrackId = null;
  restorePosition = null;
  failed = false;
  completionLogged = false;
  progress = 0;
  duration = 0;
  if (audio) {
    audio.removeAttribute("src");
    audio.load();
  }
  getRadioStore().stopRadio();
  getQueueStore().clearQueue();
  updateFavicon(null);
  if ("mediaSession" in navigator) navigator.mediaSession.metadata = null;
}

if (typeof window !== "undefined") window.addEventListener("soundtime:logout", stopOnLogout);

function setVolume(vol: number) {
  if (!Number.isFinite(vol)) return;
  volume = Math.max(0, Math.min(1, vol));
  if (audio) audio.volume = volume;
  live.setVolume(volume);
  podcast.setVolume(volume);
  try { localStorage.setItem(volumeKey, String(volume)); }
  catch { /* The active player still applies the volume. */ }
}

function toggleShuffle() {
  shuffle = !shuffle;
}

function cycleRepeat() {
  if (repeat === "none") repeat = "one";
  else if (repeat === "one") repeat = "all";
  else repeat = "none";
}

/** Reset all player state. Intended for test isolation. */
function _resetForTests() {
  live.stop();
  podcast.stop();
  playbackRequest++;
  loadedTrackId = null;
  restorePosition = null;
  failed = false;
  currentTrack = null;
  isPlaying = false;
  volume = 0.8;
  progress = 0;
  duration = 0;
  shuffle = false;
  repeat = "none";
  audio = null;
  lastPositionUpdate = 0;
  originalFaviconHref = null;
}

export function getPlayerStore() {
  return {
    get currentTrack() { return live.station || podcast.episode ? null : currentTrack; },
    get isPlaying() { return isPlaying; },
    get failed() { return failed; },
    get volume() { return volume; },
    get progress() { return progress; },
    get duration() { return duration; },
    get shuffle() { return shuffle; },
    get repeat() { return repeat; },
    play,
    playStation,
    playPodcast,
    pause,
    resume,
    togglePlay,
    seek,
    setVolume,
    toggleShuffle,
    cycleRepeat,
    _resetForTests,
  };
}
