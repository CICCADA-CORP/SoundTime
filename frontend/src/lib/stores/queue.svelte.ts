import { canPlay } from "$lib/catalog/availability";
import type { Track } from "$lib/types";
import { showcaseAudio } from "$lib/showcase";
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
 * A manually chosen queue replaces the active station.
 */
function playQueue(tracks: Track[], startIndex = 0, source?: import("$lib/types").PlaybackSource) {
  const radio = getRadioStore();
  if (radio.active && source !== "radio" && source !== "autoplay") radio.stopRadio();

  sourceContext = source ?? null;
  queue = [...tracks];
  originalQueue = [...tracks];
  currentIndex = queue.findIndex((track, i) => i >= startIndex && canPlay(track));
  const player = getPlayerStore();
  if (queue[currentIndex]) {
    player.play(queue[currentIndex], source === "radio" || source === "autoplay");
  } else {
    player.pause();
  }
}

function addToQueue(track: Track) {
  if (!canPlay(track)) return;
  queue = [...queue, track];
  originalQueue = [...originalQueue, track];
}

function addNext(track: Track) {
  if (!canPlay(track)) return;
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

function startAutoplay() {
  const lastTrack = queue[currentIndex];
  const radio = getRadioStore();
  if (!lastTrack || radio.active || showcaseAudio(lastTrack.id)) return;
  void radio.startRadio("similar", {
    seedId: lastTrack.id,
    label: lastTrack.title,
    autoplay: true,
  });
  sourceContext = "autoplay";
}

function next() { advance(false); }

function advance(automatic: boolean) {
  const player = getPlayerStore();
  if (queue.length === 0) return;

  const first = queue.findIndex(canPlay);
  if (first < 0) {
    currentIndex = -1;
    player.pause();
    if (getRadioStore().active) getRadioStore().stopRadio();
    return;
  }
  let candidate = -1;
  if (player.shuffle) {
    const choices = queue.flatMap((track, i) => i !== currentIndex && canPlay(track) ? [i] : []);
    if (choices.length) candidate = choices[Math.floor(Math.random() * choices.length)];
  } else {
    candidate = queue.findIndex((track, i) => i > currentIndex && canPlay(track));
  }
  if (candidate < 0 && player.repeat === "all") candidate = first;
  if (candidate < 0) {
    if (autoplay && queue[currentIndex] && canPlay(queue[currentIndex])) startAutoplay();
    return;
  }
  currentIndex = candidate;
  player.play(queue[currentIndex], automatic);

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
  if (player.progress > 3 && queue[currentIndex] && canPlay(queue[currentIndex])) {
    player.seek(0);
    return;
  }

  for (let i = currentIndex - 1; i >= 0; i--) {
    if (canPlay(queue[i])) {
      currentIndex = i;
      player.play(queue[i], false);
      return;
    }
  }
}

// Keep saved rows and their order when a source goes offline or returns.
function updateAvailability(track: Track, skip: boolean) {
  const wasCurrent = queue[currentIndex]?.id === track.id;
  queue = queue.map(item => item.id === track.id ? { ...item, availability: track.availability } : item);
  if (wasCurrent && skip && !canPlay(track)) advance(true);
}

// Listen for track ended events
if (typeof window !== "undefined") {
  window.addEventListener("soundtime:trackended", () => {
    advance(true);
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
    get hasNext() { return queue.some((track, i) => i > currentIndex && canPlay(track)); },
    get hasPrevious() { return queue.some((track, i) => i < currentIndex && canPlay(track)); },
    /** Current playback source context (e.g. "album", "playlist", "radio"), or `null` if unset. */
    get sourceContext() { return sourceContext; },
    get radioMode() {
      try { return getRadioStore().active; } catch { return false; }
    },
    get autoplay() { return autoplay; },
    updateAvailability,
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
