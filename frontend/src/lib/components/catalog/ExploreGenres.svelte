<script lang="ts">
  import { Radio } from "lucide-svelte";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import TrackList from "$lib/components/TrackList.svelte";
  import CatalogState from "./CatalogState.svelte";

  let { genres }: { genres: string[] } = $props();
  const radio = getRadioStore();
  let active = $state("");
  const tracks = catalogQuery(() => active ? homeApi.genreTracks(active, 10) : null);
</script>

<section class="space-y-4">
  <h2 class="text-xl font-semibold tracking-tight">{t("explore.genres")}</h2>
  <div class="flex flex-wrap gap-2">
    {#each genres as genre (genre)}
      <Button variant={active === genre ? "default" : "outline"} aria-pressed={active === genre}
        class="rounded-full" onclick={() => active = active === genre ? "" : genre}>{genre}</Button>
    {/each}
  </div>
  {#if active}
    {#if tracks.data?.data.length}
      <div class="flex flex-wrap items-center justify-between gap-3">
        <h3 class="font-semibold">{active}</h3>
        <Button variant="secondary" disabled={radio.loading} onclick={() => radio.startRadio("genre", { genre: active, label: active })}>
          <Radio size={16} />{t("radio.startGenreRadio")}
        </Button>
      </div>
      {#if radio.error}<p role="alert" class="text-sm text-destructive">{t("radio.error")}</p>{/if}
      <TrackList source="explore" tracks={tracks.data.data} />
    {:else}
      <CatalogState loading={tracks.loading} failed={tracks.failed} status={tracks.status} retry={tracks.retry} message={t("explore.noTracks")} />
    {/if}
  {/if}
</section>
