<script lang="ts">
  import { Play, Square } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { live } from "./live.svelte";
  import StationIcon from "./StationIcon.svelte";
  import type { WebradioStation } from "./types";

  let { station, tagLink }: { station: WebradioStation; tagLink: (tag: string) => string } = $props();
  const player = getPlayerStore();
  const active = $derived(live.station?.id === station.id);
  const location = $derived([station.country, station.language].filter(Boolean).join(" · "));
  const quality = $derived([station.codec, station.bitrate ? station.bitrate + " kbps" : null].filter(Boolean).join(" · "));
  const fallback = $derived([location, quality].filter(Boolean).join(" · "));
  const description = $derived(station.description || fallback);
  const hasOwnDescription = $derived(description !== fallback);
</script>

<article class="flex min-w-0 flex-col gap-3 rounded-xl border border-border bg-card p-4">
  <div class="flex items-center gap-3">
    <div class="size-12 shrink-0"><StationIcon source={station.icon_url} /></div>
    <div class="min-w-0 flex-1">
      <h2 class="break-words text-sm font-semibold leading-snug">{station.name}</h2>
      {#if hasOwnDescription && location}
        <p class="mt-0.5 break-words text-xs text-muted-foreground">{location}</p>
      {/if}
    </div>
    <button type="button" class="grid size-10 shrink-0 place-items-center rounded-full border border-border hover:bg-accent"
      aria-label={t(active ? "webradios.stopStation" : "webradios.playStation", { name: station.name })}
      onclick={() => active ? live.stop() : player.playStation(station)}>
      {#if active}<Square size={15} />{:else}<Play size={15} fill="currentColor" />{/if}
    </button>
  </div>
  {#if description}<p class="break-words text-sm text-muted-foreground">{description}</p>{/if}
  {#if station.tags?.length}
    <div class="flex flex-wrap gap-1.5">
      {#each station.tags as tag (tag)}
        <a href={tagLink(tag)} class="max-w-full break-words rounded-full bg-secondary px-2.5 py-1 text-xs hover:underline">{tag}</a>
      {/each}
    </div>
  {/if}
  {#if hasOwnDescription && quality}
    <p class="mt-auto text-xs text-muted-foreground">{quality}</p>
  {/if}
</article>
