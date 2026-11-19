import { homeApi } from "$lib/api";
import { catalogQuery } from "$lib/catalog/query.svelte";
import { getAuthStore } from "$lib/stores/auth.svelte";

export function createCatalog() {
  const auth = getAuthStore();
  const popular = catalogQuery(async () => (await homeApi.popularTracks(12)).data);
  const picks = catalogQuery(() => homeApi.randomTracks(4));
  const albums = catalogQuery(async () => (await homeApi.recentAlbums(4)).data);
  const artists = catalogQuery(async () => (await homeApi.topArtists(4)).data);
  const editorial = catalogQuery(homeApi.editorialPlaylists);
  const stats = catalogQuery(homeApi.statsOverview);
  const history = catalogQuery(() => auth.user?.id ? homeApi.recentHistory(6) : null);
  const catalog = [popular, picks, albums, artists, editorial, stats];

  return {
    popular, picks, albums, artists, editorial, stats, history,
    get loading() { return catalog.some((section) => section.loading); },
    get offline() { return catalog.every((section) => section.failed); },
    get incomplete() { return [...catalog, history].some((section) => section.failed); },
    retry() {
      for (const section of [...catalog, history]) {
        if (section.failed) section.retry();
      }
    },
  };
}
