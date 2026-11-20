<script lang="ts">
  import { ChevronDown, Radio } from "lucide-svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import { getLocale, t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "./CatalogState.svelte";
  import GenreEntry from "./GenreEntry.svelte";
  import SectionHeading from "./SectionHeading.svelte";

  let { genres }: { genres: string[] } = $props();
  const id = $props.id();
  const radio = getRadioStore();
  const previewCount = 8;
  const ordered = $derived([...new Set(genres)].sort((a, b) => a.localeCompare(b, getLocale())));
  const active = $derived(genres.find(genre => genre === page.url.searchParams.get("genre")) ?? "");
  let expanded = $state(false);
  const tracks = catalogQuery(() => active ? homeApi.genreTracks(active, 10) : null);
  let visibleGenres = $derived.by(() => {
    if (expanded || ordered.length <= previewCount) return ordered;
    const preview = ordered.slice(0, previewCount);
    if (active && !preview.includes(active)) preview[preview.length - 1] = active;
    return preview;
  });
  function selectGenre(genre: string) {
    const url = new URL(page.url.href);
    if (genre === active) url.searchParams.delete("genre");
    else url.searchParams.set("genre", genre);
    void goto(url, { reset: false });
  }
</script>

<section aria-label={t("explore.genres")} class="space-y-5">
  <div>
    <SectionHeading title={t("explore.genres")} />
    <div id={id} class="genre-index">
      {#each visibleGenres as genre (genre)}
        <button aria-pressed={active === genre} onclick={() => selectGenre(genre)}><GenreEntry {genre} /></button>
      {/each}
    </div>
    {#if ordered.length > previewCount}
      <Button variant="ghost" size="sm" aria-expanded={expanded} aria-controls={id} onclick={() => expanded = !expanded}>
        {expanded ? t("common.close") : t("explore.viewAll")}
        <ChevronDown size={14} class={expanded ? "rotate-180" : ""} />
      </Button>
    {/if}
  </div>
  {#if active}
    {#if tracks.data?.data.length}
      <div class="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-5">
        <h2 class="text-xl font-semibold tracking-tight">{active}</h2>
        <Button variant="outline" disabled={radio.loading} onclick={() => radio.startRadio("genre", { genre: active, label: active })}>
          <Radio size={16} />{t("radio.startGenreRadio")}
        </Button>
      </div>
      {#if radio.error}<p role="alert" class="text-sm text-destructive">{t("radio.error")}</p>{/if}
      <TrackList source="explore" tracks={tracks.data.data} />
    {:else}
      <CatalogState loading={tracks.loading} failed={tracks.failed} status={tracks.status}
        retry={tracks.retry} message={t("explore.noTracks")} />
    {/if}
  {/if}
</section>

<style>
  .genre-index { display: flex; flex-wrap: wrap; gap: .5rem; }
  .genre-index button { min-width: 0; border-radius: 999px; }
</style>
