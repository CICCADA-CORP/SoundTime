<script lang="ts">
  import { ListMusic, Pause, Play } from "lucide-svelte";
  import { Button } from "$lib/components/ui/button";
  import CatalogArtwork from "$lib/components/catalog/CatalogArtwork.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { accountLink } from "$lib/access/navigation";
  import { formatDuration } from "$lib/utils";
  import type { PlaybackSource, Track } from "$lib/types";
  import { homeCopy } from "./copy";
  import { spotlightTracks } from "./spotlights";
  import SessionTracks from "./SessionTracks.svelte";

  let { title, tracks = [], source = "collection", empty = false, loading = false, playAll = false }: {
    title: string; tracks?: Track[]; source?: PlaybackSource; empty?: boolean;
    loading?: boolean; playAll?: boolean;
  } = $props();
  const id = $props.id();
  const queue = getQueueStore();
  const player = getPlayerStore();
  const auth = getAuthStore();
  const copy = $derived(homeCopy());
  let expanded = $state(false);
  const selected = $derived(spotlightTracks(tracks));
  const listeningOrder = $derived(playAll ? tracks : [...selected, ...tracks.filter(track => !selected.includes(track))]);
  function play(track: Track) {
    if (player.currentTrack?.id === track.id) player.togglePlay();
    else queue.playQueue(listeningOrder, listeningOrder.indexOf(track), source);
  }
</script>

<section aria-label={title} class="spotlights">
  <header>
    <h2>{title}</h2>
    {#if tracks.length && !loading}
      <div class="actions">
        {#if playAll}<button onclick={() => queue.playQueue(tracks, 0, source)}><Play size={13} />{copy.play}</button>{/if}
        <button aria-label={copy.tracklist} title={copy.tracklist} aria-expanded={expanded} aria-controls={expanded ? id : undefined} onclick={() => expanded = !expanded}><ListMusic size={17} /></button>
      </div>
    {/if}
  </header>
  {#if loading}
    <p class="sr-only" role="status">{t("common.loading")}</p>
    <div class="cards" aria-hidden="true">{#each [0, 1, 2] as n (n)}<div class="skeleton"></div>{/each}</div>
  {:else if selected.length}
    <div class="cards">
      {#each selected as track (track.id)}
        {@const playing = player.currentTrack?.id === track.id && player.isPlaying}
        <button type="button" class="track-card" aria-label={(playing ? copy.pause : copy.play) + " · " + track.title}
          onclick={() => play(track)}>
          <span class="image" aria-hidden="true">
            <CatalogArtwork source={track.cover_url} artworkId={track.album_id ?? track.id} kind="album" title={track.title} loading="eager" />
          </span>
          <span class="caption">
            <span class="credit"><strong>{track.title}</strong>{#if track.artist_name}<small>{track.artist_name}</small>{/if}</span>
            {#if track.duration_secs}<span class="duration">{formatDuration(track.duration_secs)}</span>{/if}
          </span>
          <span class="transport" class:playing aria-hidden="true">
            {#if playing}<Pause size={17} fill="currentColor" />{:else}<Play size={17} fill="currentColor" />{/if}
          </span>
        </button>
      {/each}
    </div>
    {#if expanded}<div id={id} class="track-list"><SessionTracks tracks={listeningOrder} {source} /></div>{/if}
  {:else}
    <div class="empty">
      <p>{empty ? copy.empty : t("explore.noTracks")}</p>
      {#if empty}
        <Button href={auth.isAuthenticated ? "/upload" : accountLink("/login", "/upload")}>
          {t(auth.isAuthenticated ? "home.uploadFirst" : "shell.signInUpload")}
        </Button>
      {/if}
    </div>
  {/if}
</section>

<style>
  .spotlights { min-width: 0; container-type: inline-size; }
  header, .actions, .actions button { display: flex; align-items: center; }
  header { justify-content: space-between; gap: .65rem; min-height: 2rem; margin-bottom: .7rem; }
  h2 { font-size: clamp(1.06rem, 2.6cqi, 1.4rem); font-weight: 630; letter-spacing: -.028em; line-height: 1.2; overflow-wrap: anywhere; }
  .actions { gap: .5rem; }
  .actions button { flex: none; gap: .35rem; min-height: 2rem; padding-inline: .42rem; border-radius: .55rem; font-size: .78rem; color: hsl(var(--muted-foreground)); }
  .actions button:hover { color: hsl(var(--foreground)); background: hsl(var(--muted) / .75); }
  .cards { display: grid; grid-auto-flow: column; grid-auto-columns: calc((100% - 2 * var(--gap)) / 3);
    --gap: clamp(.6rem, 2.1cqi, 1rem); gap: var(--gap); overflow-x: auto; scrollbar-width: none; scroll-snap-type: x proximity; }
  .cards::-webkit-scrollbar { display: none; }
  .track-card, .skeleton { position: relative; isolation: isolate; min-width: 0; aspect-ratio: .97; overflow: hidden; border-radius: var(--radius-card, .85rem); scroll-snap-align: start; }
  .track-card:focus-visible { outline-offset: -3px; }
  .image { position: absolute; inset: 0; display: block; border-radius: inherit; }
  .caption { position: absolute; inset: auto 0 0; display: flex; align-items: end; gap: .5rem; justify-content: space-between; min-height: 29%;
    padding: .65rem clamp(.55rem, 1.55cqi, .9rem); text-align: start; color: white;
    background: linear-gradient(180deg, rgb(13 19 31 / .60), rgb(12 18 29 / .76)); backdrop-filter: blur(18px); }
  .credit { display: grid; min-width: 0; gap: 2px; }
  .credit strong, .credit small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .credit strong { font-size: clamp(.82rem, 1.7cqi, 1rem); font-weight: 600; line-height: 1.25; }
  .credit small, .duration { font-size: clamp(.7rem, 1.45cqi, .79rem); color: rgb(255 255 255 / .88); line-height: 1.3; }
  .duration { flex: none; font-variant-numeric: tabular-nums; }
  .transport { position: absolute; top: .65rem; right: .65rem; width: 2.1rem; height: 2.1rem; display: grid; place-items: center; border-radius: 50%; color: white; background: rgb(17 22 33 / .70); opacity: 0; }
  .track-card:hover .transport, .track-card:focus-visible .transport, .transport.playing { opacity: 1; }
  .skeleton { background: hsl(var(--muted) / .8); }
  .track-list { margin-top: 1rem; }
  .empty { display: grid; justify-items: start; align-content: center; gap: 1rem; min-height: 7rem; color: hsl(var(--muted-foreground)); font-size: .9rem; }
  @container (max-width: 475px) { .cards { grid-auto-columns: min(70%, 12.5rem); } }
  @media (hover: none) { .transport { width: 1.8rem; height: 1.8rem; opacity: 1; } }
</style>
