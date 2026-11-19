import { api } from "$lib/api";
import type { PaginatedResponse, Playlist } from "$lib/types";
import { createCatalogCollection } from "./query.svelte";

export const { query: playlistQuery, invalidate: invalidatePlaylists } = createCatalogCollection();

export function recentPlaylists() {
  return playlistQuery(signal => api.get<{ data?: unknown }>("/playlists?per_page=4", signal));
}

export function ownedPlaylists(currentPage: () => number | null) {
  return playlistQuery(signal => {
    const page = currentPage();
    return page === null ? null : api.get<PaginatedResponse<Playlist>>(`/me/playlists?page=${page}&per_page=20`, signal);
  });
}
