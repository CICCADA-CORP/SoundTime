<script lang="ts">
  import { signInLink } from "$lib/access/navigation";
  import { page } from "$app/state";
  import { currentCatalogPage, selectCatalogPage } from "$lib/catalog/pagination";
  import { Disc3, Play } from "lucide-svelte";
  import { availabilityLabel, canPlay } from "$lib/catalog/availability";
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { formatDuration } from "$lib/utils";
  import { getLocale, t } from "$lib/i18n/index.svelte";
  import type { HistoryEntry, PaginatedResponse } from "$lib/types";
  import { Button } from "$lib/components/ui/button";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import PageControls from "$lib/components/catalog/PageControls.svelte";

  const auth = getAuthStore();
  const player = getPlayerStore();
  const current = $derived(currentCatalogPage());
  const history = catalogQuery(signal => auth.isAuthenticated ? api.get<PaginatedResponse<HistoryEntry>>(`/history?page=${current}&per_page=50`, signal) : null);
  let date = $derived(new Intl.DateTimeFormat(getLocale(), { day: "numeric", month: "short", year: "numeric" }));
</script>

<svelte:head><title>{t("history.title")} — SoundTime</title></svelte:head>

<div class="space-y-6">
  <CatalogHeader title={t("history.title")} />
  {#if !auth.isAuthenticated}
    <CatalogState message={t("library.signInPrompt")}><Button href={signInLink(page.url)}>{t("auth.signIn")}</Button></CatalogState>
  {:else if history.data?.data.length}
    <div class="divide-y divide-border">
      {#each history.data.data as { id, track, listened_at } (id)}
        <button disabled={!canPlay(track)} class="group flex w-full items-center gap-3 rounded-md p-2 text-left hover:bg-secondary" onclick={() => player.play(track)}>
          <span class="relative grid size-11 shrink-0 place-items-center overflow-hidden rounded-md bg-secondary text-muted-foreground">
            {#if track.cover_url}<img src={track.cover_url} loading="lazy" alt="" class="size-full object-cover" />
            {:else}<Disc3 size={21} />{/if}
            <span class="absolute inset-0 hidden place-items-center bg-black/60 text-white group-hover:grid group-focus-visible:grid"><Play size={15} fill="currentColor" /></span>
          </span>
          <span class="min-w-0 flex-1"><span class="block truncate text-sm font-medium">{track.title}</span>
            {#if !canPlay(track)}<span class="block text-xs text-muted-foreground">{availabilityLabel(track)}</span>{/if}
            {#if track.artist_name}<span class="block truncate text-xs text-muted-foreground">{track.artist_name}</span>{/if}
          </span>
          <time datetime={listened_at} class="text-xs text-muted-foreground">{date.format(new Date(listened_at))}</time>
          <span class="hidden w-12 text-right text-xs tabular-nums text-muted-foreground sm:inline">{formatDuration(track.duration_secs)}</span>
        </button>
      {/each}
    </div>
    <PageControls page={current} total={history.data.total_pages} select={selectCatalogPage} />
  {:else}
    <CatalogState loading={history.loading} failed={history.failed} status={history.status} retry={history.retry} message={t("catalog.historyEmpty")} />
  {/if}
</div>
