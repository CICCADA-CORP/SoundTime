import { podcast } from "$lib/webradios/podcast.svelte";
import { live } from "$lib/webradios/live.svelte";
import { canPlay } from "$lib/catalog/availability";
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
let error = $state<string | null>(null);
let autoplayMode = $state(false);
let session = 0;

// ─── Functions ──────────────────────────────────────────────────────

/**
 * Start a new radio session.
 * Clears the queue, resets state, fetches initial tracks, starts playback.
 */
async function startRadio(
  type: RadioSeedType,
  opts: { seedId?: string; genre?: string; label: string; autoplay?: boolean }
) {
  live.stop();
  podcast.stop();
  const current = ++session;
  active = true;
  error = null;
  seedType = type;
  seedId = opts.seedId ?? null;
  seedGenre = opts.genre ?? null;
  seedLabel = opts.label;
  playedIds = new Set();
  loading = true;
  exhausted = false;
  autoplayMode = opts.autoplay ?? false;

  try {
    const res = await radioApi.next({
      seed_type: type,
      seed_id: opts.seedId,
      genre: opts.genre,
      count: 5,
      exclude: [],
    });
    if (session !== current) return;

    const playable = res.tracks.filter(canPlay);
    if (playable.length === 0) {
      exhausted = true;
      active = false;
      return;
    }

    exhausted = res.exhausted;

    // Add fetched track IDs to played set
    for (const track of playable) {
      playedIds = new Set([...playedIds, track.id]);
    }

    // Start playback through queue
    const queue = getQueueStore();
    queue.playQueue(playable, 0, autoplayMode ? "autoplay" : "radio");
  } catch (e) {
    if (session !== current) return;
    active = false;
    error = e instanceof Error ? e.message : "Failed to start radio";
  } finally {
    if (session === current) loading = false;
  }
}

/**
 * Stop the radio. Preserves the current queue (user can keep listening).
 */
function stopRadio() {
  session++;
  loading = false;
  active = false;
  seedType = null;
  seedId = null;
  seedGenre = null;
  seedLabel = "";
  exhausted = false;
  error = null;
  autoplayMode = false;
  playedIds = new Set();
}

function retry() {
  if (!seedType || !error) return;
  return startRadio(seedType, {
    seedId: seedId ?? undefined,
    genre: seedGenre ?? undefined,
    label: seedLabel,
    autoplay: autoplayMode,
  });
}

/**
 * Fetch more tracks when the queue is running low.
 * Called from queue.svelte.ts when currentIndex >= queue.length - 3.
 */
async function fetchMoreTracks(retrying = false) {
  if (!active || exhausted || loading || !seedType) return;

  const current = session;
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
    if (session !== current) return;

    const playable = res.tracks.filter(canPlay);
    if (playable.length === 0) {
      exhausted = true;
      // If the server didn't explicitly signal exhaustion and tracks were played,
      // reset the exclude list and retry once to find previously-excluded tracks
      if (res.tracks.length === 0 && !res.exhausted && playedIds.size > 0 && !retrying) {
        playedIds = new Set();
        exhausted = false;
        loading = false;
        return await fetchMoreTracks(true);
      }
      return;
    }

    exhausted = res.exhausted;

    // If exhausted but tracks were played and not retrying, reset for next call
    if (exhausted && playedIds.size > 0 && !retrying) {
      playedIds = new Set();
      exhausted = false;
    }

    // Add new track IDs to played set
    for (const track of playable) {
      playedIds = new Set([...playedIds, track.id]);
    }

    // Append to queue
    const queue = getQueueStore();
    for (const track of playable) {
      queue.addToQueue(track);
    }
  } catch {
    // Silent failure — will retry on next track change
  } finally {
    if (session === current) loading = false;
  }
}

/**
 * Mark a track as played (add to exclusion set).
 * Called from queue.svelte.ts when a track starts playing in radio mode.
 */
function markPlayed(trackId: string) {
  playedIds = new Set([...playedIds, trackId]);
}

function clearError() {
  error = null;
}

// ─── Exported Store ──────────────────────────────────────────────────

export function getRadioStore() {
  return {
    get active() { return active; },
    get seedType() { return seedType; },
    get seedId() { return seedId; },
    get seedLabel() { return seedLabel; },
    get loading() { return loading; },
    get exhausted() { return exhausted; },
    get error() { return error; },
    get playedCount() { return playedIds.size; },
    get autoplayMode() { return autoplayMode; },
    startRadio,
    retry,
    stopRadio,
    fetchMoreTracks,
    markPlayed,
    clearError,
  };
}
