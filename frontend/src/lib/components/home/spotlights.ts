import type { Track } from "$lib/types";
import { canPlay } from "$lib/catalog/availability";

export function availableTracks(feeds: (Track[] | null | undefined)[]): Track[] {
  const byId = new Map<string, Track>();
  for (const tracks of feeds) {
    for (const track of tracks ?? []) {
      if (!canPlay(track)) continue;
      const previous = byId.get(track.id);
      if (!previous || (!previous.cover_url && track.cover_url)) byId.set(track.id, track);
    }
  }
  return [...byId.values()];
}

/** Give each distinct release a place before repeating the same artwork. */
export function spotlightTracks(tracks: Track[]) {
  const images = new Set<string>();
  const illustrated: Track[] = [];
  const other: Track[] = [];
  for (const track of tracks) {
    const cover = track.cover_url;
    if (cover && !images.has(cover)) {
      images.add(cover);
      illustrated.push(track);
    } else {
      other.push(track);
    }
  }
  return [...illustrated, ...other].slice(0, 3);
}
