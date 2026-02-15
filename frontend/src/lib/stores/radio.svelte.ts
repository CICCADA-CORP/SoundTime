import type { RadioSeedType } from "$lib/types";
import { radioApi } from "$lib/api";
import { getQueueStore } from "./queue.svelte";

// ─── State ──────────────────────────────────────────────────────────

let active = $state(false);
let seedType = $state<RadioSeedType | null>(null);
let seedId = $state<string | null>(null);
let seedGenre = $state<string | null>(null);
let seedLabel = $state<string>("");
let playedIds = $state<Set<string>>(new Set());
let loading = $state(false);
let exhausted = $state(false);

// ─── Functions ──────────────────────────────────────────────────────

/**
 * Start a new radio session.
 * Clears the queue, resets state, fetches initial tracks, starts playback.
 */
async function startRadio(
  type: RadioSeedType,
  opts: { seedId?: string; genre?: string; label: string }
) {
  // Reset state
  active = true;
  seedType = type;
  seedId = opts.seedId ?? null;
  seedGenre = opts.genre ?? null;
  seedLabel = opts.label;
  playedIds = new Set();
  loading = true;
  exhausted = false;

  try {
    const res = await radioApi.next({
      seed_type: type,
      seed_id: opts.seedId,
      genre: opts.genre,
      count: 5,
      exclude: [],
    });

    if (res.tracks.length === 0) {
      exhausted = true;
      active = false;
      return;
    }

    exhausted = res.exhausted;

    // Add fetched track IDs to played set
    for (const track of res.tracks) {
      playedIds = new Set([...playedIds, track.id]);
    }

    // Start playback through queue
    const queue = getQueueStore();
    queue.playQueue(res.tracks, 0);
  } catch {
    active = false;
  } finally {
    loading = false;
  }
}

/**
 * Stop the radio. Preserves the current queue (user can keep listening).
 */
function stopRadio() {
  active = false;
  seedType = null;
  seedId = null;
  seedGenre = null;
  seedLabel = "";
  exhausted = false;
}

/**
 * Fetch more tracks when the queue is running low.
 * Called from queue.svelte.ts when currentIndex >= queue.length - 3.
 */
async function fetchMoreTracks() {
  if (!active || exhausted || loading || !seedType) return;

  loading = true;
  try {
    // Limit exclude to 2000 most recent IDs
    const excludeArray = [...playedIds];
    const exclude = excludeArray.length > 2000
      ? excludeArray.slice(excludeArray.length - 2000)
      : excludeArray;

    const res = await radioApi.next({
      seed_type: seedType,
      seed_id: seedId ?? undefined,
      genre: seedGenre ?? undefined,
      count: 5,
      exclude,
    });

    if (res.tracks.length === 0) {
      exhausted = true;
      return;
    }

    exhausted = res.exhausted;

    // Add new track IDs to played set
    for (const track of res.tracks) {
      playedIds = new Set([...playedIds, track.id]);
    }

    // Append to queue
    const queue = getQueueStore();
    for (const track of res.tracks) {
      queue.addToQueue(track);
    }
  } catch {
    // Silent failure — will retry on next track change
  } finally {
    loading = false;
  }
}

/**
 * Mark a track as played (add to exclusion set).
 * Called from queue.svelte.ts when a track starts playing in radio mode.
 */
function markPlayed(trackId: string) {
  playedIds = new Set([...playedIds, trackId]);
}

// ─── Exported Store ──────────────────────────────────────────────────

export function getRadioStore() {
  return {
    get active() { return active; },
    get seedType() { return seedType; },
    get seedLabel() { return seedLabel; },
    get loading() { return loading; },
    get exhausted() { return exhausted; },
    get playedCount() { return playedIds.size; },
    startRadio,
    stopRadio,
    fetchMoreTracks,
    markPlayed,
  };
}
