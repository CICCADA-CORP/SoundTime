import { createCatalogCollection } from "./query.svelte";

export const { query: favoriteQuery, invalidate: invalidateFavorites } = createCatalogCollection();
