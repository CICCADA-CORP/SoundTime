<script lang="ts">
  import SectionHeading from "$lib/components/catalog/SectionHeading.svelte";
  import GenreEntry from "$lib/components/catalog/GenreEntry.svelte";
  import { getLocale, t } from "$lib/i18n/index.svelte";

  let { genres }: { genres: string[] } = $props();
  const selection = $derived([...new Set(genres)].sort((a, b) => a.localeCompare(b, getLocale())).slice(0, 6));
</script>

<section>
  <SectionHeading title={t("explore.genres")} href="/explore?tab=genres" />
  <ul>
    {#each selection as genre (genre)}
      <li><a href={"/explore?" + new URLSearchParams({ tab: "genres", genre })}><GenreEntry {genre} /></a></li>
    {/each}
  </ul>
</section>

<style>
  ul { display: flex; flex-wrap: wrap; gap: .5rem; }
  li { min-width: 0; }
  a { display: block; border-radius: 999px; }
</style>
