<script lang="ts">
  import type { Album, Artist, Track } from "$lib/types";
  import { homeCopy } from "./copy";
  import { t } from "$lib/i18n/index.svelte";
  import CatalogArtwork from "$lib/components/catalog/CatalogArtwork.svelte";
  import SectionHeading from "$lib/components/catalog/SectionHeading.svelte";

  let { artists, albums, tracks }: { artists: Artist[]; albums: Album[]; tracks: Track[] } = $props();
  const portraits = $derived.by(() => {
    const records = new Map<string, { cover: string; title: string; id: string }>();
    for (const record of [...albums, ...tracks]) {
      if (record.artist_id && record.cover_url && !records.has(record.artist_id)) {
        records.set(record.artist_id, { cover: record.cover_url, title: record.title, id: record.id });
      }
    }
    return artists.map(artist => ({ artist, record: records.get(artist.id) }))
      .sort((a, b) => Number(!!b.artist.image_url) - Number(!!a.artist.image_url) || Number(!!b.record) - Number(!!a.record)).slice(0, 6);
  });
</script>

<section class="artists">
  <SectionHeading title={t("explore.discoverArtists")} href="/artists" linkLabel={t("explore.allArtists")} />
  <ul>
    {#each portraits as { artist, record } (artist.id)}
      <li>
        <a href={"/artists/" + encodeURIComponent(artist.id)} aria-label={artist.name}>
          {#if !artist.image_url && record}
            <span class="portrait record"><CatalogArtwork source={record.cover} artworkId={record.id} kind="album" title={homeCopy().cover.replace("{title}", record.title)} /></span>
          {:else}
            <span class="portrait" aria-hidden="true"><CatalogArtwork source={artist.image_url} artworkId={artist.id} kind="artist" title={artist.name} /></span>
          {/if}
          <strong>{artist.name}</strong>
        </a>
      </li>
    {/each}
  </ul>
</section>

<style>
  .artists { min-width: 0; container-type: inline-size; }
  ul { display: grid; grid-auto-flow: column; --gap: clamp(.7rem, 2.8cqi, 1.5rem); gap: var(--gap);
    grid-auto-columns: minmax(4.1rem, calc((100% - 5 * var(--gap)) / 6)); overflow-x: auto; scrollbar-width: none; scroll-snap-type: x proximity; }
  ul::-webkit-scrollbar { display: none; }
  li { min-width: 0; scroll-snap-align: start; }
  a { display: grid; justify-items: center; min-width: 0; padding: 2px; gap: .5rem; border-radius: .5rem; }
  a:focus-visible { outline-offset: -2px; }
  .portrait { width: min(100%, 6.3rem); aspect-ratio: 1; display: block; overflow: hidden; border-radius: 50%; }
  .record { position: relative; padding: 5%; background: repeating-radial-gradient(circle closest-side, #35404b 0 1px, #29323d 2px 3px); }
  .record::after { content: ""; position: absolute; width: 6%; height: 6%; left: 47%; top: 47%; border-radius: 50%; background: #29323d; box-shadow: 0 0 0 1px #ffffff70; }
  strong { width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: center; font-size: clamp(.73rem, 1.8cqi, .86rem); font-weight: 520; }
  a:hover strong { text-decoration: underline; text-underline-offset: .15em; }
</style>
