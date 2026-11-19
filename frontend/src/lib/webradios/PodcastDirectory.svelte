<script lang="ts">
  import { page } from "$app/state";
  import { ArrowLeft, ArrowRight, ExternalLink, Pause, Play, Search } from "lucide-svelte";
  import { api } from "$lib/api";
  import { catalogSection } from "$lib/catalog/query.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import { Button } from "$lib/components/ui/button";
  import { getLocale, t } from "$lib/i18n/index.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { formatDuration } from "$lib/utils";
  import { podcast, type PodcastEpisode, type PodcastShow } from "./podcast.svelte";
  import PodcastCover from "./PodcastCover.svelte";

  let { compact = false }: { compact?: boolean } = $props();
  const player = getPlayerStore();
  const query = $derived((page.url.searchParams.get("q") ?? "").trim().slice(0, 160));
  const showId = $derived(page.url.searchParams.get("show") ?? "");
  const country = $derived.by(() => {
    const value = page.url.searchParams.get("country")?.toUpperCase();
    return value && /^[A-Z]{2}$/.test(value) ? value : new Intl.Locale(getLocale()).maximize().region ?? "US";
  });
  const offset = $derived.by(() => {
    const value = Number(page.url.searchParams.get("offset"));
    return Number.isSafeInteger(value) && value >= 0 ? value : 0;
  });
  const marketNames = $derived(new Intl.DisplayNames([getLocale()], { type: "region" }));
  const regions = ("AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ " +
    "CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR " +
    "GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT " +
    "JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO " +
    "MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY " +
    "QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO " +
    "TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW").split(" ");
  const markets = $derived(Array.from(new Set([...regions, country])).map(code => ({ code, name: marketNames.of(code) ?? code }))
    .sort((a, b) => a.name.localeCompare(b.name, getLocale())));
  const dateFormat = $derived(new Intl.DateTimeFormat(getLocale(), {day:"numeric", month:"short", year:"numeric"}));
  function link(id = "", start = 0) {
    const params = new URLSearchParams({ view: "podcasts", country });
    if (query) params.set("q", query);
    if (id) params.set("show", id);
    if (start) params.set("offset", String(start));
    return "/webradios?" + params;
  }
  const shows = catalogSection(signal => showId ? null : api.get<{shows: PodcastShow[]; next_offset: number | null}>(
    "/webradios/podcasts?" + new URLSearchParams({q: compact ? "" : query, country, offset: String(compact ? 0 : offset)}), signal));
  const detail = catalogSection(signal => !/^\d{1,20}$/.test(showId) ? null : api.get<{
    show: PodcastShow; episodes: PodcastEpisode[]; next_offset: number | null; truncated: boolean;
  }>(`/webradios/podcasts/${showId}/episodes?` + new URLSearchParams({country, offset: String(offset)}), signal));
  function play(show: PodcastShow, episode: PodcastEpisode) {
    if (podcast.show?.id !== show.id || podcast.episode?.id !== episode.id || podcast.phase === "failed" || podcast.phase === "unsupported") player.playPodcast(show, episode, country);
    else if (podcast.phase === "playing") podcast.pause();
    else podcast.resume();
  }
</script>

<section class="podcasts space-y-4" aria-labelledby="podcast-heading">
  {#if showId}
    <a class="section-link" href={link()}><ArrowLeft size={16} />{t("broadcast.podcasts")}</a>
    {#if detail.data}
      {@const show = detail.data.show}
      <div class="show-heading">
        <div class="show-art"><PodcastCover source={show.artwork_url}/></div>
        <div class="min-w-0 space-y-2">
          {#if show.genre}<p class="text-xs font-medium text-muted-foreground">{show.genre}</p>{/if}
          <h2 id="podcast-heading">{show.title}</h2>
          {#if show.author}<p class="text-sm">{show.author}</p>{/if}
          <a class="section-link" href={`https://podcasts.apple.com/${country.toLowerCase()}/podcast/id${show.id}`} target="_blank" rel="noreferrer">Apple Podcasts <ExternalLink size={13} /></a>
        </div>
      </div>
      <h3 class="text-lg font-semibold">{t("broadcast.episodes")}</h3>
      {#if detail.data.episodes.length}
        <ol class="divide-y divide-border border-y border-border">
          {#each detail.data.episodes as episode (episode.id)}
            {@const playing = podcast.show?.id === show.id && podcast.episode?.id === episode.id && podcast.phase === "playing"}
            <li class="episode">
              <div class="min-w-0 space-y-1">
                {#if episode.published_at || episode.duration}
                  <div class="flex flex-wrap gap-2 text-xs text-muted-foreground">
                    {#if episode.published_at && Number.isFinite(Date.parse(episode.published_at))}
                      <time datetime={episode.published_at}>{dateFormat.format(new Date(episode.published_at))}</time>
                    {/if}
                    {#if episode.duration}<span>{formatDuration(episode.duration)}</span>{/if}
                  </div>
                {/if}
                <h4 class="break-words text-base font-semibold">{episode.title}</h4>
                {#if episode.description}<p class="line-clamp-2 break-words text-sm text-muted-foreground">{episode.description}</p>{/if}
              </div>
              <button class="episode-play" type="button" aria-label={t(playing ? "webradios.podcastPause" : "webradios.podcastPlay", {title: episode.title})}
                onclick={() => play(show, episode)}>{#if playing}<Pause size={17} />{:else}<Play size={17} fill="currentColor" />{/if}</button>
            </li>
          {/each}
        </ol>
        {#if offset || detail.data.next_offset != null}
          <nav class="flex justify-between" aria-label={t("broadcast.episodes")}>
            {#if offset}<Button href={link(showId, Math.max(0,offset-50))} variant="outline"><ArrowLeft size={15} />{t("admin.reports.previous")}</Button>{/if}
            {#if detail.data.next_offset != null}<Button class="ml-auto" href={link(showId,detail.data.next_offset)} variant="outline">{t("admin.reports.next")}<ArrowRight size={15} /></Button>{/if}
          </nav>
        {/if}
        {#if detail.data.truncated}<p class="text-sm text-muted-foreground">{t("broadcast.archiveLimit")}</p>{/if}
      {:else}<CatalogState message={t("broadcast.noEpisodes")} />{/if}
    {:else}
      <h2 id="podcast-heading" class="sr-only">{t("broadcast.episodes")}</h2>
      <CatalogState loading={detail.loading} failed={detail.failed || !/^\d{1,20}$/.test(showId)} status={detail.status} retry={detail.retry} />
    {/if}
  {:else}
    <div class="flex items-end justify-between gap-3">
      <h2 id="podcast-heading">{compact || !query ? t("broadcast.topPodcasts") : t("broadcast.podcasts")}</h2>
      {#if compact}<a class="section-link" href={link()}>{t("broadcast.allPodcasts")} <ArrowRight size={15}/></a>{/if}
    </div>
    {#if !compact}
      <form method="GET" action="/webradios" class="podcast-search">
        <input type="hidden" name="view" value="podcasts" />
        <label><span>{t("broadcast.searchPodcasts")}</span><input type="search" name="q" value={query} maxlength="160" /></label>
        <label><span>{t("broadcast.store")}</span><select name="country" value={country}>
          {#each markets as market (market.code)}<option value={market.code}>{market.name}</option>{/each}
        </select></label>
        <Button type="submit" class="h-10"><Search size={16}/>{t("admin.reports.searchBtn")}</Button>
      </form>
    {/if}
    {#if shows.data?.shows?.length}
      <div class="cover-grid" class:featured={compact}>
        {#each (compact ? shows.data.shows.slice(0,5) : shows.data.shows) as show (show.id)}
          <a class="show-card" href={link(show.id)}>
            <PodcastCover source={show.artwork_url}/>
            <h3 class="line-clamp-2">{show.title}</h3>
            {#if show.author || show.genre}<p class="truncate text-xs text-muted-foreground">{show.author || show.genre}</p>{/if}
          </a>
        {/each}
      </div>
      {#if !compact && (offset || shows.data.next_offset != null)}
        <nav class="flex justify-between" aria-label={t("broadcast.podcasts")}>
          {#if offset}<Button href={link("", Math.max(0,offset-25))} variant="outline"><ArrowLeft size={15} />{t("admin.reports.previous")}</Button>{/if}
          {#if shows.data.next_offset != null}<Button class="ml-auto" href={link("",shows.data.next_offset)} variant="outline">{t("admin.reports.next")}<ArrowRight size={15} /></Button>{/if}
        </nav>
      {/if}
    {:else}<CatalogState loading={shows.loading} failed={shows.failed} status={shows.status} retry={shows.retry} message={t("common.noResults")} layout="shelf" />{/if}
  {/if}
</section>

<style>
  h2 { font-size: clamp(1.45rem, 2.65vw, 2.1rem); line-height: 1.12; font-weight: 650; letter-spacing: -.04em; overflow-wrap: anywhere; }
  .section-link { display: inline-flex; gap: .35rem; align-items: center; font-size: .82rem; font-weight: 600; }
  .section-link:hover, .show-card:hover h3 { text-decoration: underline; }
  .cover-grid { display: grid; grid-template-columns: repeat(auto-fill,minmax(min(9rem,45%),1fr)); gap: 1.1rem; }
  .featured { grid-template-columns: repeat(5,minmax(0,1fr)); }
  .show-card { min-width: 0; display: block; }
  .show-card h3 { margin: .65rem 0 .15rem; font-size: .94rem; line-height: 1.25; font-weight: 650; overflow-wrap: anywhere; }
  .podcast-search { display: grid; grid-template-columns: minmax(0,2fr) minmax(6rem,1fr) auto; align-items: end; gap: .75rem; }
  .podcast-search label { display: grid; gap: .25rem; font-size: .875rem; font-weight: 500; }
  .podcast-search input:not([type="hidden"]), .podcast-search select { min-width: 0; width: 100%; height: 2.5rem; border: 1px solid hsl(var(--input)); border-radius: .45rem; background: hsl(var(--background)); padding-inline: .75rem; }
  .show-heading { display: flex; align-items: center; gap: clamp(1rem,3vw,2rem); }
  .show-art { width: clamp(6rem,16vw,11rem); flex-shrink: 0; }
  .episode { display: flex; justify-content: space-between; align-items: center; gap: 1rem; padding-block: 1rem; }
  .episode-play { flex-shrink: 0; display: grid; place-items: center; width: 2.75rem; height: 2.75rem; border-radius: 100%; background: hsl(var(--foreground)); color: hsl(var(--background)); }
  @media (max-width: 767px) { .featured { display: grid; grid-auto-flow: column; grid-template-columns: none; grid-auto-columns: minmax(8rem,39%); overflow-x: auto; padding: .2rem .1rem .5rem; scroll-snap-type: x proximity; } .featured .show-card { scroll-snap-align: start; } }
  @media (max-width: 550px) { .podcast-search { grid-template-columns: minmax(0,1.35fr) minmax(0,1fr); } .podcast-search :global(button) { grid-column: 1/-1; } }
</style>
