import type { Track } from "$lib/types";
import { getPlayerStore } from "./player.svelte";
import { getRadioStore } from "./radio.svelte";

let queue = $state<Track[]>([]);
let currentIndex = $state(-1);
let originalQueue = $state<Track[]>([]);

function playQueue(tracks: Track[], startIndex = 0) {
  // Stop radio if active — user is manually choosing what to play
  try {
    const radio = getRadioStore();
    if (radio.active) radio.stopRadio();
  } catch {}

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
    get radioMode() {
      try { return getRadioStore().active; } catch { return false; }
    },
    playQueue,
    addToQueue,
    addNext,
    removeFromQueue,
    moveInQueue,
    clearQueue,
    next,
    previous,
  };
}
