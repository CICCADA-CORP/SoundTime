import { homeApi } from "$lib/api";
import { catalogSection } from "$lib/catalog/query.svelte";
import { getAuthStore } from "$lib/stores/auth.svelte";

export function createCatalog() {
  const auth = getAuthStore();
  const popular = catalogSection(async signal => (await homeApi.popularTracks(36, signal)).data);
  const picks = catalogSection(signal => homeApi.randomTracks(4, undefined, signal));
  const albums = catalogSection(async signal => (await homeApi.recentAlbums(8, signal)).data);
  const recent = catalogSection(async signal => (await homeApi.recentlyAdded(36, signal)).data);
  const genres = catalogSection(homeApi.genres);
  const artists = catalogSection(async signal => (await homeApi.topArtists(12, signal)).data);
  const editorial = catalogSection(homeApi.editorialPlaylists);
  const stats = catalogSection(homeApi.statsOverview);
  const history = catalogSection(signal => auth.user?.id ? homeApi.recentHistory(6, signal) : null);
  const catalog = [popular, picks, albums, recent, genres, artists, editorial, stats];

  return {
    popular, picks, albums, recent, genres, artists, editorial, stats, history,
    get loading() { return catalog.some((section) => section.loading); },
    get initialLoading() { return catalog.some(section => section.initialLoading) || history.initialLoading; },
    get offline() { return catalog.every((section) => section.failed); },
    get incomplete() { return [...catalog, history].some((section) => section.failed); },
    retry() {
      for (const section of [...catalog, history]) {
        if (section.failed) section.retry();
      }
    },
  };
}
