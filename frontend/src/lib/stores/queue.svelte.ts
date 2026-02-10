import type { Track } from "$lib/types";
import { getPlayerStore } from "./player.svelte";

let queue = $state<Track[]>([]);
let currentIndex = $state(-1);
let originalQueue = $state<Track[]>([]);

function playQueue(tracks: Track[], startIndex = 0) {
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
    playQueue,
    addToQueue,
    addNext,
    removeFromQueue,
    clearQueue,
    next,
    previous,
  };
}
