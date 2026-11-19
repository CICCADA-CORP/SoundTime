<script lang="ts">
  import { ChevronDown, Radio } from "lucide-svelte";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "./CatalogState.svelte";

  let { genres }: { genres: string[] } = $props();
  const id = $props.id();
  const radio = getRadioStore();
  const previewCount = 8;
  let active = $state("");
  let expanded = $state(false);
  const tracks = catalogQuery(() => active ? homeApi.genreTracks(active, 10) : null);
  let visibleGenres = $derived.by(() => {
    if (expanded || genres.length <= previewCount) return genres;
    const preview = genres.slice(0, previewCount);
    if (active && !preview.includes(active)) preview[preview.length - 1] = active;
    return preview;
  });
</script>

<section aria-label={t("explore.genres")} class="space-y-5">
  <div class="space-y-2">
    <div id={id} class="flex flex-wrap gap-2 {expanded ? 'max-h-64 overflow-y-auto p-1' : ''}">
      {#each visibleGenres as genre (genre)}
        <Button variant={active === genre ? "default" : "outline"} aria-pressed={active === genre}
          class="max-w-full justify-start" onclick={() => active = active === genre ? "" : genre}>
          <span class="truncate">{genre}</span>
        </Button>
      {/each}
    </div>
    {#if genres.length > previewCount}
      <Button variant="ghost" size="sm" aria-expanded={expanded} aria-controls={id} onclick={() => expanded = !expanded}>
        {expanded ? t("common.close") : t("explore.viewAll")}
        <ChevronDown size={14} class={expanded ? "rotate-180" : ""} />
      </Button>
    {/if}
  </div>

  {#if active}
    {#if tracks.data?.data.length}
      <div class="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-5">
        <h2 class="text-xl font-semibold tracking-tight">{t("explore.tracks")}</h2>
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
