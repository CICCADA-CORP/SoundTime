import type { Track } from "$lib/types";
import { streamUrl, api } from "$lib/api";

let currentTrack = $state<Track | null>(null);
let isPlaying = $state(false);
let volume = $state(0.8);
let progress = $state(0);
let duration = $state(0);
let shuffle = $state(false);
let repeat = $state<"none" | "one" | "all">("none");
let audio: HTMLAudioElement | null = null;

function initAudio() {
  if (typeof window === "undefined") return;
  if (!audio) {
    audio = new Audio();
    audio.volume = volume;

    audio.addEventListener("timeupdate", () => {
      progress = audio!.currentTime;
    });

    audio.addEventListener("loadedmetadata", () => {
      duration = audio!.duration;
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
}

function pause() {
  audio?.pause();
  isPlaying = false;
}

function resume() {
  if (audio && currentTrack) {
    audio.play().catch(console.error);
    isPlaying = true;
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
