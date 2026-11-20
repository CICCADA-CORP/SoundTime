<script lang="ts">
  import type { Artist } from "$lib/types";
  import CatalogCard from "./catalog/CatalogCard.svelte";
  let { artist }: { artist: Artist } = $props();
</script>

<CatalogCard artworkId={artist.id} kind="artist" href="/artists/{artist.id}" title={artist.name} image={artist.image_url} description={artist.bio?.trim()} />
