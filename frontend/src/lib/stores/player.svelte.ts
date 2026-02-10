import type { Track } from "$lib/types";
import { streamUrl, api, API_BASE } from "$lib/api";

let currentTrack = $state<Track | null>(null);
let isPlaying = $state(false);
let volume = $state(0.8);
let progress = $state(0);
let duration = $state(0);
let shuffle = $state(false);
let repeat = $state<"none" | "one" | "all">("none");
let audio: HTMLAudioElement | null = null;

/** Resolve a relative media URL to an absolute one. */
function resolveMediaUrl(url: string): string {
  if (url.startsWith("http")) return url;
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
    artist: track.artist_name ?? "Unknown Artist",
    album: track.album_title ?? "",
    artwork,
  });
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
    if (details.seekTime != null && audio) {
      audio.currentTime = details.seekTime;
      progress = details.seekTime;
      updatePositionState();
    }
  });
  navigator.mediaSession.setActionHandler("seekbackward", (details) => {
    if (audio) {
      const skipTime = details.seekOffset ?? 10;
      audio.currentTime = Math.max(audio.currentTime - skipTime, 0);
      progress = audio.currentTime;
      updatePositionState();
    }
  });
  navigator.mediaSession.setActionHandler("seekforward", (details) => {
    if (audio) {
      const skipTime = details.seekOffset ?? 10;
      audio.currentTime = Math.min(audio.currentTime + skipTime, audio.duration);
      progress = audio.currentTime;
      updatePositionState();
    }
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
      progress = audio!.currentTime;
    });

    audio.addEventListener("loadedmetadata", () => {
      duration = audio!.duration;
      updatePositionState();
    });

    audio.addEventListener("ended", () => {
      // Log listen when track ends naturally
      if (currentTrack) {
        api.post("/history", {
          track_id: currentTrack.id,
          duration_listened: duration > 0 ? duration : progress,
        }).catch(() => {});
      }

      if (repeat === "one") {
        audio!.currentTime = 0;
        audio!.play();
      } else {
        isPlaying = false;
        // Queue handles next track
        const event = new CustomEvent("soundtime:trackended");
        window.dispatchEvent(event);
      }
    });

    audio.addEventListener("pause", () => {
      isPlaying = false;
    });

    audio.addEventListener("play", () => {
      isPlaying = true;
    });
  }
}

function play(track: Track) {
  initAudio();
  if (!audio) return;

  // Log listen for the previous track if it played long enough
  if (currentTrack && progress > 5) {
    api.post("/history", {
      track_id: currentTrack.id,
      duration_listened: progress,
    }).catch(() => {});
  }

  currentTrack = track;
  audio.src = streamUrl(track.id);
  audio.play().catch(console.error);
  isPlaying = true;

  // Update lock screen / notification with track info
  updateMediaSession(track);
  if ("mediaSession" in navigator) {
    navigator.mediaSession.playbackState = "playing";
  }
}

function pause() {
  audio?.pause();
  isPlaying = false;
  if (typeof navigator !== "undefined" && "mediaSession" in navigator) {
    navigator.mediaSession.playbackState = "paused";
  }
}

function resume() {
  if (audio && currentTrack) {
    audio.play().catch(console.error);
    isPlaying = true;
    if (typeof navigator !== "undefined" && "mediaSession" in navigator) {
      navigator.mediaSession.playbackState = "playing";
    }
    updatePositionState();
  }
}

function togglePlay() {
  if (isPlaying) pause();
  else resume();
}

function seek(time: number) {
  if (audio) {
    audio.currentTime = time;
    progress = time;
  }
}

function setVolume(vol: number) {
  volume = Math.max(0, Math.min(1, vol));
  if (audio) audio.volume = volume;
}

function toggleShuffle() {
  shuffle = !shuffle;
}

function cycleRepeat() {
  if (repeat === "none") repeat = "one";
  else if (repeat === "one") repeat = "all";
  else repeat = "none";
}

export function getPlayerStore() {
  return {
    get currentTrack() { return currentTrack; },
    get isPlaying() { return isPlaying; },
    get volume() { return volume; },
    get progress() { return progress; },
    get duration() { return duration; },
    get shuffle() { return shuffle; },
    get repeat() { return repeat; },
    play,
    pause,
    resume,
    togglePlay,
    seek,
    setVolume,
    toggleShuffle,
    cycleRepeat,
  };
}
