<script lang="ts">
  import type { Artist } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import CatalogCard from "./catalog/CatalogCard.svelte";
  let { artist }: { artist: Artist } = $props();
</script>

<CatalogCard kind="artist" href="/artists/{artist.id}" title={artist.name} image={artist.image_url} description={t("track.artist")} />
