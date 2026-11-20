<script lang="ts">
  import { ChevronRight, ListMusic } from "lucide-svelte";
  import { formatTrackCount, getLocale, t } from "$lib/i18n/index.svelte";
  import type { createCatalog } from "./catalog.svelte";

  let { catalog }: { catalog: ReturnType<typeof createCatalog> } = $props();
  const number = $derived(new Intl.NumberFormat(getLocale()));
</script>

{#snippet heading(title: string, href?: string)}
  <div class="mb-3.5 flex items-center justify-between gap-4">
    <h2 class="subheading">{title}</h2>
    {#if href}
      <a {href} class="inline-flex shrink-0 items-center gap-1 text-[13px] font-medium text-muted-foreground hover:underline">
        {t("explore.viewAll")}<ChevronRight size={15} />
      </a>
    {/if}
  </div>
{/snippet}

{#if catalog.editorial.data?.length}
  <section class="mt-8">
    {@render heading(t("home.editorialPicks"), "/explore?tab=playlists")}
    <div class="grid gap-2.5 sm:grid-cols-2">
      {#each catalog.editorial.data as playlist (playlist.id)}
        <a href={"/playlists/" + playlist.id} class="group flex min-w-0 items-center gap-3 rounded-lg px-2 py-2 hover:bg-muted">
          <span class="grid size-12 shrink-0 place-items-center overflow-hidden rounded-lg bg-secondary text-primary">
            {#if playlist.cover_url}<img src={playlist.cover_url} alt="" loading="lazy" class="size-full object-cover" />
            {:else}<ListMusic size={24} />{/if}
          </span>
          <span class="min-w-0 flex-1">
            <span class="block truncate text-sm font-semibold">{playlist.name}</span>
            <span class="block truncate text-xs text-muted-foreground">{formatTrackCount(playlist.track_count)}</span>
          </span>
          <ChevronRight size={16} class="mr-2 text-muted-foreground group-hover:text-primary" />
        </a>
      {/each}
    </div>
  </section>
{/if}

{#if catalog.stats.data && catalog.stats.data.total_tracks > 0}
  {@const stats = catalog.stats.data}
  <details class="summary">
    <summary>{t("home.networkStats")}</summary>
    <dl class="grid grid-cols-2 gap-3 sm:grid-cols-4">
      {#each [
        [t("stats.totalTracks"), stats.total_tracks],
        [t("stats.totalAlbums"), stats.total_albums],
        [t("stats.totalArtists"), stats.total_artists],
        [t("stats.totalDuration"), Math.round(stats.total_duration_secs / 3600)],
      ] as [label, value] (label)}
        <div class="flex flex-col-reverse items-start gap-1 border-t border-border py-3">
          <dt class="text-xs text-muted-foreground">{label}</dt>
          <dd class="text-xl tabular-nums">{number.format(Number(value))}</dd>
        </div>
      {/each}
    </dl>
  </details>
{/if}

<style>
  .summary { margin-top: 1.8rem; padding: .75rem 0; border-top: 1px solid hsl(var(--border)); color: hsl(var(--muted-foreground)); }
  summary { width: fit-content; cursor: pointer; font-size: .8rem; font-weight: 550; }
  .summary dl { margin-top: .7rem; }
  .subheading { font-size: 1.3rem; font-weight: 625; line-height: 1.2; letter-spacing: -.035em; }
</style>
