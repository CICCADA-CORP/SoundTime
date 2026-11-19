import type { Track } from "$lib/types";
import { t } from "$lib/i18n/index.svelte";

export function canPlay(track: Track) {
  return track.availability === undefined || track.availability === "playable";
}

export function availabilityLabel(track: Track): string {
  switch (track.availability) {
    case "withdrawn": return t("track.withdrawn");
    case "temporarily_unavailable": return t("track.temporarilyUnavailable");
    case "unavailable_on_instance": return t("track.unavailableOnInstance");
    default: return "";
  }
}
