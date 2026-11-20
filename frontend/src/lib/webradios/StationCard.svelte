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
  const accent = $derived(["#a95d35", "#397575", "#7c5279", "#526789", "#90723c"][parseInt(station.id.slice(-2), 16) % 5] ?? "#a95d35");
  const location = $derived([station.country, station.language].filter(Boolean).join(" · "));
  const quality = $derived([station.codec, station.bitrate ? station.bitrate + " kbps" : null].filter(Boolean).join(" · "));
  const fallback = $derived([location, quality].filter(Boolean).join(" · "));
  const description = $derived(station.description || fallback);
  const hasOwnDescription = $derived(description !== fallback);
</script>

<article class="station flex min-w-0 flex-col gap-3 rounded-xl border border-border bg-card p-4" class:active style:--accent={accent}>
  <div class="flex items-center gap-3">
    <div class="logo"><StationIcon source={station.icon_url} /></div>
    <div class="min-w-0 flex-1">
      <h2 class="break-words text-base font-semibold leading-snug">{station.name}</h2>
      {#if hasOwnDescription && location}
        <p class="mt-0.5 break-words text-xs text-muted-foreground">{location}</p>
      {/if}
    </div>
    <button type="button" class="station-play grid size-11 shrink-0 place-items-center rounded-full"
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

<style>
  .station { position: relative; border-color: color-mix(in srgb, var(--accent) 23%, hsl(var(--border))); background: linear-gradient(120deg, color-mix(in srgb, var(--accent) 6%, hsl(var(--card))), hsl(var(--card)) 75%); }
  .station.active { box-shadow: inset 3px 0 var(--accent); }
  .logo { width: 3rem; height: 3rem; flex-shrink: 0; }
  .logo > :global(span) { background: color-mix(in srgb, var(--accent) 13%, hsl(var(--card))); color: color-mix(in srgb, var(--accent) 65%, hsl(var(--foreground))); }
  .station-play { background: hsl(var(--foreground)); color: hsl(var(--background)); }
  .station-play:hover { background: color-mix(in srgb, var(--accent) 28%, hsl(var(--foreground))); }
</style>
