<script lang="ts">
  import { page } from "$app/state";
  import { ArrowLeft, ArrowRight, Compass, Podcast, Radio, Search } from "lucide-svelte";
  import PodcastDirectory from "$lib/webradios/PodcastDirectory.svelte";
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
    return "/webradios?" + (p || "view=radios");
  }
  const section = $derived.by(() => {
    const value = page.url.searchParams.get("view");
    if (value === "podcasts" || page.url.searchParams.has("show")) return "podcasts";
    if (value === "radios" || name || tag || offset) return "radios";
    return "discover";
  });
  const directory = catalogSection(signal => {
    if (!webPreferences.enabled || section === "podcasts") return null;
    const p = parameters(name, tag, offset);
    p.set("limit", String(limit));
    return api.get<StationPage>(`/webradios/stations?${p}`, signal);
  });
</script>

<svelte:head><title>{t("nav.webradios")} — SoundTime</title></svelte:head>

<div class="broadcast-page space-y-8 sm:space-y-10">
  <header class="broadcast-hero">
    <img src="/art/broadcast-studio.jpg" alt="" fetchpriority="high" width="1440" height="720" />
    <div class="hero-copy">
      <h1>{t("nav.webradios")}</h1>
      {#if webPreferences.enabled}
        <nav aria-label={t("broadcast.browse")}>
          <a href="/webradios" aria-current={section === "discover" ? "page" : undefined}><Compass size={16} />{t("broadcast.discover")}</a>
          <a href="/webradios?view=radios" aria-current={section === "radios" ? "page" : undefined}><Radio size={16} />{t("broadcast.radios")}</a>
          <a href="/webradios?view=podcasts" aria-current={section === "podcasts" ? "page" : undefined}><Podcast size={16} />{t("broadcast.podcasts")}</a>
        </nav>
      {/if}
    </div>
  </header>
  {#if webPreferences.enabled && section !== "radios"}
    <PodcastDirectory compact={section === "discover"} />
  {/if}
  {#if webPreferences.enabled && section !== "podcasts"}
    <section aria-labelledby="broadcast-stations" class="space-y-4">
    <div class="flex items-end justify-between gap-3">
      <h2 class="section-title" id="broadcast-stations">{t("broadcast.radios")}</h2>
      {#if section === "discover"}<a class="see-all" href="/webradios?view=radios">{t("broadcast.allStations")} <ArrowRight size={15} /></a>{/if}
    </div>
    <form action="/webradios" method="GET" class="grid gap-3 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_auto] sm:items-end">
      <input type="hidden" name="view" value="radios" />
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
        {#each section === "discover" ? directory.data.stations.slice(0, 6) : directory.data.stations as station (station.id)}
          <StationCard {station} tagLink={genre => link(genre)} />
        {/each}
      </div>
    {:else}
      <CatalogState loading={directory.loading} failed={directory.failed} status={directory.status}
        retry={directory.retry} message={t("common.noResults")} />
    {/if}
    {#if section === "radios" && (offset || directory.data?.next_offset != null)}
      <nav aria-label={t("webradios.pagination")} class="flex justify-between gap-3">
        {#if offset}<Button href={link(tag, Math.max(0, offset - limit))} variant="outline"><ArrowLeft size={15} />{t("admin.reports.previous")}</Button>{/if}
        {#if directory.data?.next_offset != null}
          <Button href={link(tag, directory.data.next_offset)} variant="outline" class="ml-auto">{t("admin.reports.next")}<ArrowRight size={15} /></Button>
        {/if}
      </nav>
    {/if}
    </section>
  {:else if !webPreferences.status}
    <CatalogState loading={!webPreferences.failed} failed={webPreferences.failed} />
  {/if}
  <div class="max-w-lg border-t border-border pt-5"><Preference /></div>
</div>

<style>
  .broadcast-hero { position: relative; display: grid; align-items: end; isolation: isolate; overflow: hidden; min-height: clamp(14rem, 24vw, 20rem); border-radius: 1.15rem; background: #291911; color: #fffaf4; }
  .broadcast-hero img { position: absolute; z-index: -2; width: 100%; height: 100%; object-fit: cover; object-position: center 48%; }
  .broadcast-hero::after { content: ""; position: absolute; inset: 0; z-index: -1; background: linear-gradient(90deg, rgb(16 9 13 / .77), transparent 85%), linear-gradient(0deg, rgb(16 9 13 / .55), transparent 63%); }
  .hero-copy { padding: clamp(1.15rem, 3.4vw, 2.8rem); }
  h1 { max-width: 15ch; margin: 0 0 1.3rem; font: inherit; font-size: clamp(2.25rem, 5vw, 4.75rem); font-weight: 680; line-height: .99; letter-spacing: -.06em; text-wrap: balance; }
  .hero-copy nav { display: flex; gap: .45rem; flex-wrap: wrap; }
  .hero-copy nav a { display: inline-flex; gap: .4rem; align-items: center; justify-content: center; min-height: 2.6rem; padding: .4rem .9rem; border: 1px solid rgb(255 255 255 / .52); border-radius: 99px; background: rgb(20 12 13 / .72); color: #fffaf4; font-size: .84rem; font-weight: 600; }
  .hero-copy nav a:hover, .hero-copy nav a[aria-current] { background: #fff6e6; color: #251b14; border-color: #fff6e6; }
  .section-title { margin: 0; font-size: clamp(1.45rem, 2.65vw, 2.1rem); line-height: 1.12; font-weight: 650; letter-spacing: -.04em; }
  .see-all { display: inline-flex; align-items: center; gap: .35rem; text-align: end; font-size: .82rem; font-weight: 600; }
  .see-all:hover { text-decoration: underline; }
  @media (max-width: 550px) { .broadcast-hero { min-height: 14.25rem; } .hero-copy nav a { padding-inline: .65rem; font-size: .76rem; } .hero-copy nav { gap: .33rem; } }
</style>
