import type { Track } from "$lib/types";
import { getPlayerStore } from "./player.svelte";
import { getRadioStore } from "./radio.svelte";

let queue = $state<Track[]>([]);
let currentIndex = $state(-1);
let originalQueue = $state<Track[]>([]);
/** Where the current queue originated from (e.g. "album", "playlist", "radio").
 *  Attached to every listen event so the recommendation engine can weigh
 *  intentional plays differently from auto-queued ones. Reset on `clearQueue()`. */
let sourceContext = $state<import("$lib/types").PlaybackSource | null>(null);

const AUTOPLAY_STORAGE_KEY = "soundtime_autoplay";

/** Read autoplay preference from localStorage (defaults to false). */
function loadAutoplay(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return localStorage.getItem(AUTOPLAY_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

let autoplay = $state(loadAutoplay());

/**
 * Replace the queue with `tracks`, start playback at `startIndex`, and
 * optionally record the `source` context (e.g. "album", "playlist") so
 * that subsequent listen events include where the playback originated.
 * Stops radio mode if it was active.
 */
function playQueue(tracks: Track[], startIndex = 0, source?: import("$lib/types").PlaybackSource) {
  // Stop radio if active — user is manually choosing what to play
  try {
    const radio = getRadioStore();
    if (radio.active) radio.stopRadio();
  } catch {}

  sourceContext = source ?? null;
  queue = [...tracks];
  originalQueue = [...tracks];
  currentIndex = startIndex;
  const player = getPlayerStore();
  if (queue[currentIndex]) {
    player.play(queue[currentIndex]);
  }
}

function addToQueue(track: Track) {
  queue = [...queue, track];
  originalQueue = [...originalQueue, track];
}

function addNext(track: Track) {
  const idx = currentIndex + 1;
  queue = [...queue.slice(0, idx), track, ...queue.slice(idx)];
}

function removeFromQueue(index: number) {
  queue = queue.filter((_, i) => i !== index);
  if (index < currentIndex) currentIndex--;
}

function moveInQueue(fromIndex: number, toIndex: number) {
  if (fromIndex < 0 || fromIndex >= queue.length) return;
  if (toIndex < 0 || toIndex >= queue.length) return;
  if (fromIndex === toIndex) return;
  const updated = [...queue];
  const [track] = updated.splice(fromIndex, 1);
  updated.splice(toIndex, 0, track);
  queue = updated;
  // Adjust currentIndex if it was affected by the move
  if (currentIndex === fromIndex) {
    currentIndex = toIndex;
  } else if (fromIndex < currentIndex && toIndex >= currentIndex) {
    currentIndex--;
  } else if (fromIndex > currentIndex && toIndex <= currentIndex) {
    currentIndex++;
  }
}

function clearQueue() {
  queue = [];
  originalQueue = [];
  currentIndex = -1;
  sourceContext = null;
}

/** Toggle autoplay preference and persist to localStorage. */
function toggleAutoplay() {
  autoplay = !autoplay;
  if (typeof window !== "undefined") {
    try {
      localStorage.setItem(AUTOPLAY_STORAGE_KEY, String(autoplay));
    } catch {
      // Silent fail in testing environments
    }
  }
}

function next() {
  const player = getPlayerStore();
  if (queue.length === 0) return;

  if (player.shuffle) {
    const remaining = queue.filter((_, i) => i !== currentIndex);
    if (remaining.length === 0) {
      if (player.repeat === "all") {
        currentIndex = 0;
        player.play(queue[0]);
      } else if (autoplay) {
        // Autoplay: start similar radio from the last track
        try {
          const radio = getRadioStore();
          if (!radio.active) {
            const lastTrack = queue[currentIndex];
            if (lastTrack) {
              radio.startRadio("similar", {
                seedId: lastTrack.id,
                label: lastTrack.title,
                autoplay: true,
              });
              sourceContext = "autoplay";
            }
          }
        } catch {}
      }
      return;
    }
    const randomTrack = remaining[Math.floor(Math.random() * remaining.length)];
    currentIndex = queue.indexOf(randomTrack);
    player.play(randomTrack);
  } else {
    if (currentIndex < queue.length - 1) {
      currentIndex++;
      player.play(queue[currentIndex]);
    } else if (player.repeat === "all") {
      currentIndex = 0;
      player.play(queue[0]);
    } else if (autoplay) {
      // Autoplay: start similar radio from the last track
      try {
        const radio = getRadioStore();
        if (!radio.active) {
          const lastTrack = queue[currentIndex];
          if (lastTrack) {
            radio.startRadio("similar", {
              seedId: lastTrack.id,
              label: lastTrack.title,
              autoplay: true,
            });
            sourceContext = "autoplay";
          }
        }
      } catch {}
    }
  }

  // Radio auto-fetch: mark played and fetch more if running low
  try {
    const radio = getRadioStore();
    if (radio.active && !radio.exhausted && !radio.loading) {
      if (queue[currentIndex]) {
        radio.markPlayed(queue[currentIndex].id);
      }
      if (currentIndex >= queue.length - 3) {
        radio.fetchMoreTracks();
      }
    }
  } catch {}
}

function previous() {
  const player = getPlayerStore();
  if (player.progress > 3) {
    player.seek(0);
    return;
  }

  if (currentIndex > 0) {
    currentIndex--;
    player.play(queue[currentIndex]);
  }
}

// Listen for track ended events
if (typeof window !== "undefined") {
  window.addEventListener("soundtime:trackended", () => {
    next();
  });
  // Listen for Media Session previous/next track actions
  window.addEventListener("soundtime:previoustrack", () => {
    previous();
  });
  window.addEventListener("soundtime:nexttrack", () => {
    next();
  });
}

export function getQueueStore() {
  return {
    get queue() { return queue; },
    get currentIndex() { return currentIndex; },
    get currentTrack() { return queue[currentIndex] ?? null; },
    get hasNext() { return currentIndex < queue.length - 1; },
    get hasPrevious() { return currentIndex > 0; },
    /** Current playback source context (e.g. "album", "playlist", "radio"), or `null` if unset. */
    get sourceContext() { return sourceContext; },
    get radioMode() {
      try { return getRadioStore().active; } catch { return false; }
    },
    get autoplay() { return autoplay; },
    playQueue,
    addToQueue,
    addNext,
    removeFromQueue,
    moveInQueue,
    clearQueue,
    toggleAutoplay,
    next,
    previous,
  };
}
