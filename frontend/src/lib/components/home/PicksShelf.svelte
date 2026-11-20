<script lang="ts">
  import { Tabs } from "bits-ui";
  import { ArrowDownUp, ChevronDown, LayoutGrid, List, Pause, Play, RefreshCw, Search, Tag } from "lucide-svelte";
  import type { Snippet } from "svelte";
  import { Input } from "$lib/components/ui/input";
  import CatalogArtwork from "$lib/components/catalog/CatalogArtwork.svelte";
  import { getLocale, t } from "$lib/i18n/index.svelte";
  import { canPlay } from "$lib/catalog/availability";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import type { Track } from "$lib/types";
  import type { createCatalog } from "./catalog.svelte";
  import { homeCopy } from "./copy";
  import SessionTracks from "./SessionTracks.svelte";

  type Entry = { id: string; title: string; detail?: string | null; image?: string | null; href?: string; track?: Track };
  let { tracks, catalog, children }: { tracks: Track[]; catalog: ReturnType<typeof createCatalog>; children: Snippet } = $props();

  const tabs = [["tracks", "workspaceHome.tracks"], ["albums", "explore.albums"], ["artists", "explore.artists"]] as const;
  let active = $state("tracks");
  let preferredView = $state<"grid" | "list">();
  const view = $derived(preferredView ?? (active === "tracks" && !tracks.some(track => track.cover_url) ? "list" : "grid"));
  let search = $state("");
  let genre = $state("");
  let sort = $state("original");
  const player = getPlayerStore();
  const queue = getQueueStore();
  const copy = $derived(homeCopy());
  const genres = $derived([...new Set(tracks.map(track => track.genre).filter((genre): genre is string => !!genre))].sort());
  const preview = $derived(catalog.offline ? tracks[0] : undefined);
  const entries = $derived.by((): Entry[] => {
    if (active === "albums") {
      if (preview?.album_title) return [{ id: "preview-album", title: preview.album_title, detail: preview.artist_name, image: preview.cover_url }];
      return (catalog.albums.data ?? []).map(album => ({
        id: album.id, title: album.title, detail: album.artist_name, image: album.cover_url, href: "/albums/" + encodeURIComponent(album.id),
      }));
    }
    if (active === "artists") {
      if (preview?.artist_name) return [{ id: "preview-artist", title: preview.artist_name, detail: t("track.artist") }];
      return (catalog.artists.data ?? []).map(artist => ({
        id: artist.id, title: artist.name, detail: t("track.artist"), image: artist.image_url, href: "/artists/" + encodeURIComponent(artist.id),
      }));
    }
    return tracks.map(track => ({
      id: track.id, title: track.title, detail: track.artist_name, image: track.cover_url, track,
    }));
  });
  const needle = $derived(search.trim().toLocaleLowerCase(getLocale()));
  const useGenre = $derived(active === "tracks" && genres.includes(genre));
  const filtered = $derived.by(() => {
    const items = entries.filter(item => (!useGenre || item.track?.genre === genre) &&
      [item.title, item.detail].some(value => value?.toLocaleLowerCase(getLocale()).includes(needle)));
    if (sort === "name") items.sort((a, b) => a.title.localeCompare(b.title, getLocale(), {sensitivity: "base", numeric: true}));
    return items;
  });
  const filteredTracks = $derived(filtered.flatMap(item => item.track ? [item.track] : []));
  const loading = $derived(active === "tracks" ? catalog.loading : active === "albums" ? catalog.albums.loading : catalog.artists.loading);

  function selectTab(value: string) {
    active = value;
    search = "";
    genre = "";
  }

  function play(entry: Entry, index: number) {
    if (!entry.track) queue.playQueue(tracks, 0, "collection");
    else if (entry.track.id === player.currentTrack?.id) player.togglePlay();
    else queue.playQueue(filteredTracks, index, "explore");
  }
</script>

<Tabs.Root value={active} onValueChange={selectTab}>
  <Tabs.List class="home-tabs" aria-label={copy.title}>
    {#each tabs as [value, label] (value)}
      <Tabs.Trigger {value} class="home-tab">{t(label)}</Tabs.Trigger>
    {/each}
  </Tabs.List>
  {#each tabs as [value] (value)}
    <Tabs.Content {value} tabindex={-1}>
      {#if active === value}
        <div class="toolbar">
          <label class="filter-input">
            <Search size={16} strokeWidth={1.7} />
            <Input type="search" bind:value={search} class="h-8 rounded-lg pl-9 text-sm shadow-none" placeholder={t("workspaceHome.filter")} aria-label={t("workspaceHome.filter")} />
          </label>
          {#if active === "tracks" && genres.length > 1}
            <label class="select-control bordered">
              <Tag size={14} /> <span>{useGenre ? genre : t("track.genre")}</span> <ChevronDown size={12} />
              <select bind:value={genre} aria-label={t("track.genre")}>
                <option value="">{t("workspaceHome.allGenres")}</option>
                {#each genres as option (option)}<option value={option}>{option}</option>{/each}
              </select>
            </label>
          {/if}
          <div class="display-tools">
            {#if active === "tracks" && !catalog.offline}
              <button type="button" class="refresh" onclick={catalog.picks.retry} disabled={catalog.picks.loading} title={t("home.refreshRandom")} aria-label={t("home.refreshRandom")}><RefreshCw size={15} /></button>
            {/if}
            <label class="select-control sort">
              <ArrowDownUp size={15} /><span>{sort === "name" ? t("workspaceHome.byName") : t("workspaceHome.selectionOrder")}</span>
              <select aria-label={t("workspaceHome.sort")} bind:value={sort}>
                <option value="original">{t("workspaceHome.selectionOrder")}</option>
                <option value="name">{t("workspaceHome.byName")}</option>
              </select>
            </label>
            <div class="view-switch">
              <button aria-label={copy.list} title={copy.list} aria-pressed={view === "list"} onclick={() => preferredView = "list"}><List size={15} /></button>
              <button aria-label={copy.grid} title={copy.grid} aria-pressed={view === "grid"} onclick={() => preferredView = "grid"}><LayoutGrid size={14} /></button>
            </div>
          </div>
        </div>
        {#if !filtered.length}
          <div class="empty" role="status">
            {loading ? t("common.loading") : needle || useGenre ? t("common.noResults") :
              active === "tracks" ? t("explore.noTracks") : active === "albums" ? t("explore.noAlbums") : t("explore.noArtists")}
            {#if needle || useGenre}
              <button type="button" onclick={() => { search = ""; genre = ""; }}>{t("workspaceHome.reset")}</button>
            {/if}
          </div>
        {:else if active === "tracks" && view === "list"}
          <SessionTracks tracks={filteredTracks} source="explore" />
        {:else}
          <div class={["entries", { rows: view === "list" }]}>
            {#each filtered as entry, index (entry.id)}
              {@const playing = !!entry.track && entry.id === player.currentTrack?.id && player.isPlaying}
              {#snippet contents()}
                <span class="art" aria-hidden="true">
                  <CatalogArtwork source={entry.image} artworkId={entry.track?.album_id ?? entry.id} kind={active === "artists" ? "artist" : "album"} title={entry.title} />
                  {#if !entry.href}
                    <span class={["play-icon", { playing }]}>
                      {#if playing}<Pause size={13} fill="currentColor" />{:else}<Play size={13} fill="currentColor" />{/if}
                    </span>
                  {/if}
                </span>
                <span class="caption"><strong>{entry.title}</strong>{#if entry.detail}<small>{entry.detail}</small>{/if}</span>
              {/snippet}
              {#if entry.href}
                <a href={entry.href} class="entry">{@render contents()}</a>
              {:else}
                <button type="button" class="entry" disabled={entry.track && !canPlay(entry.track)} aria-label={[playing ? copy.pause : copy.play, entry.title, entry.detail].filter(Boolean).join(" · ")} onclick={() => play(entry, index)}>{@render contents()}</button>
              {/if}
            {/each}
          </div>
        {/if}
        {#if active === "tracks" && !needle && !useGenre}{@render children()}{/if}
      {/if}
    </Tabs.Content>
  {/each}
</Tabs.Root>

<style>
  :global(.home-tabs) { display: flex; gap: 1.5rem; border-bottom: 1px solid hsl(var(--border)); }
  :global(.home-tab) { margin-bottom: -1px; padding: 9px 0 10px; border-bottom: 2px solid transparent; color: hsl(var(--muted-foreground)); font-size: .875rem; font-weight: 500; }
  :global(.home-tab[data-state="active"]) { border-color: hsl(var(--foreground)); color: hsl(var(--foreground)); }
  .toolbar, .display-tools, .select-control, .view-switch { display: flex; align-items: center; }
  .toolbar { flex-wrap: wrap; gap: 8px; margin-block: 16px; }
  .filter-input { position: relative; width: min(100%, 250px); }
  .filter-input > :global(svg) { position: absolute; z-index: 1; top: 8px; left: 11px; pointer-events: none; color: hsl(var(--muted-foreground)); }
  .select-control, .refresh { position: relative; justify-content: center; height: 32px; gap: 7px; padding-inline: 9px; border-radius: .55rem; color: hsl(var(--muted-foreground)); font-size: .81rem; font-weight: 500; }
  .select-control:focus-within { outline: 2px solid hsl(var(--ring)); outline-offset: 2px; }
  .select-control select { position: absolute; inset: 0; width: 100%; height: 100%; cursor: pointer; opacity: 0; }
  .select-control.bordered { border: 1px solid hsl(var(--input)); }
  .display-tools { margin-inline-start: auto; gap: 7px; }
  .view-switch { padding: 2px; border-radius: .55rem; background: hsl(var(--muted)); }
  .view-switch button { display: grid; width: 29px; height: 27px; place-items: center; border: 1px solid transparent; border-radius: .4rem; color: hsl(var(--muted-foreground)); }
  .view-switch [aria-pressed="true"] { color: hsl(var(--foreground)); background: hsl(var(--card)); border-color: hsl(var(--border)); }
  .entries { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: clamp(.85rem, 2.65cqi, 1.8rem); }
  .entry { display: flex; flex-direction: column; min-width: 0; gap: 4px; border-radius: .65rem; text-align: left; }
  .entry:hover strong { text-decoration: underline; }
  .art { position: relative; display: grid; width: 100%; aspect-ratio: 1; overflow: hidden; place-items: center; border-radius: .65rem; background: hsl(var(--muted)); color: hsl(var(--muted-foreground)); }
  .art :global(.artwork) { min-width: 0; min-height: 0; border-radius: inherit; }
  .play-icon { position: absolute; right: 8px; bottom: 8px; display: grid; width: 28px; height: 28px; place-items: center; border-radius: 50%; background: hsl(var(--background)); color: hsl(var(--foreground)); opacity: 0; }
  .entry:hover .play-icon, .entry:focus-visible .play-icon, .play-icon.playing { opacity: 1; }
  .caption { display: grid; min-width: 0; gap: 2px; padding: 4px 1px; }
  .caption strong, .caption small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .caption strong { font-size: .88rem; font-weight: 550; }
  .caption small { font-size: .76rem; color: hsl(var(--muted-foreground)); }
  .rows { display: grid; grid-template-columns: 1fr; gap: 0; }
  .rows .entry { flex-direction: row; align-items: center; gap: .7rem; padding-block: .45rem; border: 0; border-radius: 0; }
  .rows .entry + .entry { border-top: 1px solid hsl(var(--border)); }
  .rows .art { width: 42px; aspect-ratio: 1; }
  .empty { display: flex; align-items: center; justify-content: center; flex-wrap: wrap; gap: 10px; min-height: 150px; color: hsl(var(--muted-foreground)); font-size: .875rem; }
  .empty button { text-decoration: underline; }
  @media (max-width: 1100px) { .filter-input { width: 210px; } }
  @container (max-width: 560px) { .entries:not(.rows) { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @media (max-width: 620px) {
    .toolbar { gap: 6px; margin-block: 12px; }
    .filter-input { width: 100%; }
    .display-tools { gap: 2px; }
    .select-control { padding-inline: 6px; }
    .entries { gap: 8px; }
    .caption { padding: 4px 2px; }
  }
</style>
