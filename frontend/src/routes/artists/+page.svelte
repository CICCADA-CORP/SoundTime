<script lang="ts">
  import type { Artist } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";
  import PaginatedShelf from "$lib/components/catalog/PaginatedShelf.svelte";
</script>

<svelte:head><title>{t("explore.artists")} — SoundTime</title></svelte:head>

<PaginatedShelf path="/artists" title={t("explore.artists")} empty={t("explore.noArtists")}>
  {#snippet card(artist: Artist)}<ArtistCard {artist} />{/snippet}
</PaginatedShelf>
