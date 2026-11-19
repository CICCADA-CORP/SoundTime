<script lang="ts">
  import { page } from "$app/state";
  import { ArrowLeft, ArrowRight, Search } from "lucide-svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import { Button } from "$lib/components/ui/button";
  import { catalogSection } from "$lib/catalog/query.svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import Preference from "$lib/webradios/Preference.svelte";
  import StationCard from "$lib/webradios/StationCard.svelte";
  import { webPreferences } from "$lib/webradios/preferences.svelte";
  import type { StationPage } from "$lib/webradios/types";

  const limit = 30;
  const name = $derived((page.url.searchParams.get("name") ?? "").trim().slice(0, 160));
  const tag = $derived((page.url.searchParams.get("tag") ?? "").trim().slice(0, 120));
  const offset = $derived.by(() => {
    const value = Number(page.url.searchParams.get("offset"));
    return Number.isSafeInteger(value) && value > 0 && value <= 0xffffffff - limit ? value : 0;
  });
  function parameters(name: string, tag: string, offset = 0) {
    const p = new URLSearchParams();
    if (name) p.set("name", name);
    if (tag) p.set("tag", tag);
    if (offset) p.set("offset", String(offset));
    return p;
  }
  function link(genre: string, start = 0) {
    const p = parameters(name, genre, start).toString();
    return p ? "/webradios?" + p : "/webradios";
  }
  const directory = catalogSection(signal => {
    if (!webPreferences.enabled) return null;
    const p = parameters(name, tag, offset);
    p.set("limit", String(limit));
    return api.get<StationPage>(`/webradios/stations?${p}`, signal);
  });
</script>

<svelte:head><title>{t("nav.webradios")} — SoundTime</title></svelte:head>

<div class="space-y-6">
  <CatalogHeader title={t("nav.webradios")} />
  <div class="max-w-md"><Preference /></div>
  {#if webPreferences.enabled}
    <form action="/webradios" method="GET" class="grid gap-3 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_auto] sm:items-end">
      <label class="space-y-1 text-sm font-medium">
        <span>{t("webradios.stationName")}</span>
        <input type="search" name="name" value={name} maxlength="160" class="block h-10 w-full rounded-md border border-input bg-background px-3 font-normal" />
      </label>
      <label class="space-y-1 text-sm font-medium">
        <span>{t("track.genre")}</span>
        <input type="search" name="tag" value={tag} maxlength="120" class="block h-10 w-full rounded-md border border-input bg-background px-3 font-normal" />
      </label>
      <Button type="submit" class="h-10"><Search size={16} />{t("admin.reports.searchBtn")}</Button>
    </form>

    {#if directory.data?.stations.length}
      <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {#each directory.data.stations as station (station.id)}
          <StationCard {station} tagLink={genre => link(genre)} />
        {/each}
      </div>
    {:else}
      <CatalogState loading={directory.loading} failed={directory.failed} status={directory.status}
        retry={directory.retry} message={t("common.noResults")} />
    {/if}
    {#if offset || directory.data?.next_offset != null}
      <nav aria-label={t("webradios.pagination")} class="flex justify-between gap-3">
        {#if offset}<Button href={link(tag, Math.max(0, offset - limit))} variant="outline"><ArrowLeft size={15} />{t("admin.reports.previous")}</Button>{/if}
        {#if directory.data?.next_offset != null}
          <Button href={link(tag, directory.data.next_offset)} variant="outline" class="ml-auto">{t("admin.reports.next")}<ArrowRight size={15} /></Button>
        {/if}
      </nav>
    {/if}
  {:else if !webPreferences.status}
    <CatalogState loading={!webPreferences.failed} failed={webPreferences.failed} />
  {/if}
</div>
