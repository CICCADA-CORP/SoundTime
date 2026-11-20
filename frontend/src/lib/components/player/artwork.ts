import { API_BASE } from "$lib/api";
import { showcaseAudio } from "$lib/showcase";
import type { Track } from "$lib/types";

export function playerArtwork(track: Track) {
  const url = track.cover_url;
  if (!url || showcaseAudio(track.id) || /^https?:\/\//i.test(url)) return url;
  return API_BASE.replace(/\/api$/, "") + url;
}
