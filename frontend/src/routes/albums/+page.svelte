<script lang="ts">
  import type { Album } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import PaginatedShelf from "$lib/components/catalog/PaginatedShelf.svelte";
</script>

<svelte:head><title>{t("explore.albums")} — SoundTime</title></svelte:head>

<PaginatedShelf path="/albums" title={t("explore.albums")} empty={t("explore.noAlbums")}>
  {#snippet card(album: Album)}<AlbumCard {album} />{/snippet}
</PaginatedShelf>
